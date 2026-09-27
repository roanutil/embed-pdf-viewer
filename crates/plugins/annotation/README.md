# epdf-plugin-annotation

`packages/plugin/annotation` in Rust, on the v3 controller contract: the pure `epdf-annotation-core`
model wired to the engine and to the interaction hub. All 956 cases in ten vector files replay
identically (`CARGO_BUILD_JOBS=6 cargo test -p epdf-plugin-annotation`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/annotation-repository.json` | function | 693 | `repository/`: `fromDTO` over 60 fixtures (measurement dimensions and rich free-text bodies among them), `toPatch`, `toScopedPatch` (geometry, caption, leader and 11 prop scopes), `toCreateDraft`, the seam helpers |
| `vectors/plugins/annotation-pure.json` | function | 151 | the tool registry, `buildTextItems`, `iconPlacementDraft`, the reducer, the rich-text policy (`rich-text.ts`), `geometryFromInput`, `previewBucket`, `engine-core`'s `buildCommentThreads` and `sniffBinaryMetadata` |
| `vectors/plugins/annotation-api.json` | scenario | 18 | the page-space reads and writes, the change events, `selectAll`, `selectInRect`, run restyles over an editor range, rich-text editing, `createAttachment`, `placeStamp`, `readAttachment` without `downloadFile`, the measurement seam, the v3 document events, preview providers |
| `vectors/plugins/annotation-hydration.json` | scenario | 16 | whole-document hydration, the cursor-ordered replay, remote events, page membership, form and redaction events, `reloadPage` |
| `vectors/plugins/annotation-crud.json` | scenario | 13 | the raw create, update and delete, the optimistic effects and their rollback, selection, props and flags, text editing, settings, projections |
| `vectors/plugins/annotation-comments.json` | scenario | 15 | comment threads and their verbs, attached links, the script commit door, attachment download, baked appearances |
| `vectors/plugins/annotation-grouping.json` | scenario | 5 | `group`/`ungroup` and their twins, the grouped Replace Text create and its rollback |
| `vectors/plugins/annotation-tools.json` | scenario | 33 | tools in the hub and the five pointer handlers, click placement, stamps, the file-picker port, ghosts, behaviors |
| `vectors/plugins/annotation-markup.json` | scenario | 9 | markup, caret and replace-text from the selection plugin's signals, `createFromSelection` |
| `vectors/plugins/annotation-hover.json` | scenario | 3 | hover state, the enter/exit feed onto the actions hover pump, the permission twins |

The older scenario files were written against v2 names and reach v3 through the mapping table in
`annotation-fixtures.ts`. `annotation-api.json` calls v3 by name, for the surface that table never
reached.

## The API sibling plugins use

Redaction, link, form, stamp, measurement and signature call this plugin through the TypeScript's
`/contract/host`. Here the public and host lenses are one type, `AnnotationCapability`
(`AnnotationHostCapability` is an alias), resolved with `ANNOTATION`. Its methods are the
TypeScript's in snake case, grouped by the file that defines them: `read/` (`get`, `list`,
`list_raw`, `hit_test_at`, `list_page_items`, `get_hit_kind`, ...), `write/` (`create`,
`update`, `create_raw`, `update_raw`, `place_stamp`, `create_attachment`, `set_placement_preview`,
`set_page_viewports`, `remeasure_page`, ...), `sync/` (`get_status`, `refresh`, `reload_page`,
`deliver_remote_event`), `tools/` (`register_tool`, `register_behavior`, `get_resolved_tool`) and
the events (`on_created`, `on_draft_captured`, ...). `comments()` and `links()` return the two
object members as lenses. `create_annotation_controller`, the tool table, the repository and the
five handler constructors are public, as `/internal` exports them.

Engine DTOs stay `serde_json::Value`: the model carries each one verbatim in `Annot.data`, and
every engine call sends JSON built the way the TypeScript's object literal is.

## Host inputs

`AnnotationHost` on `AnnotationConfig` carries the three ambient values the TypeScript reads:
`console_error` (every failure the plugin swallows; the vectors log it as a `console.error`
callback), `random_uuid` (`crypto.randomUUID()`, seeding `/NM`), and `fonts`, the registered-font
list `ctx.engine.fonts.list()` answers. The Rust engine contract has no fonts service, so the host
passes the list; `None` is no registered fonts, which is what the fake engine and the cloud engine
have. The face mapping over a registered list is pinned by the function cases in
`annotation-pure.json`.

## Turn order

The trace records how engine answers and model updates interleave, so each `await` and `.then` is
mirrored (`src/promise.rs`). A `.then` on an engine call reacts one turn after it settles; an
`await` on one adopts the guarded `AbortablePromise` as a thenable. `Promise.allSettled` over the
write sink adopts each abortable element the same way. An `async` function returning a promise
costs the adoption job and its reaction.

## TypeScript behaviour, re-checked under v3

The v2 README pinned two behaviours and one gap. Under v3:

- `armStamp` no longer disarms itself. It activates `ARMED_STAMP_TOOL_ID`, and the tool-change
  callback keeps the payload on that tool (and on any tool with the legacy `annotation-stamp` tag).
  The `armed-stamps` case in `annotation-tools.json` reads `hasArmedStamp()` as `true` after arming
  with the default tools.
- `refresh()` (v2 `rehydrate`) still never re-checks read authority. `runHydration` returns an
  already-resolved promise when `canRead()` is false, `rehydrate` stores it as `running`, and only
  the `listRawAll` chain's `finally` clears `running`. Every later `refresh()` sets `again` and
  returns that promise, so a grant arriving through `/access` is never seen. The port keeps this;
  `no-read-grant-means-forbidden` in `annotation-hydration.json` pins it.
- `readAttachment` on a page service without `downloadFile` now has a vector. The fake engine's
  `lacks` list can name `page.annotations.downloadFile` on both sides, and
  `attachments-stamps-and-downloads` rejects with the TypeScript's error. The Rust side asks
  `DocumentHandle::has("page.annotations.downloadFile")`.

## Where the port differs in shape

The TypeScript memoizes its projections by model identity so a subscribed selector gets a stable
reference. Canonical JSON has no identity, so the port recomputes every read; the values are the
same.

The store announces a draft change when the draft differs by value; the TypeScript compares the
draft object by identity. They disagree only when an update rebuilds the draft with equal content,
which no vector does.

`remeasurePage` walks `Object.values(byId)`, which is insertion order. The Rust model's map is
sorted, so the port walks `order`, which follows insertion for every record the model holds.

A page object number the document does not know makes `doc.page(ref)` throw. Where that throw
would escape a synchronous path (the stamp and icon placements, the effect runners), the Rust
skips the call or logs it. The crop lookup that precedes each one comes from the same page list, so
no vector reaches it.

## Coverage

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
