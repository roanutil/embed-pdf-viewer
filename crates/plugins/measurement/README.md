# epdf-plugin-measurement

`packages/plugin/measurement` in Rust, ported to the current TypeScript (Task 13.22). `create()`
builds the controller, each page's viewports are a kernel page mirror of the engine's
`page.measure.listViewports`, re-read on a confirmed `pages.scaleSet`, and a scale write goes through
`page.measure.setScale` before the annotation plugin re-measures the page. The settings are
registered with the kernel (`defaultScale`, replaced whole, and `presets`). 64 cases replay
(`cargo test -p epdf-plugin-measurement`), with no member-order allowance:

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/measurement.json` | scenario | 19 | page scale from owned, foreign and default viewports, refused reads and retries, refresh epochs, `stream.desynced` and `document.versioned`, single- and multi-page writes with re-measurement, the write preflight, units, area units, precision and presets, the calibration flow, readouts, `createMeasurement`, authority, a throwing listener, inserted and deleted pages, and closing the document mid-change |
| `vectors/plugins/measurement-functions.json` | function | 45 | the named state transitions, `DEFAULT_PRESETS`, `defaultMeasure`, `selectPageScale`, `pageScaleOf`, `withUnit`, `withAreaUnit` and `withPrecision` |

The scenarios run the real interaction and annotation adapters beside this one, because the plugin
requires both. The generator is `tooling/plugin-conformance/src/suites/measurement.ts`.

The conformance fake gives every page a `measure` service, as the TypeScript fake does
(`fake-engine.ts:678`; Ruling13.22, `83fb65435`).

`tests/behavior.rs` covers what the brief asks beyond the printed traces: the three read states
`pageScaleOf` projects, a calibration capture that ends where the drag's last `move` left it (the
`up` does not move it, `packages/core/annotation/src/update/draw.ts:203`), `calibrate` refusing
lengths that are not finite and positive before anything reaches the engine, and a close while
changes run, where the all-pages change resolves one `scaleError` per page, the queued single-page
change rejects `instance-closed`, and the closed-state update is reported once.

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
list. `get_page_scale` returns an `Rc<PageScale>` that stays the same while the page's viewports,
read state, layout and default scale do. It is a `Result` because the TypeScript throws there: when
the `defaultScale` setting cannot be built in the page's user unit, and when the registry still
names the page after the document handle is gone. A read whose answer has no `viewports` leaves the
page not ready, as the TypeScript's `undefined` does.

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
| `MeasurementState { pages, pending, calibration, reports }`, `MeasurementAction`, `measurement_reducer`, `PageEntry` | `MeasurementState { pending, calibration, reports, load_errors, local_viewports }` and the named transitions in `model`; the viewports live in the page mirror |
| `units::measure_from_ratio_typed`, `decode_measure`, `range_error` | removed; `units` answers JSON |
| ffi `ScaleChangeOptions` | ffi `MeasurementScaleChangeOptions` |
| ffi `AnnotationRef` in readouts and reports | `annotation_json` strings |

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task
13.33. The native host answers `page.measure.listViewports` with `{ viewports }`.

## Defensive absent-service branches

`sync/viewports.ts:47` reads a page's session-only viewports, and `write/scale.ts:81` writes them,
when `doc.page(page).measure` is undefined. The current engine contract requires the service, and
the TypeScript fake supplies it on every page, so no vector reaches either branch: the vectors cover
the required service and its refusals. The port keeps both branches line for line
(`sync/viewports.rs`, `write/scale.rs::save`) without a case of its own, as the Task 12b ruling on
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

`write/scale.rs` keeps each page's queue tail beside the kernel's `SerialQueue`, because a download
waits on `queue.idle()` (`ctx.onSettle`) and the kernel queue has no `idle`.
