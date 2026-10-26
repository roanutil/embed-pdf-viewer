# epdf-plugin-form

`packages/plugin/form` in Rust, re-ported in Task 13.18 onto the current kernel: the fields mirror,
the change queue, registered settings and the K/V/C/F pipeline over the actions plugin's realm. 220
of the 221 cases in six vector files replay identically (`cargo test -p epdf-plugin-form`). The
221st, `form-design.json` `placement-refused-by-the-engine`, waits on a kernel repair described
below. No member-order allowances and no outcome exceptions are used for these files.

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/form-core.json` | function | 99 | `FieldIndex` and its folds, `predictFormOp`, `undoLookOf`, the fill projection, the hit test, the palette table, ISO field selection and the submit dataset |
| `vectors/plugins/form-scripting.json` | scenario | 46 | the K/V/C/F controller over a detached realm, its budgets and fault ladder, and the plugin's fills, resets and activation through scripts |
| `vectors/plugins/form-fill.json` | scenario | 31 | the mirror's load and folds, values, resets, batches, interchange, authority, events and close |
| `vectors/plugins/form-design.json` | scenario | 29 | the palette and its place handler, the design verbs, lending to the annotation plugin, the hit test |
| `vectors/plugins/form-actions.json` | scenario | 13 | the reset-form executor, the commit sink, the submit resolver, activation and the widget event feed |
| `vectors/plugins/form-core-lifecycle.json` | scenario | 3 | the mirror's status, a confirmed and a refused fill, a field the session may not fill |

`tests/conformance.rs` also pins the six files at 221 cases. `tests/behavior.rs` asserts what the
replay holds only implicitly: a whole reset reports only the fields it moved (refs 1, 3 and 4 of
`reset-all`), a palette placement keeps the tool's look, the three placement paths (form-only
success, the awaited refusal that is logged, the form-only early return), and lost object numbers.
`tests/budget.rs` is H4. `tests/engine_cross_check.rs` holds the value rules to `epdf-engine`'s on
native hosts.

## Layout

The modules follow `packages/plugin/form/src` file for file. `controller.rs` builds the one `Core`
behind `FormCapability`, which is the public lens and the host lens. `connect.rs` wires the hub, the
annotation plugin and the actions plugin. `sync/` is the fields mirror and the prediction,
`services/` the events, fill rights, the scripting seam, the write queue, object numbers, formatted
texts, pictures and looks, `read/` the reads, `write/` the verbs, `tools/` the palette and its
handler, `scripting/` the K/V/C/F pipeline. `engine.rs` is a pure copy of the engine-core rules the
plugin calls (`forms/predict.ts`, `forms/write.ts`, `forms/widgets.ts`, list positions, ref keys),
since `epdf-engine` links PDFium and this crate builds for wasm32 (Ruling13.15).

Engine records stay `serde_json::Value`. The kernel's mirror holds JSON, so the `fields` mirror's
value is the snapshot and the picture versions, and `services/index_cache.rs` keeps the decoded
`FieldIndex` beside each recent value. The index's records (`byKey`, `byWidget` and so on) are kept
as JavaScript objects order them, integer-like keys first, so they print and iterate as the
TypeScript's do.

## What differs from the TypeScript

H4 stays. The transaction's time budget runs on `rt::monotonic_now`, where the TypeScript reads
`Date.now()`. A real host runs a whole K/V/C/F chain inside one drain, where the virtual clock stands
still, so a budget on it never runs out. `tests/budget.rs` burns 30 ms of wall time per run and gets
two runs out of the default 50 ms.

A5 stays. `importValues` and `exportValues` order names as a plain object does, integer-like names
first (`form-fill.json` `values-keyed-by-integer-like-names` replays it). A6 is gone with the
code it named: placement names fields with `nextFieldName` over a `HashSet`.

M21 is in part a kernel cost now. The plugin shares a snapshot between reads through
`IndexCache`, but the kernel's mirror writes every value into its store cell by copy
(`crates/epdf-kernel/src/mirror.rs`, `write`), so a fold costs a copy of the form there.

The kernel's `SerialQueue` has no `signal` option and no `idle()`. `services/queue.rs` adds both over
it as `serial-queue.ts` defines them: an operation whose signal fired while it waited rejects
`operation-cancelled` without starting, and `idle()` settles a turn after the last operation queued.
The kernel's own queue keeps the order and timing.

The palette's form-only early return is kept as the TypeScript has it. With no annotation plugin a
field the view already shows ends the placement before `created` is awaited, so an engine refusal of
that create reaches no console line. The TypeScript raises an unhandled rejection there; Rust has no
such channel, and nothing catches the refusal on the plugin's behalf.
`a_form_only_placement_refusal_ends_before_it_is_awaited` pins it. This is the Task 14 decision.

The form pipeline's script print still skips the `print` policy row, as
`packages/plugin/actions/src/scripting/surface.ts:120` does. The form surfaces its results through
`ActionsCapability::surface_script_commit` (`services/scripting.rs`); the Task 14 decision covers
it.

Default QuickJS execution has no Rust counterpart. With JavaScript on and no sandbox factory the
actions plugin rejects every sandbox build (`NO_DEFAULT_SANDBOX`), so the form's scripts fail where
the TypeScript would load QuickJS. No vector runs that configuration. It is the separate loader gap
the Task 14 decision covers.

`flags.raw` is gone with the TypeScript. `DoNotSpellCheck` and `Sort` are not derived from anything
in its place.

## The kernel repair this crate waits on

`placement-refused-by-the-engine` step 2 expects the refused create to reject with `permission:
"doc.forms.modify"` and `details: { required: "doc.forms.modify" }`. Rust gives `permission: null`
and `details: {}`. The engine's refusal is a `PermissionDenied` whose members are `code` and
`details` only, as the fake makes it (`tooling/plugin-conformance/src/fake-engine.ts:262`
`makeEngineError`). The TypeScript maps it through its `EngineError` branch: `details` as the error
carries them, and the permission from `details.required`
(`packages/core/main/src/errors.ts:121-135`, `requiredPermissionOf` at `:106`). The kernel's
`PermissionDenied` branch builds `details` from top-level `required` and `context` members instead
(`crates/epdf-kernel/src/error.rs:310-326`), which this error doesn't have. Taking `details` from the
error when it has them makes all 221 cases replay; the controller rules on it.

## Wasm

`epdf-web` builds this crate for wasm32, so it depends on no PDFium: `cargo tree -p epdf-web --target
wasm32-unknown-unknown -e normal -i pdfium-sys` prints nothing. The painter path (`services/looks.rs`)
brings in `epdf-form-core` and `epdf-fonts`. Task 13.18 split the font programs out of the metrics
table in `epdf-fonts` (Ruling13.5(b)): they are decoded at build time into a table of their own,
which a build that never loads a program drops. A release wasm32 probe that only reads metrics went
from 461,992 to 34,554 bytes; one that also loads a program went from 467,202 to 358,996.

## The ffi

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand and are not compiled: `epdf-ffi` does not build
at this commit, and Task 13.33 brings it, `epdf-viewer-host` and Swift up together. Refs are written
there in the engine's current spelling, since `epdf-ffi`'s `to_json` still writes the retired
`fieldObjectNumber`. The handle follows the current verbs: `set_text`, `set_checked`, `set_choice`,
`reset_all`, `attach_widget`, `detach_widget`, `export_data` and `import_data` are gone, and
`add_widget`, `remove_widget`, `export` and `import` (bundles as JSON) replace them.

## Migrating from the pre-13.18 API

| Old | New |
|---|---|
| `form_plugin(FormConfig { validation, host })` | `form_plugin(Option<Value>)`, the registered settings over `form_defaults()`; console output goes through `console::set_sink` |
| `Model`, `Msg`, `update`, `FormState`, `form_reducer` | `model::FieldIndex` and its functions; the session state is `{}` |
| `get_field`, `list_fields`, `list_fill_items`, `get_fill_item` | `get`, `list`, `list_widgets`; no fill item lookup by number |
| `set_text`, `set_checked`, `set_choice`, `set_value_raw` | `set_value(ref, value, signal)` with `{ value }`, `{ checked }` or `{ selectedValues }` |
| `reset(ref)`, `reset_all(options)` | `reset(Option<Vec<Value>>, signal)` |
| `create_field`, `update_field`, `delete_field`, `detach_widget`, `attach_widget` | `create(draft)`, `update`, `delete`, `remove_widget`, `add_widget(ref, placement)` |
| `export_data`, `import_data` | `export(selection)`, `import(bundle, options)` |
| `FormScriptingController::new` with `doc`; `commit(ref, value)` writing to the engine | `FormScriptingController::new` with `document`, `transaction`, `budget`; `commit(form, ref, value)` returns a `FormScriptResult` and writes nothing |
| `services::scripting::commit_surface` (stamp) | `ActionsCapability::surface_script_commit` with the result's UI effects, diagnostics and error |
| `get_widget_at(PageRef, x, y)` (signature) | `get_widget_at(&Value, x, y)`, the page as a ref or an index |
| `refresh()` (signature) | `refresh(signal)` |

Known call sites: `crates/plugins/signature` (`controller.rs`, `lib.rs`, `harness.rs`,
`tools/armed_mark.rs`, `services/siblings.rs`, `read/signatures.rs`, `write/fill.rs`,
`tests/ffi.rs`), `crates/plugins/stamp` (`write/placement.rs`, `tests/conformance.rs`),
`crates/epdf-web` (`src/adapters.rs`) and `crates/epdf-viewer-host` (`session.rs`, `lib.rs`,
`api.rs`, `tests/viewer_host.rs`). None of them compiles at this commit for reasons that predate
this task.
