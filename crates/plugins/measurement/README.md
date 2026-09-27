# epdf-plugin-measurement

`packages/plugin/measurement` in Rust, on the v3 controller shape. All 57 cases replay identically
(`cargo test -p epdf-plugin-measurement`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/measurement.json` | scenario | 18 | page scale with and without the engine's `page.measure` service (owned, foreign and default viewports, refused reads, refresh epochs, `page.viewportsChanged`, `stream.desynced` and `document.versioned`), single- and multi-page scale writes with re-measurement, units, area units, precision and presets, the calibration flow, readouts, `createMeasurement`, authority, a throwing listener, inserted and deleted pages, and closing the document mid-change |
| `vectors/plugins/measurement-functions.json` | function | 39 | `measurementReducer`, `initialMeasurementState`, `DEFAULT_PRESETS`, `defaultMeasure`, `selectPageScale`, `withUnit`, `withAreaUnit` and `withPrecision` |

The scenarios run the real interaction and annotation adapters beside this one, because the plugin
requires both. The generator is `tooling/plugin-conformance/src/suites/measurement.ts`.

## The API

`measurement_plugin(MeasurementConfig)` builds the plugin, `MEASUREMENT` is its token and
`MeasurementCapability` its capability, which is also `MeasurementHostCapability`. The members are
the TypeScript's in snake case: `can_calibrate`, `can_measure`, `get_page_scale`, `ensure_loaded`,
`is_busy`, `list_last_reports`, `list_presets`, `list_units`, `list_area_units`, `get_readout`,
`measure_distance`, `measure_area`, `get_calibration_request`, `set_scale`, `calibrate`,
`set_unit`, `set_area_unit`, `set_precision`, `set_preset`, `clear_scale`, `create_measurement`,
`start_calibration`, `dismiss_calibration` and the four `on_*` hooks.

`set_scale` takes the measure as JSON. The TypeScript runs `assertWritableMeasure` over whatever
object it was handed, and a malformed member has to fail with its message (`Missing x formats`,
`Only rectilinear measures can be authored`), not a decode error. `units.rs` ports that preflight
over JSON, along with the rest of `engine-core`'s measurement helpers that
`epdf_annotation_core::measure` does not carry: the unit table, `measureFromRatio` and
`measureFromKnownLength` on the default format, `measurementPoint` and `viewportForPoint`.

Where the TypeScript throws synchronously, the Rust returns `Err`: `measure_distance` and
`measure_area` on a page not in the document, `ensure_loaded` once the plugin is disposed, and
`start_calibration` when the hub refuses the tool.

A throwing listener goes to `console.error('[measurement] listener failed:', error)`, which comes in
as `MeasurementConfig::host`. The generator logs it as a `console.error` callback
(`listener-throws`).

## Turn order

`enqueue` is `(previous ?? Promise.resolve()).catch(() => {}).then(action)`. That is one turn for
the `catch` reaction, one for the `then` reaction that calls the action, and one for adopting the
promise the action returns, and `write/scale.rs` spends each explicitly. Engine calls go through
`epdf_plugin_annotation::promise::await_engine` (three turns), and `change` awaits
`epdf_plugin_annotation::promise::all`, as `await Promise.all(...)` does.

A refresh without the measure service runs its whole `async` body at call time. Its `finally`
therefore drops the read from `reading` before `refresh` records it, and the settled read stays
recorded. The port keeps that order, so a later `ensure_loaded` joins the old read.

## Where the port differs in shape

The slice and the page scale hold the measure as the typed `PdfMeasurement`, so a key that type does
not name is dropped where the TypeScript keeps the caller's object. No vector feeds one.

An engine event listener cannot return an error in Rust. A synchronous throw from a refresh there
(the plugin disposed, or the engine not knowing the page) is dropped, where the TypeScript's would
escape the engine's `emit`. No vector throws there.

`createMeasurementCapability`, the build-and-connect helper the unit tests use, is not ported;
`create_measurement_controller` and `MeasurementCapability::connect` are the same two steps.

## Coverage

The suite reaches 1,126 of the package's 1,135 statements and 247 of its 259 branches (c8, as
`src/coverage.ts` counts them). What is left cannot be reached from the public surface:

- `controller.ts:56-62`, `createMeasurementCapability`, which only the package's unit tests call.
- `services/store.ts:36`, `sync/viewports.ts:107` and `write/calibration.ts:31`: a document plugin
  always has `ctx.doc`, so `?? false` and the two `if (!doc) return` never run.
- `services/store.ts:49` and `sync/viewports.ts:109`, `123` and `126`, the `?? []` after
  `ctx.document()?.pages`. A document plugin always has its document.
- `sync/viewports.ts:28`, `publish` on a disposed plugin. Both callers check first.
- `sync/viewports.ts:71` and `write/scale.ts:79`, the `?? []` for a page with no slice entry.
  `refresh` dispatches the entry before it reads, and `save` runs after `ensureLoaded`.
- `write/calibration.ts:44-46`, the `catch` around `toPage`. The annotation plugin only captures a
  draft on a page whose crop it can read from the same registry.
- `write/scale.ts:164`, `(error as Error).message ?? error`. The calibration helpers only throw
  `RangeError`s, which have a message.

## TypeScript behavior the vectors pin

`setScale('all')` re-measures nothing (`set-scale-on-several-pages`). Each page calls the
annotation plugin's `remeasurePage` at once, each call starts a rehydration, and every page reports
`Annotation hydration is incomplete`. This looks like a bug in how the two plugins meet, not in
either alone.

The draft listener checks `doc.annotate.modify` again, but the annotation plugin already refuses a
capture tool without it, so that branch is dead (`calibration-needs-modify` in `annotation-api.json`
pins the annotation side).

`withUnit` and `withAreaUnit` test the base unit with `basis in METRES`, which also accepts
`Object.prototype` keys such as `constructor`. The port looks the unit up in the table.

Closing the document while a viewport read is in flight dispatches the error scale after the
instance closed, because the guarded call rejects before the store's cleanup marks it disposed. The
kernel drops the dispatch and reports it once (`close-mid-flight`).

## Mutation testing

`scripts/mutants.sh -p epdf-plugin-measurement --test-package epdf-plugin-measurement --file
<file>`, file by file over every module but `harness.rs`, catches 245 of 248 viable mutants (245
caught, 0 timeouts; 124 unviable). The run left 27 survivors. 23 of them are killed by new cases
and one by a unit test, each checked by applying the mutant by hand:

- `set-scale-preflight` sends malformed and edge-case measures through a single-page `setScale`,
  which rejects with the preflight's message. It kills 18 in `units.rs`. A zero conversion kills
  `valid_number_format` as `true`, `num_member` as `Some(1.0)` and `||` to `&&` at `:187`. A
  precision of 1.5 kills `is_integer` as `true` and its `&&` to `||`; `1e10` kills `||` to `&&` at
  `:193`. A format whose unit is the number 5 fails as a number format, and both `&&` to `||` at
  `:236` and `:237` let it through to fail as text instead. A non-string `thousands` kills `text_ok`
  as `true`. A measure without a distance list kills `&&` to `||` at `:228`. An origin at `1e39`
  kills `assert_pdf_float` as `Ok(())` and its guard as `true`; a finite origin kills the guard as
  `false`. A decimal format with no precision kills `==` to `!=` at `:192`, whose default of 16 is
  not a power of ten. A `labelPosition`, a `fixed` flag and a CYX of 1 kill the deleted `!` at
  `:247` and `:252` and `<=` to `>` at `:269`. A calibration over 7 points writes `1 in =
  10.285714 m`, which kills `to_precision` as `1.0`: every earlier calibration came out at exactly
  1.
- A `plan` preset with a paper length of 2 in `imperial-default-and-custom-presets` kills `/` to
  `*` for `real / paper` in `measure_from_ratio` (`units.rs:148`). Every other preset has a paper
  length of 1.
- `select-beside-the-centre`, a viewport that spans the crop centre's height but not its x, kills
  both `&&` to `||` in `viewport_for_point` (`units.rs:285`). `select-on-a-raised-crop` kills `+`
  to `-` for the centre's y in `select_page_scale` (`scale.rs:62`): every earlier crop started at 0.
- `with-unit-keeps-precision` kills the `kept_precision` guard as `true` (`scale.rs:96`).
- `tests/api.rs` kills `ScaleChangeReport::page` as `Default::default()` (`contract.rs:213`). The
  accessor is Rust only, and nothing in the crate calls it.

The 3 survivors:

- `src/contract.rs:28`, the `s == "all"` guard in `PageTarget`'s decoder as `true`. It turns any
  string into `All`. The TypeScript's `PageTarget` admits no string but `'all'`, and the Rust
  decoder rejects any other before the plugin sees it, so no vector can carry one.
- `src/model.rs:63`, `initial_measurement_state` as `Default::default()`. The body is already
  `MeasurementState::default()`.
- `src/read/scale.rs:77`, the `!raw.is_null()` guard in `get_readout` as `true`. A null DTO has no
  subtype, so `readout_of_dto` answers `not-dimension`, the same as the fallback arm.
