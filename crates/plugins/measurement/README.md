# epdf-plugin-measurement

`packages/plugin/measurement` in Rust, ported to the current TypeScript (Task 13.22, then upstream
`8bc6546e5`). `create()` builds the controller. A page's viewports come with its layout
(`PageLayout.viewports`), so a page's scale is known once the document opens and nothing reads
them. A scale change is one change of the document: its `pages.setScale` ops and the annotation
plugin's `stageRemeasure` of the page's measurements, staged in one group. The settings are
registered with the kernel (`defaultScale`, replaced whole, and `presets`). 59 cases replay
(`cargo test -p epdf-plugin-measurement`), with no member-order allowance:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/measurement.json` | scenario | 18 | page scale from owned, foreign and default viewports on the layout, `pages.scaleSet` events moving the layout, single- and multi-page changes with re-measurement, the write preflight, units, area units, precision and presets, the calibration flow, readouts, `createMeasurement`, authority, a throwing listener, inserted and deleted pages, a page removed while a change waits, and closing the document mid-change |
| `vectors/plugins/measurement-functions.json` | function | 41 | the named state transitions, `DEFAULT_PRESETS`, `defaultMeasure`, `selectPageScale`, `withUnit`, `withAreaUnit` and `withPrecision` |

The scenarios run the real interaction and annotation adapters beside this one, because the plugin
requires both. The generator is `tooling/plugin-conformance/src/suites/measurement.ts`.

The conformance fake gives every page a `measure` service, as the TypeScript fake does
(`fake-engine.ts:678`; Ruling13.22, `83fb65435`).

`tests/behavior.rs` covers what the brief asks beyond the printed traces: a calibration capture
that ends where the drag's last `move` left it (the `up` does not move it,
`packages/core/annotation/src/update/draw.ts:203`), `calibrate` refusing lengths that are not finite
and positive before anything reaches the engine, a close while two changes are on their way, where
the all-pages change resolves one `scaleError` per page and the single-page change rejects
`instance-closed`, and a download that waits for a scale change on its way.

## The public API

`measurement_plugin(MeasurementConfig)` builds the plugin (`measurement_plugin_json` takes the
registered settings as JSON), `MEASUREMENT` is its token and `MeasurementCapability` its capability,
which is also `MeasurementHostCapability`. The members are the TypeScript's in snake case:
`get_settings`, `update_settings`, `reset_settings`, `on_settings_changed`, `can_measure`,
`get_page_scale`, `is_busy`, `list_last_reports`, `list_presets`, `list_units`, `list_area_units`,
`get_readout`, `measure_distance`, `measure_area`, `get_calibration_request`, `set_scale`,
`calibrate`, `set_unit`, `set_area_unit`, `set_precision`, `set_preset`, `clear_scale`,
`start_calibration`, `dismiss_calibration`, `create_measurement`, `can_calibrate`, `ensure_loaded`
and the four `on_*` events.

Measures, viewports, readouts and re-measurement reports are the engine's records as JSON (`Value`),
so a member the caller wrote survives a round trip, and the write preflight
(`assertWritableMeasure`, annotation-core's) refuses a malformed measure with the TypeScript's
message. A page argument is `PageInput`, a `PageRef` or a display index; `PageTarget` adds `All` and a
list. `get_page_scale` returns an `Rc<PageScale>` that stays the same while the page list, the
session's own viewports and the default scale do. It is a `Result` because the TypeScript throws
there: when the `defaultScale` setting cannot be built in the page's user unit, and when the registry
still names the page after the document handle is gone.

## Migrating from the pre-13.22 API

For the viewer host and `epdf-web` (Task 13.33).

| Old | New |
|---|---|
| `measurement_plugin(MeasurementConfig { default_scale, presets, host })` | the same without `host`; a throwing listener goes to the kernel's report |
| `DefaultScale::Measure(Box<PdfMeasurement>)` | `DefaultScale::Measure(Value)` |
| typed `PdfMeasurement`, `MeasurementViewport`, `RecalibrationReport` | `Value` records |
| `PageScale { measure: Option<PdfMeasurement>, source: ScaleSource, .. }` | `measure: Value` (`null` for none), `source: &'static str` |
| `PageTarget::Page(PageRef)`, `Pages(Vec<PageRef>)` | `PageTarget::Page(PageInput)`, `Pages(Vec<PageInput>)` |
| `get_page_scale(&PageRef) -> PageScale` | `get_page_scale(page) -> Result<Rc<PageScale>, Error>`; `can_measure`, `measure_distance` and `measure_area` are fallible the same way |
| `ensure_loaded(&PageRef)` | `ensure_loaded(page, OperationOptions)` |
| `CalibrateOptions { recalculate, apply_to }`, `KnownDistance` | `CalibrateOptions { change: ScaleChangeOptions, apply_to }`, `KnownLength` |
| `create_measurement(input)` resolving a ref | `create_measurement(input, OperationOptions)` resolving `{ annotation }` |
| `get_readout(&AnnotationRef) -> Readout` | `get_readout(&Value) -> Value` |
| `MeasurementState { pages, pending, calibration, reports }`, `MeasurementAction`, `measurement_reducer`, `PageEntry` | `MeasurementState { pending, calibration, reports, local_viewports }` and the named transitions in `model`; the viewports live on the page layout (`load_errors`, `clear_load_error`, `record_load_error` and `page_scale_of` went with upstream `8bc6546e5`) |
| `units::measure_from_ratio_typed`, `decode_measure`, `range_error` | removed; `units` answers JSON |
| ffi `ScaleChangeOptions` | ffi `MeasurementScaleChangeOptions` |
| ffi `AnnotationRef` in readouts and reports | `annotation_json` strings |

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task
13.33. Their fake answers a scale change's `apply` and publishes `pages.scaleSet` with the new layout.

## Defensive absent-service branches

`sync/viewports.ts` (`viewportsOf`) reads a page's session-only viewports, and `write/scale.ts`
(`writeLocal`) writes them, when `doc.page(page).measure` is undefined. The current engine contract requires the service, and
the TypeScript fake supplies it on every page, so no vector reaches either branch: the vectors cover
the required service and its refusals. The port keeps both branches line for line
(`sync/viewports.rs::viewports_of`, `write/scale.rs::write_local`) without a case of its own, as the Task 12b ruling on
malformed absent services asks. `tests/ffi.rs` `without_a_service_the_scale_is_in_memory` runs the
in-memory branch over a test kernel whose handle answers `has("page.measure")` false
(`TestSpec::lacking`, Task 13.33).

## Where the port differs

Kept Rust-only divergences, none reached by a vector:

- `withUnit` and `withAreaUnit` test the base unit with `basis in METRES`, which also accepts
  `Object.prototype` keys such as `constructor`. The port looks the unit up in the table.
- A base unit that is not a string throws a `TypeError` whose message is the port's own; the
  TypeScript's names its own call site. `withPrecision` over a format list that is not an array does
  the same.
- `get_readout` computes the readout per call. The TypeScript caches it per annotation object, which
  only shows as identity; the annotation plugin hands Rust a fresh record per read.
- A captured calibration draft whose page is not a `PageRef` makes no request; the TypeScript would
  store it.
- A malformed measure is read as annotation-core reads it, loosely, where its README lists the kept
  differences.
- The typed `get_settings` and `list_presets` refuse a `presets` setting that is not a list of
  presets with a `TypeError`; the TypeScript hands back whatever was written, as `settings_json`
  still does.

These are registered in `docs/research/rust-plugins-branch-review.md`, "Status".

`connect_viewports` tells the annotation plugin a page's viewports again when the list differs by
value; the TypeScript compares the array's identity. The annotation plugin keeps the last list it
was told, so the calls it skips change nothing.
