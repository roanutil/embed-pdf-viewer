# epdf-plugin-redaction

`packages/plugin/redaction` in Rust, ported to the current TypeScript (Task 13.23). A mark is a
`redact` annotation the annotation plugin makes, from the text selection, an area, a whole page or
search matches. The pending view is a projection of `annotation.list({pages: [page]})`, and applying
is the engine's `redaction.apply` in `{pages}` or `{annotations}` scope, one apply at a time. The
overlay settings are registered with the kernel and written into the redact tool's defaults, so
every mark takes its look from them. 57 cases replay (`cargo test -p epdf-plugin-redaction`):

| File | Kind | Cases | Covers |
|---|---|---|---|
| `vectors/plugins/redaction.json` | scenario | 25 | the twins, marking from the selection, by area, by page and from search, the redact tool, the pending view and its reads, collateral, `unmark` and `clearPending` with skips and refusals, labels, self-scoped ownership, `apply`, `applyPages` and `applyAll`, refusals before and at the engine, the apply queue, remote applies, a throwing listener, closing mid-apply |
| `vectors/plugins/redaction-functions.json` | function | 32 | the named session transitions, `markOf`, and `estimateCollateral` over a read-only `annotation.list` port |

All 57 replay. `mark-ownership-and-flags` needs the conformance fake to resolve
`security.rawScope` through the shipping scope rules, as `fake-engine.ts:489-560` does; `840198fc3`
(Ruling13.23) gave the Rust fake that.

The scenarios run the real interaction, selection, annotation and search adapters beside this one.
The generator is `tooling/plugin-conformance/src/suites/redaction.ts`. Its `redaction-search`
factory is `searchPlugin()`, which `harness.rs` builds too.

`tests/behavior.rs` covers what the brief asks beyond the printed traces. The apply verbs settle on
the TypeScript's turns, measured on the shipping kernel and plugins over the TypeScript fake: 7 for
`apply` and `applyAll`, 8 with a signal or through `applyPages`. A page's apply sends only the
pending marks among its refs and leaves the other page's marks as they were. The pending view keeps
its `Rc` while the annotation plane's list holds. A close mid-apply rejects `instance-closed`, reports
the dropped `finishApply` once and publishes nothing. A refused apply rolls the flag back, keeps the
last result and the marks, and the next queued apply still runs. A download started during an apply
reads the file only after the apply settles (`onSettle` waits on the queue's `idle()`). A queued apply whose signal fires never starts, and one cancelled at
the engine rejects at once with the caller's reason. `unmark` keeps skipped, failed and rejected
apart, and each apply grant is checked on its own.

## The public API

`redaction_plugin(RedactionConfig)` builds the plugin (`redaction_plugin_json` takes the registered
settings as JSON), `REDACTION` is its token and `RedactionCapability` its capability, which is also
`RedactionHostCapability`. The members are the TypeScript's in snake case: the settings
(`get_settings`, `update_settings`, `reset_settings`, `on_settings_changed`), `list_pending`,
`get_pending`, `get_pending_count`, `estimate_collateral`, `can_unmark`, `can_update_label`,
`mark_selection`, `mark_area`, `mark_page`, `mark_matches`, `unmark`, `clear_pending`,
`update_label`, `apply`, `apply_pages`, `apply_all`, `can_mark`, `can_apply`, `is_applying`,
`get_last_result`, `on_applied` and `on_pending_changed`.

Every verb returns a `Promise` and refuses by rejecting, as the TypeScript's `async` functions do.
Each takes `OperationOptions { signal }`. A page argument is `PageTarget`, a `PageRef` or a display
index. Annotation refs and the records they come from are the engine's JSON (`Value`), as the
annotation plugin hands them over. `RedactionApplyResult` is `Rc<Value>`, and `get_last_result`
returns the very `Rc` the confirmed event carried. `list_pending` returns `Rc<Vec<RedactionMark>>`.

## Migrating from the pre-13.23 API

For the viewer host and `epdf-web` (Task 13.33).

| Old | New |
|---|---|
| `RedactionConfig { overlay: Option<RedactionOverlay>, host }` | `RedactionConfig { overlay: Option<RedactionOverlayConfig> }`; a throwing listener goes to the kernel's report |
| `RedactionAction`, `redaction_reducer` | `start_apply`, `finish_apply`, `set_last_result` |
| `mark_selection()`, `mark_matches(query, pages)` resolving refs | `mark_selection(options)`, `mark_matches(query, pages, options)` resolving `MarksResult { marks }` |
| `mark_area(&page, bounds) -> Result<Promise<AnnotationRef>, Error>`, `mark_page` likewise | `mark_area(PageTarget, Rect, options)`, `mark_page(PageTarget, options) -> Promise<MarkResult>` |
| `unmark(refs)`, `clear_pending()` resolving JSON | `unmark(refs, options)`, `clear_pending(options)` resolving `BatchResult` |
| `update_label(ref, patch) -> Promise<()>`, `patch.overlay_text: Nullable<String>` | `update_label(ref, patch, options) -> Promise<MarkResult>`, `Option<Option<String>>` |
| `apply(&[AnnotationRef])`, `apply_pages(&[PageRef])`, `apply_all()`; scopes with `kind` | `apply(&[Value], options)`, `apply_pages(&[PageTarget], options)`, `apply_all(options)`; scopes `{pages}` or `{annotations}` |
| `RedactionMark { ref_: AnnotationRef, overlay_text: Option<Value>, .. }` | `ref_: Value`, `overlay_text: Option<String>`, new `repeat` |
| `regions_of`, `estimate_collateral` over PDF-space edges | page-space `Rect`s; `estimate_collateral(pages, list, refs)` takes the one-page list |
| ffi `RedactionPageStatus::{Failed, Skipped}`, `RedactionPageResult.error` | gone: the engine's apply is all or nothing (`applied` or `unchanged`) |
| ffi verbs resolving refs | resolving `RedactionMark`s |

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand, and are compiled and tested since Task 13.33,
with the viewer host's call sites.

## Where the port differs

The fakes differ on one point no vector reaches. The TypeScript fake gives every document a
redaction service; the Rust one only when `services` lists it, which every redaction vector does.
`without_a_redaction_service_applies_refuse_unsupported` uses that to run `requireService()`'s
`unsupported` branch, which the TypeScript keeps as a defensive guard (`services/store.ts:17-23`) and
no vector exercises.

A record whose `ref.page` is not a page ref cannot become a typed `RedactionMark`. The pending view
skips it and hands the decode error to the kernel's report. An engine record always has one.

`apply` reads its refs' keys when called; the TypeScript reads them in the queue's turn. Only a
JavaScript caller that changed the array in between could tell.

Two harness edges no vector reaches. `apply` with no array throws the TypeScript's `TypeError`
(`Cannot read properties of undefined (reading 'map')`), but the harness rejects it before the queue
turn, since the Rust signature takes a slice. The function table's `markOf` refuses a `pageIndex`
that is not a whole number with a `TypeError`, where the TypeScript passes any number through; the
typed mark holds an `i64`.

The ffi handle's `last_result` decodes the stored result each call, so after a remote event that
names a page status the engine no longer writes (`failed`, `skipped`), every `last_result()` refuses
`invalid-state` until the next apply replaces it
(`a_retired_failed_status_is_refused_as_invalid_state` in `tests/ffi.rs`). Task 13.33 keeps that:
no current engine writes those statuses, so only a malformed event reaches the branch, and a refusal
says so where decoding once or dropping the result would hide it.

Applies ride the kernel's `SerialQueue`: `run_with` is `serial-queue.ts`'s `{ signal }` option and
`idle()` is the settle flush a download waits on (`write/apply.rs`).
