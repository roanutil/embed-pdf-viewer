# epdf-plugin-annotation

`packages/plugin/annotation` in Rust, re-ported in Task 13.17 onto the current kernel (state cells,
the records mirror, the change queue and registered settings). The pure model is
`epdf-annotation-core`. All 992 cases in eleven vector files replay identically
(`cargo test -p epdf-plugin-annotation --features harness`). No member-order allowances and no
outcome exceptions are left for these files.

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/annotation-repository.json` | function | 690 | `record.ts` and `appearance.ts`: `fromDTO`, `withValues`, `annotationPatchBetween` (95 field-changing pairs), the 12 missing-icon negatives, `drawnAfter`, `sourceOfConfirmed` |
| `vectors/plugins/annotation-pure.json` | function | 153 | the tool registry, `buildTextItems`, the rich-text helpers, `previewBucket`, `buildCommentThreads`, `sniffBinaryMetadata`, `parseAnnotationDraft` |
| `vectors/plugins/annotation-tools.json` | scenario | 34 | tools in the hub and the four handlers, click placement, ghosts, stamps, the file-picker port, behaviors |
| `vectors/plugins/annotation-reconciliation.json` | scenario | 32 | the records mirror against the store: creates, refusals, upserts and deletions mid-gesture, the H3 cases |
| `vectors/plugins/annotation-api.json` | scenario | 19 | the public reads and writes, events, rich-text editing, attachments, stamps, measurement |
| `vectors/plugins/annotation-hydration.json` | scenario | 18 | the mirror's load, page reloads, remote events, the cursor watermark |
| `vectors/plugins/annotation-comments.json` | scenario | 16 | comment threads and their verbs, links, the script commit door |
| `vectors/plugins/annotation-crud.json` | scenario | 13 | create, update, delete, settings, selection, text editing |
| `vectors/plugins/annotation-markup.json` | scenario | 9 | markup, caret and replace-text from the selection plugin, `createFromSelection` |
| `vectors/plugins/annotation-grouping.json` | scenario | 5 | `group`, `ungroup` and the grouped replace-text create |
| `vectors/plugins/annotation-hover.json` | scenario | 3 | hover state, the enter/exit feed onto the actions pump |

`tests/conformance.rs` also pins the repository suite's 95 `annotationPatchBetween` cases and its
12 `text-no-icon` cases, so a regenerated vector that drops one fails. `tests/behavior.rs` covers
what no vector reaches: H3, N6, M16 and M19 (below), lost object numbers, the cursorless-handle
read of `getCursorAt`, and re-entry from an `onWriteFailed` listener, ported from
`tooling/plugin-conformance/test/annotation-live.test.ts` ("the explicit internal
failure-callback reentry edge").

## Layout

The modules follow `packages/plugin/annotation/src` file for file. `controller.rs` builds the one
`Core` behind `AnnotationCapability` (the public and host lenses are one type, resolved with
`ANNOTATION`). `model.rs` is the plugin's state cell, `sync/` the records mirror and the confirmed
change feed, `services/` the store, staging, lending, authority and events, `read/` the view and
its projections, `write/` the verbs, `tools/` the registry, handlers and ghosts, `comments/` the
threads. Engine records stay `serde_json::Value` throughout, as the TypeScript passes the engine's
objects.

The mirror's value is `{ "byKey": { key: { "dto", "pictureVersion", "$v" } }, "order": [...] }`.
`$v` is a fresh token per put: the view caches by record identity, as the TypeScript's `WeakMap`s
do, and JSON has none. Lent records are `Rc<Value>` rows for the same reason, so a lender that
hands back the same rows is not asked about them again.

## Turn order

The trace records how engine answers and model updates interleave, so each `await` and `.then` is
mirrored (`src/promise.rs`). An `async` function that ends in `await` costs a turn after the value
it awaits; `text_end` and `toggle_format` spend it as `text.end` and `toggleFormat` do.

## Kept divergences

H3 (a drag before the create is confirmed) needs no guard of its own any more. Records keep their
object-number key from the first frame, so nothing is renamed, and a record that leaves the view
leaves the session in the same notification (`services/store.rs`, the forget watch, as
`packages/plugin/annotation/src/services/store.ts` does). The `reconcile/h3-*` cases replay it, and
`h3_a_record_deleted_mid_drag_leaves_the_session_before_the_next_sample` pins that the forget runs
before any later pointer sample.

N6 stays: a record whose ref is of a kind no key is defined for is refused (`sync/records.rs`
`dto_key`), where the TypeScript keys it `"undefined"` and collapses every such record into one
(`n6_a_record_whose_ref_has_no_key_is_refused_and_the_others_stay`).

M15 is the actions plugin's URI sanitizer. This crate opens no URI, so it has nothing to keep.

M16 is superseded. Hydration is the kernel's records mirror now, whose `refresh` clears `running`
on every path (`crates/epdf-kernel/src/mirror.rs` `refresh`, as `packages/core/main/src/mirror.ts`
`refresh`). The plugin's own hydration and its stuck-promise path are gone.
`m16_a_read_grant_after_a_forbidden_load_is_seen_on_refresh` pins the sequence: forbidden at
open, a refresh still forbidden, the grant, and the next refresh loads.

M19 stays: comment threads are built with hashed lookups, once per change of the records, the
layout or the user. `m19_the_threads_index_is_built_once_per_change` checks the memo by `Rc`
identity, and `m19_threading_ten_thousand_annotations_stays_linear` builds 10,000 annotations
under a 10 s hang ceiling (about 0.4 s today).

Lost object numbers leave the session, as `services/object-numbers.ts:14-19` has them: the first
time the session takes numbers it subscribes to `DocumentHandle::on_lost_object_numbers`
(`a_lost_object_number_leaves_the_session`). The conformance fake never loses one, as the
TypeScript fake never does.

Two Rust-only differences, both on paths no vector reaches, are registered in
`docs/research/rust-plugins-branch-review.md`, Status:

- `fonts()` is always none. The TypeScript reads the local engine's registered fonts
  (`services/index.ts:71`); the Rust plugin context has no engine font list, so registered faces
  are not mapped on a local engine. The cloud path is the same in both (an empty list). The
  native host registers no fonts, so its list is empty too; a host font list needs an engine
  member the kernel's `Engine` lacks (Task 13.33 report). No vector registers a font.
- An attachment or icon placement that `iconPlacement` refuses (an attachment with no name) is
  logged as `[annotation] icon placement failed:` and the click counts as handled
  (`write/icons.rs`). The TypeScript throws inside the picker's `.then`, an unhandled rejection
  with no console line. Rust has no unhandled rejection to raise there.

## Migrating from the pre-13.17 API

For form (13.18), link, measurement, redaction, signature and stamp, and viewer-host and epdf-web
(13.33).

| Old | New |
|---|---|
| `annotation_plugin(AnnotationConfig)`, `AnnotationHost { console_error, random_uuid, fonts }` | `annotation_plugin(Option<Value>)`, the registered settings (`annotation_defaults()`). `console_error` is `console::set_sink`; `/NM` seeds and fonts are gone |
| typed `Annotation`, `AnnotationPatch`, `CreateAnnotationInput`, `geometry_from_input`, `AnnotationGeometry` | the engine's records as `Value`: `create(page, fields, resources, CreateOptions)`, `update(ref, patch, resources, signal)` |
| `list_raw`, `get_raw`, `create_raw`, `update_raw` | removed: `list` and `get` return the engine's records |
| `create_many`, `update_many`, `delete_many`, `set_rotation`, `rotate_by`, `set_contents` | removed with the TypeScript's; `selection_update`, `selection_rotate_by(degrees)`, `delete` |
| `get_status`, `get_hydration`, `ensure_hydrated`, `refresh`, `reload_page`, `deliver_remote_event` | `get_status() -> ResourceStatus`, `refresh(signal) -> Promise<()>`, `when_synced()`; hydration is the records mirror, and remote events arrive through the kernel |
| `on_created`, `on_updated`, `on_deleted`, `on_resynced`, `on_selection_changed`, `on_draft_changed`, `on_editing_changed`, `on_draft_captured`, `comments_on_thread_changed` | the same names, each taking `Fn(&Value) -> Result<(), Error>` and returning `Unsubscribe`; the payload is the TypeScript's event object as JSON. New: `on_reordered`, `on_hover_changed`, `on_write_failed`, `tools_on_defaults_changed` |
| `AnnotationChangedEvent` | the `on_created`/`on_updated` payload, `{ annotation, origin }` |
| `AnnotationFilter` (root) | `read::annotations::AnnotationFilter { pages, subtype }` |
| `comments()` (`CommentsLens`), `links()` (`LinksLens`) | `comments_*` and `links_*` methods |
| `hit_test_at` | `annotation_at(page, point)` |
| `register_tool`, `AnnotationToolDef` | `tools_register(definition: Value)`; a tool definition is JSON |
| `arm_stamp`, `place_stamp`, `StampToolInput`, `StampPlacement` | `stamps_arm(StampInput, signal)`, `stamps_place(StampInput, placement: &Value, signal)` |
| `create_attachment`, `read_attachment` | `create` with `resources`, `download_resource(ref, role, signal)` |
| `QuarterTurn`, `TextFormat` | `f64` degrees, `&str` format |
| `widget_appearance_from_props`, `color_to_css`, `css_to_color` | `epdf_annotation_core::record::widget_appearance_of`; colors are CSS strings in the engine's records, so the two converters have no counterpart |
| `HoverFlags`, `LinkNavItem` | `list_link_items(page)` as `Value` |
| `RecalibrationReport`, `SkipReason`, `PageMeasurementViewport` | `remeasure_page(page, scale) -> Promise<Value>`, `set_page_viewports(page, viewports, fallback)` |
| `annotation_key`, `repository::seam::annotation_key_of` | `js::key_of` (`epdf_annotation_core::record::annotation_key`) |
| `value::{get, str_of, truthy, number}` | `epdf_annotation_core::json::{get, get_str, truthy, number}` |
| `value::{pdf_rect_of, pdf_point_of}` | none: records are page space; convert with the kernel's page geometry (`PageSpace::page_rect_to_pdf`) |
| `HitView`, `view_env` (root) | `read::chrome::{HitView, view_env}` |
| `wire_markup` (root) | internal: the plugin wires the selection plugin itself on connect |
| `from_dto`, `to_patch` (root) | `epdf_annotation_core::record::from_dto`, `annotation_patch_between` |
| `StampPreview`, `StampPreviewProvider`, `ArmedStampPreview` | `write::stamps::StampPreview`, `StampPreviewProvider` (root), the preview as JSON `{ bytes, mimeType? }` |
| `ANNOTATION`, `AnnotationCapability`, `ANNOTATION_EDIT_PRIORITY`, `Behavior`, `ARMED_STAMP_TOOL_ID`, `harness::Annotation` | unchanged |
| `ffi::AnnotationConfig` | removed: the settings are registered with the kernel |
| `host_contract::serialize_error` | the kernel's `Error::to_canonical` |
| `promise::{await_engine, spawn, then, all}`, `binary::{resolve_binary_source, sniff_binary_metadata}` | unchanged |
| new | `lend(AnnotationLender)`, `register_behavior`, `commit_script_effects`, `import`, `export`, `reorder`, `is_pending`, `is_editable`, `get_hovered`, `selection_update_link` |
| ffi typed `AnnotationRecord` (`geometry`, `props`, `flags`), `AnnotationPatch`, `CreateAnnotationInput`, `AnnotationHydration`, `AnnotationQuarterTurn`, `AnnotationSnapSettings` | the record carries `rect`, the text members and authority typed and the rest as `fields_json`; patches, create fields and tool defaults are JSON strings. `src/ffi/` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task 13.33 |

Known call sites, by crate:

| Crate | Files | Names |
|---|---|---|
| form | `src/controller.rs`, `lib.rs`, `harness.rs`, `tools/handlers.rs` | `AnnotationCapability`, `AnnotationToolDef`, `Behavior`, `widget_appearance_from_props` |
| link | `src/lib.rs`, `controller.rs`, `contract.rs`, `ffi.rs`, `source.rs` | `AnnotationCapability`, `Behavior`, `ANNOTATION_EDIT_PRIORITY`, `HoverFlags`, `LinkNavItem`, `promise::{all, await_engine, spawn, then}`, `value::{get, pdf_rect_of, str_of, truthy}` |
| measurement | `src/lib.rs`, `ffi.rs`, `contract.rs`, `write/create.rs`, `write/scale.rs`, `read/scale.rs`, `sync/viewports.rs`, `services/store.rs` | `AnnotationCapability`, `CreateAnnotationInput`, `PageMeasurementViewport`, `RecalibrationReport`, `SkipReason`, `host_contract::serialize_error`, `promise::{all, await_engine}`, `value::{get, pdf_point_of, str_of, truthy}`, `on_draft_captured` |
| redaction | `src/lib.rs`, `harness.rs`, `controller.rs`, `write/apply.rs`, `write/marks.rs`, `read/pending.rs`, `sync/mod.rs` | `AnnotationCapability`, `annotation_key`, `AnnotationChangedEvent`, `AnnotationFilter`, `CreateAnnotationInput`, `repository::seam::annotation_key_of`, `promise::{await_engine, spawn}`, `on_created`, `on_updated`, `on_deleted` |
| signature | `src/harness.rs`, `lib.rs`, `contract.rs`, `write/place.rs`, `tools/armed_mark.rs`, `services/marks.rs`, `services/siblings.rs` | `AnnotationCapability`, `StampPlacement`, `StampToolInput`, `ANNOTATION_EDIT_PRIORITY`, `ARMED_STAMP_TOOL_ID`, `binary::resolve_binary_source` |
| stamp | `src/lib.rs`, `harness.rs`, `value.rs`, `ffi.rs`, `write/libraries.rs`, `write/placement.rs`, `write/marks.rs`, `write/assets.rs`, `read/catalog.rs`, `services/ghosts.rs` | `StampPlacement`, `StampToolInput`, `StampPreview`, `StampPreviewProvider`, `ArmedStampPreview`, `binary::{resolve_binary_source, sniff_binary_metadata}`, `value::{get, number, str_of, truthy}` |
| epdf-viewer-host, epdf-web | `src/session.rs`, `lib.rs`, `api.rs`; `src/adapters.rs` | `annotation_plugin`, `ffi::AnnotationHandle`, `ffi::AnnotationConfig`, `harness::Annotation` |

None of those crates compiles at this commit for their own reasons (the annotation-core and
geometry re-ports).

## Coverage

Stale since Task 13.17: the counts and lines below are the pre-13.17 port's, over files that no
longer exist. Task 14 regenerates them.

The generators reach 9,238 of the package's 9,254 statements and 2,159 of its 2,258 branches
(measured with `src/coverage.ts`). The 16 statements left cannot be reached from the public
surface:

- `services/write-sink.ts` 64 to 68, `awaitCreate` with no optimistic id. `create` validates its
  input before committing, and every message it and `createFromSelection` commit stages an entry
  in the core.
- `write/crud.ts` 290 to 292, a `createGroup` effect whose members are missing or on different
  pages. The core's replace-text group only ever names members of one page.
- `write/script-effects.ts` 64 to 71, `commitScriptEffects` with no `ctx.doc`. The plugin is
  document-scoped, so its controller always has a handle.

The 99 unreached branches are mostly the `??` and optional-chaining arms of the same guards;
`coverage.json` lists each by line and column.

## Mutation testing

Stale since Task 13.17: the files and lines below name the pre-13.17 code. Task 14 reruns it.

`scripts/mutants.sh -p epdf-plugin-annotation --test-package epdf-plugin-annotation --file <file>`,
file by file over every module but `harness/`, catches 1,412 of 1,449 viable mutants (1,411 caught,
1 timeout; 534 unviable). The run itself, at `afb842b48`, caught 1,356 and missed 92, across the 59
files that have mutants. Cases added afterwards kill 55 of those 92, and the counts above include
them. Each kill was confirmed by applying the mutant by hand and watching `cargo test` fail, not by
a second run.

Most of the 55 needed a listener or a read the vectors never made. The `annotation-lens` test plugin
now exposes `onThreadChanged` on both sides, `direct-delivery` listens to the change events (all
six `originOf` mutants), and four tool cases listen to the hub's `onGestureStarted`, which is where
a handler's `id()` shows. Six scenario cases are new: `text-range-needs-an-active-edit`,
`dimension-drafts-continue-without-viewports` and `set-contents-settles-with-the-engine` in
`annotation-api`, `file-picker-remover-clears-only-its-own` and
`hover-follows-the-subtype-the-draft-began-with` in `annotation-tools`,
and `create-from-selection-anchors-at-the-end-glyph` in `annotation-markup`. `annotation-repository`
gains two `fromDTO` fixtures with NaN and infinite rotations. The rest are extra steps in existing
cases and extra function cases. None needed a unit test.

The timeout is `+=` to `*=` in `parse_jpeg` (`binary.rs:127`), a loop that never ends. The 37
survivors left are equivalent or unreachable:

| File | Line | Mutant | Why no case kills it |
|---|---|---|---|
| `binary.rs` | 105 | `offset + 4 < len` to `<=` in `parse_jpeg` | Equivalent. With four bytes left every branch of the extra pass returns `None` or leaves the loop: an SOF needs nine. |
| `binary.rs` | 129 | `\|` to `^` in the segment length | Equivalent. The high byte is shifted clear of the low one. |
| `comments/build.rs` | 131 | `x < y` to `<=` in `chronological` | Equivalent. It sits under `x != y`. |
| `comments/build.rs` | 142 | `d > 0.0` to `==`, `<` and `>=` (3 mutants) | Equivalent. `sort_by` only asks whether the answer is `Less`, and every mutant still answers `Less` exactly when `d < 0`. |
| `controller.rs` | 152 | `+` to `-` and `*` in `random_uuid` (2) | Unreachable by a vector. It is the fallback for a host that passes no `random_uuid`; the harness always passes one, so the `/NM` seeds match the TypeScript's. |
| `controller.rs` | 161 | `fonts` to `vec![]` | Unreachable by a vector. The harness passes `fonts: None`, as neither fake engine has a fonts service on `ctx.engine`. The face mapping over a registered list is pinned by the `annotation-pure` function cases. |
| `controller.rs` | 172 | `defer_fn` to `()` | Unobservable. The deferred cleanups clear the viewport cache and dispose the event hooks at document close, after which nothing can emit or read them. |
| `host_contract.rs` | 92 | delete `!` on the `details` filter in `serialize_error` | Unreachable. The `EngineError` branch never runs: the kernel hands every engine refusal to this plugin as a `PluginError`, and the other errors are plain. A refused update and a failed hydration both arrive that way. |
| `read/annotations.rs` | 109 | the `a.ref_.is_some()` guard in `loaded_or_throw` to `true` | Unreachable. An entry without a ref is keyed `tmp:N` (the core's optimistic ids) or `""` (a DTO with no ref), and `annotation_key` builds neither. |
| `read/annotations.rs` | 126 | `&&` to `\|\|` in `selected_committed` | Equivalent. An entry has a ref without data only between `Msg::Created` and the `sync` that follows it in the same synchronous block (`write/crud.rs` 423, 485, 499), and optimistic entries carry neither. |
| `read/render.rs` | 79 | the `subtype == "link"` guard in `list_link_items` to `true` | Equivalent. The loop already skips entries whose subtype is not `link`, and `ingest` takes an entry's subtype and data from the same DTO; a link-tool draft has no data. |
| `repository/kinds/quads.rs` | 85, 88, 91, 94 | `<` to `<=` and `>` to `>=` in `pdf_bounds_of_quads` (4) | Equivalent. On a tie the running bound is replaced by an equal value (a `-0` prints as `0`). |
| `repository/seam.rs` | 127 | the `truthy_num(r)` rotation guard in `box_geom_from_dto` to `true` | Equivalent. It only lets a 0 or `NaN` rotation through, and the two `truthy_num(rot)` tests below treat those as no rotation either way. |
| `repository/seam.rs` | 305 | `content_rect_of` to `Default::default()` | Unobservable. Nothing in the workspace calls it. |
| `services/authority.rs` | 16 | `owner_of` to `Default::default()` | Unreachable by a vector. Both fake engines answer `allowsAnnotationMutation` by action alone and ignore the target. |
| `services/file_picker.rs` | 69 | delete the `(None, None)` arm in `prompt_at` | Unreachable. The plugin is document-scoped, so the document is bound whenever a handler runs a click. |
| `tools/definitions.rs` | 84 | the `mode == "footprint"` guard in `GhostPolicy`'s decoder to `true` | Unreachable by a vector. It only changes what a non-`false` value without that mode decodes to, which is outside the TypeScript's `false \| { mode: 'footprint' }` type. |
| `tools/definitions.rs` | 331, 339 | delete `subtype` from the `draw` and `markup` rows (2) | Equivalent. Those rows set the subtype to the tool's id, which is what resolution falls back to. |
| `tools/definitions.rs` | 332 | delete `cursor` from the `draw` row | Equivalent. Its value is `crosshair`, the resolved default. |
| `tools/definitions.rs` | 522 | `\|\|` to `&&` in `merge_defaults` | Equivalent. When only one side has line endings, merging them over the empty default gives that side's endings, which the spread already holds. |
| `tools/definitions.rs` | 633 | `!=` to `==` on `ToolRegistry::delete`'s return | Unobservable. Its one caller, the `registerTool` unregister (`tools/registry.rs:93`), ignores it. |
| `tools/handlers.rs` | 85 | the ghost handler's `id()` to `""` and `"xyzzy"` (2) | Unobservable. The hub reads an id only for the handler that owns a gesture, and the ghost handler's `on_down` always declines. |
| `value.rs` | 61 | `<` to `==` and `<=` in `number` (2) | Equivalent under the harness. They only change whether an integral value is spelled as an integer or a float, and the vectors compare numbers as `f64`. The `>` mutant, which saturates past `i64`, is killed. |
| `write/links.rs` | 105 | `==` to `!=` in `schedule_link_sync`'s cleanup | Equivalent. The finished chain stays in the map, but a chain never rejects (`reconcile_link_children` reports every engine failure to `console_error`), so the next sync chains onto a fulfilled promise either way. |
| `write/measurement.rs` | 47 | `PageViewports::clear` to `()` | Unobservable. Its only caller is the close cleanup (`controller.rs:114`). |
| `write/selection.rs` | 195 | `-1.0` to `1.0` in `group`'s index | Unreachable. Every entry the core holds is in `order`, so the position is never missing. |
| `write/stamps.rs` | 61 | the `w > 0.0` guard in `desired_stamp_size` to `true`, and `>` to `>=` (2) | Unreachable. `sniff_binary_metadata` reports a size only for a PNG or JPEG whose width and height are both nonzero. |

Writing the `direct-delivery` steps turned up one difference, off the contract. An `annotation.updated` event with no
`appearance` makes the TypeScript throw reading `changed` (`packages/plugin/annotation/src/sync/remote.ts:51`); the port applies
it. The `direct-delivery` case sends a well-formed event, and the port is unchanged.
