# epdf-plugin-annotation

`packages/plugin/annotation` in Rust, on the v3 controller contract: the pure `epdf-annotation-core`
model wired to the engine and to the interaction hub. All 932 cases in ten vector files replay
identically (`CARGO_BUILD_JOBS=6 cargo test -p epdf-plugin-annotation`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/annotation-repository.json` | function | 685 | `repository/`: `fromDTO` over 60 fixtures (measurement dimensions and rich free-text bodies among them), `toPatch`, `toScopedPatch` (geometry, caption, leader and 11 prop scopes), `toCreateDraft`, the seam helpers |
| `vectors/plugins/annotation-pure.json` | function | 141 | the tool registry, `buildTextItems`, `iconPlacementDraft`, the reducer, the rich-text policy (`rich-text.ts`), `geometryFromInput`, `previewBucket`, `engine-core`'s `buildCommentThreads` and `sniffBinaryMetadata` |
| `vectors/plugins/annotation-api.json` | scenario | 15 | the page-space reads and writes, the change events, `selectAll`, `selectInRect`, run restyles over an editor range, rich-text editing, `createAttachment`, `placeStamp`, `readAttachment` without `downloadFile`, the measurement seam, the v3 document events, preview providers |
| `vectors/plugins/annotation-hydration.json` | scenario | 16 | whole-document hydration, the cursor-ordered replay, remote events, page membership, form and redaction events, `reloadPage` |
| `vectors/plugins/annotation-crud.json` | scenario | 13 | the raw create, update and delete, the optimistic effects and their rollback, selection, props and flags, text editing, settings, projections |
| `vectors/plugins/annotation-comments.json` | scenario | 15 | comment threads and their verbs, attached links, the script commit door, attachment download, baked appearances |
| `vectors/plugins/annotation-grouping.json` | scenario | 5 | `group`/`ungroup` and their twins, the grouped Replace Text create and its rollback |
| `vectors/plugins/annotation-tools.json` | scenario | 31 | tools in the hub and the five pointer handlers, click placement, stamps, the file-picker port, ghosts, behaviors |
| `vectors/plugins/annotation-markup.json` | scenario | 8 | markup, caret and replace-text from the selection plugin's signals, `createFromSelection` |
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

## Mutation testing, pre-v3, rerun pending

The numbers below are from the v2 port. `capability.rs`, `effects.rs`, `reducer.rs` and `types.rs`
are gone (their code moved into the v3 areas), so their rows are dropped. The rest describe files
that still exist, some renamed (`handler.rs` is `tools/handlers.rs`, `tools.rs` is
`tools/definitions.rs`, `threads.rs` is `comments/build.rs`, `markup.rs` is
`tools/markup_bridge.rs`, `placement.rs` is `write/placement.rs`), and wait on a rerun through
`scripts/mutants.sh`.

Every source file but `harness/` (excluded in `.cargo/mutants.toml`) has been run, one file at a
time (`scripts/mutants.sh -p epdf-plugin-annotation --test-package epdf-plugin-annotation --file
<file>`); `repository/kinds/mod.rs` has no mutants. Of the 165 that survived, 118 are killed by
cases added afterwards and 47 are equivalent or unreachable, each listed below. "Killed after"
counts the first group; each of those was confirmed by applying the mutant by hand and watching the
replay fail, not by a second full run. The nine files the first pass covered (`effects` through
`value`) are fresh runs; that pass had counted 15 misses in them, and 8 remain.

| File | Caught | Timeouts | Missed | Unviable | Killed after | Accepted |
|---|---|---|---|---|---|---|
| `handler.rs` | 75 | 0 | 28 | 7 | 18 | 10 |
| `tools.rs` | 73 | 0 | 17 | 6 | 11 | 6 |
| `binary.rs` | 50 | 1 | 21 | 10 | 17 | 4 |
| `repository/mod.rs` | 29 | 0 | 0 | 1 | | |
| `repository/links.rs` | 2 | 0 | 0 | 0 | | |
| `repository/projection.rs` | 39 | 0 | 0 | 3 | | |
| `repository/props.rs` | 9 | 0 | 1 | 1 | 1 | |
| `repository/seam.rs` | 67 | 0 | 3 | 4 | 1 | 2 |
| `repository/kinds/free_text.rs` | 13 | 0 | 3 | 3 | 3 | |
| `repository/kinds/misc.rs` | 15 | 0 | 0 | 10 | | |
| `repository/kinds/quads.rs` | 48 | 0 | 6 | 4 | 2 | 4 |
| `repository/kinds/shape.rs` | 5 | 0 | 0 | 1 | | |
| `repository/kinds/stroke.rs` | 22 | 0 | 0 | 5 | | |
| `hover_feed.rs` | 6 | 0 | 1 | 2 | 1 | |
| `markup.rs` | 7 | 0 | 1 | 0 | 1 | |
| `placement.rs` | 8 | 0 | 0 | 0 | | |
| `promise.rs` | 5 | 0 | 0 | 11 | | |
| `text_item.rs` | 6 | 0 | 0 | 1 | | |
| `threads.rs` | 54 | 0 | 5 | 7 | 1 | 4 |
| `value.rs` | 13 | 0 | 0 | 0 | | |

Timeouts count as caught: the one is a mutant that loops forever.

### Accepted misses

| Line | Mutant | Why no case kills it |
|---|---|---|
| `handler.rs` 43, 73, 123, 278, 413 | each handler's `id()` to `""` and to `"xyzzy"` | Unobservable. The hub never reads a handler's id; it orders by `priority()` and gates by `enabled_for()`. |
| `tools.rs` 69 | the `mode == "footprint"` guard in `GhostPolicy`'s decoder to `true` | Unreachable by a vector. It only changes what an object without that mode decodes to. That object is outside `GhostPolicy`'s type: the TypeScript draws a ghost for anything but `false`, and the port refuses to decode it, the same split as the core's unknown enum strings. |
| `tools.rs` 293, 301 | delete `subtype` from the `draw` and `markup` built-in rows | Equivalent. Those rows set the subtype to the tool's id, which is what resolution falls back to. |
| `tools.rs` 294 | delete `cursor` from the `draw` row | Equivalent. Its value is `crosshair`, the resolved default. |
| `tools.rs` 439 | `\|\|` to `&&` in `merge_defaults` | Equivalent. When only one side has line endings, merging them over the empty default gives that side's endings, which the spread already holds. |
| `tools.rs` 549 | `!=` to `==` on `ToolRegistry::delete`'s return | Unobservable. Its one caller, the `registerTool` unregister, ignores the result. |
| `binary.rs` 31, 48 | the `is_undefined` guards in `not_a_blob` and `resolve_now` to `false` | Unreachable. The harness decodes an explicit `{"$undefined": true}` member to `null`, and an omitted member is `None`, so the undefined marker never reaches either match. |
| `binary.rs` 105 | `offset + 4 < len` to `<=` in `parse_jpeg` | Equivalent. With four bytes left, every branch of the extra pass returns `None` or leaves the loop: an SOF needs nine. |
| `binary.rs` 129 | `\|` to `^` in the segment length | Equivalent. The high byte is shifted clear of the low one, so the bits never overlap. |
| `repository/seam.rs` 126 | the `truthy_num(r)` guard on the rotation in `box_geom_from_dto` to `true` | Equivalent. It only lets a 0 or `NaN` rotation through, and `from_pdf_rotation` turns those into 0 and `NaN`, which the two `truthy_num(rot)` tests below treat as no rotation either way. |
| `repository/seam.rs` 304 | `content_rect_of` to `Default::default()` | Unobservable. Nothing in the workspace calls it. |
| `repository/kinds/quads.rs` 85, 88, 91, 94 | `<` to `<=` and `>` to `>=` in `pdf_bounds_of_quads` | Equivalent. On a tie the running bound is replaced by an equal value. |
| `threads.rs` 130 | `x < y` to `<=` in `chronological` | Equivalent. It sits under `x != y`. |
| `threads.rs` 141 | `d > 0.0` to `==`, `<` and `>=` | Equivalent. `sort_by` only asks whether the answer is `Less`, and every mutant still answers `Less` exactly when `d < 0`. |
