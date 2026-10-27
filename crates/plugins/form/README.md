# epdf-plugin-form

`packages/plugin/form` in Rust, re-ported in Task 13.18 onto the current kernel: the fields mirror,
the change queue, registered settings and the K/V/C/F pipeline over the actions plugin's realm. All
221 cases in six vector files replay identically (`cargo test -p epdf-plugin-form`). No member-order
allowances and no outcome exceptions are used for these files. The last case to pass,
`form-design.json` `placement-refused-by-the-engine`, needed the kernel's `to_plugin_error` to pass
a `PermissionDenied`'s `details` through whole, as `packages/core/main/src/errors.ts:131` does
(`1ff58faa4` and `526059c29`, pinned by `a_permission_refusal_keeps_its_details_whole` in
`crates/epdf-kernel`).

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
`reset-all`), a palette placement keeps the tool's look while the create is held, the three
placement paths (form-only success, the awaited refusal that is logged, the form-only early
return), lost object numbers, and the painter drawing a text field, a checkbox and a listbox, which
no vector does. `tests/sharing.rs` is M21. `tests/budget.rs` is H4. `tests/engine_cross_check.rs`
holds `engine.rs` to `epdf-engine` on native hosts: `field_after_value`, `field_after_reset`,
`form_field_after`, `default_entry_of_draft`, `fit_widget_to_family`, `reordered_list` and
`reorder_part`, refusals compared by message.

## Layout

The modules follow `packages/plugin/form/src` file for file. `controller.rs` builds the one `Core`
behind `FormCapability`, which is the public lens and the host lens. `connect.rs` wires the hub, the
annotation plugin and the actions plugin. `sync/` is the fields mirror and the prediction,
`services/` the events, fill rights, the scripting seam, object numbers, formatted
texts, pictures and looks, `read/` the reads, `write/` the verbs, `tools/` the palette and its
handler, `scripting/` the K/V/C/F pipeline. `engine.rs` is a pure copy of the engine-core rules the
plugin calls (`forms/predict.ts`, `forms/write.ts`, `forms/widgets.ts`, list positions, ref keys),
since `epdf-engine` links PDFium and this crate builds for wasm32 (Ruling13.15).

Engine records stay `serde_json::Value`. The snapshot (`model::Snapshot`) holds its `fields` and
`widgets` apart, one `Rc` per item. The kernel's mirror holds JSON and copies each value it writes,
so the `fields` mirror's value is a token, `{ "$fieldIndex": n }`, naming a `FieldIndex` that
`services/index_cache.rs` keeps: the 256 used last and the confirmed one. The store never shows the
token. The mirror's `to_json` (`MirrorSpec::to_json`, `640a17350`) prints the whole index when the
store is dumped, `snapshot`, `byKey`, `byName`, `byWidget`, `rowByWidget` and `pictureVersions` in
that order, as `signature.json` and `stamp-*.json` dump `form::a/mirror/fields`
(`the_fields_mirror_cell_dumps_the_whole_field_index` holds it to `signature.json`
`reads-over-the-snapshot`). The index's records (`byKey`, `byWidget` and so on) are kept
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

M21 is kept. A fold, a prediction or a fill replaces one field or widget row and shares every other
one, as the TypeScript's spreads do: an upsert copies pointers, and the mirror's token makes the
kernel's copy on write a few bytes. The whole snapshot is put together as one value only for
`getSnapshot`, the scripts and `onResynced`, once per snapshot. `tests/sharing.rs` counts those
copies with `model::snapshot_copies` and holds them at none across 50 folds and predictions on
5,000 fields and 15 live fills, folds and reads, each under a 120 s hang ceiling. It also pins
`listWidgets`' reference stability (`the_fill_projection_is_reference_stable`). The cost: a
scripted fill still puts the form together once per new view, for the realm.

`listWidgets` is memoized as `memoByKey` does, by page, on the view, the fill rights key and
`fillable`. Since Task 14.5 (M36) the other identity memos are too: `getValue` and `getWidgets` per
field key with `maxEntries: 1024`, `exportValues` on the view, `validate` on the view, the document
and the rights key, and `getSelectedField` on the view and the annotation plugin's selection list.
Each holds its inputs' `Rc`s and compares them with `Rc::ptr_eq`, the document's record included
(`ControllerContext::document_rc`). `list(None)` returns a new `Vec` of the shared fields where the
TypeScript returns the snapshot's array itself. The reads hand out the fields and rows the form holds
(`Rc<Value>`), as the TypeScript's do; `tests/sharing.rs` pins the identity and counts the memos'
computations (`reads_share_the_form_and_keep_their_answers`).

Five Rust-only console lines and errors have no TypeScript counterpart. A token the cache no longer
holds, which the confirmed value's pin and the view's recency should make impossible, is logged as
`[form] the fields mirror named a form it no longer keeps:` and shows the confirmed form, in a read
and in a store dump alike (`services/index_cache.rs`). A widget row or field the
painter can no longer decode is logged as `[form] the painter could not read a widget:` and not
drawn (`services/looks.rs`); the TypeScript has no decode step. A `surface` that throws inside
`fill_soon`'s failure path is logged as `[form] surfacing a fill failure threw:`
(`write/fill.rs`), where the TypeScript raises an unhandled rejection. A widget picture that does
not serialise rejects the lend's `renderAppearances` (`write/lending.rs`). A widget ref with no
`page` makes the activation verbs throw a `TypeError` (`write/activation.rs` `page_of_ref`), where
the TypeScript passes `undefined` on as the action source's page; `epdf-plugin-actions` types that
page as a `PageRef`, so it can't carry one. Valid refs always name a page.

The write queue is the kernel's `SerialQueue` (`ctx.serial_queue("writes")`), whose `run_with` and
`idle()` are `serial-queue.ts`'s `{ signal }` option and `idle()`.

The palette's form-only early return is kept as the TypeScript has it. With no annotation plugin a
field the view already shows ends the placement before `created` is awaited, so an engine refusal of
that create reaches no console line. The TypeScript raises an unhandled rejection there; Rust has no
such channel, and nothing catches the refusal on the plugin's behalf.
`a_form_only_placement_refusal_ends_before_it_is_awaited` pins it. Task 14 kept it: the TypeScript edge is the user's to fix.

The form pipeline's script print still skips the `print` policy row, as
`packages/plugin/actions/src/scripting/surface.ts:120` does. The form surfaces its results through
`ActionsCapability::surface_script_commit` (`services/scripting.rs`); Task 14 kept
it as the TypeScript has it.

Default QuickJS execution has no Rust counterpart. With JavaScript on and no sandbox factory the
actions plugin rejects every sandbox build (`NO_DEFAULT_SANDBOX`), so the form's scripts fail where
the TypeScript would load QuickJS. No vector runs that configuration. It is the separate loader gap
Task 14 left it open.

`flags.raw` is gone with the TypeScript. `DoNotSpellCheck` and `Sort` are not derived from anything
in its place.

## Wasm

`epdf-web` builds this crate for wasm32, so it depends on no PDFium: `cargo tree -p epdf-web --target
wasm32-unknown-unknown -e normal -i pdfium-sys` prints nothing. The painter path (`services/looks.rs`)
brings in `epdf-form-core` and `epdf-fonts`. Task 13.18 split the font programs out of the metrics
table in `epdf-fonts` (Ruling13.5(b)): they are decoded at build time into a table of their own,
which a build that never loads a program drops. A release wasm32 probe that only reads metrics went
from 461,992 to 34,554 bytes; one that also loads a program went from 467,202 to 358,996.

## The ffi

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task
13.33. Refs are written there in the engine's current spelling. The handle follows the current verbs: `set_text`, `set_checked`, `set_choice`,
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
