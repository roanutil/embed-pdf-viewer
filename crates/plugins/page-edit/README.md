# epdf-plugin-page-edit

`packages/plugin/page-edit` in Rust, at upstream `b456ee4fd`: rotate, reorder, delete, insert,
duplicate and extract pages, each edit one change of the document's history, plus the page
selection. The vectors are `vectors/plugins/page-edit.json`, 19 scenario cases written by
`tooling/plugin-conformance/src/suites/page-edit.ts` (`cargo test -p epdf-plugin-page-edit`).
17 of them replay; the other two wait on a generator repair described under "Open".

The modules follow the TypeScript files. `contract.rs` is `contract.ts` with the token `PAGE_EDIT`,
`model.rs` is `model.ts` (`selected` returns the state itself when nothing changes, so it writes
nothing), `controller.rs` is `controller.ts` and `state.rs` is `state.ts`. The plugin is
document-scoped and its state is `{ selected }`.

A verb refuses before anything starts: the permission (`doc.pages.assemble`, or `doc.download` for
`extract` and also for `duplicate`), an aborted signal, then its pages and its position, each read
from the confirmed page list at the call. A page argument is a `PageArg`, a ref or an index. A
position is `Start`, `End`, or `After`/`Before` a page, sent to the engine as a ref. Then the verb
stages one change on `ctx.changes` labelled `pages.<verb>` with the page count, and resolves once
the engine answers. Nothing predicts pages: the layout changes when the engine's page fact lands,
which is before the verb settles. `rotate_by` turns from the rotation the pending changes give a
page, so two quick quarter turns are a half turn, and stages the undo as each page turned back.

## Where the port differs in shape

Verbs take an options struct with `signal: Option<AbortSignal>`, where the TypeScript takes
`OperationOptions`. `rotate_by` and `set_rotation` take any `f64`, and the wrap and the grouping
are done the way JavaScript does them (one `NaN` group, `-0` is `0`).

`PageArg::ForeignRef(f64)` is a ref whose `objectNumber` no PDF object can have (`1.5`, `NaN`,
`0`). A typed caller never builds one. It exists so a JavaScript caller's ref is refused with the
number printed as the TypeScript prints it (`page 1.5 is not in this document`).

`PageEditInsertResult.pages` holds the engine's `insertedPages` as given. An `insertedPages` that
is not an array reads as `[]`, where the TypeScript resolves whatever the engine sent.

`can_edit` and `can_extract` read through the kernel's `allows`, which answers `false` when the
session has no handle. The TypeScript's `ctx.allows` throws there.

When staging an insert fails, an extracted `{"$hostBytes": n}` source is read once so the host lets
it go. The TypeScript holds the bytes in memory and has nothing to release.

A position object with neither `after` nor `before` can't be expressed by `PagePosition`. The
harness reads it as `Before` a page that isn't there, so it refuses `not-found`; the TypeScript
throws a `TypeError` from `pageRefsEqual`. No vector sends one.

## Open

`add-blank-fractional-index` (steps 3 and 4) and `insert-a-subset-names-the-missing-index` (steps 7
to 9) pass `NaN`, `Infinity` and `-Infinity` as step arguments, and the generator writes the steps
with `JSON.stringify`, so the vector holds `null` for all three
(`tooling/plugin-conformance/src/suites/page-edit.ts:228` and `:362`). The expected messages still
name the real number, so no replay can reproduce them from the input. Other suites author
`{ $number: 'NaN' }` in steps (`annotation-tools.ts:471`), which `decodeValue` in `scenario.ts:589`
turns back into the number. The repair is the same there, then regenerating the vector. Until then
the harness reads a `null` number as `NaN`, which matches the `NaN` steps by coincidence.

## Migrating from the pre-13.19 API

For viewer-host and the Swift package (13.33).

| Old | New |
|---|---|
| `PagePlacement::{After, Before, Index, End}` | `PagePosition::{Start, End, After(PageArg), Before(PageArg)}`. Index positions are gone |
| verbs take `Vec<PageRef>` | `Vec<PageArg>` and an options struct with `signal` |
| `move_pages(pages, placement)` | `reorder(pages, position, options)` |
| verbs resolve the engine's result (`Value`) | `Promise<()>`; inserts resolve `PageEditInsertResult { pages }` |
| `can_edit() -> Result<bool, Error>` | `can_edit() -> bool`, plus `can_extract` |
| one `SerialQueue` for every verb but `extract` | the document's change queue; edits apply in the order they were staged |
| new | `get_selected`, `select`, `page_edit_state`, `PageEditState`, `initial_page_edit_state` |

`src/ffi.rs` and `tests/ffi.rs` were migrated by hand and are not compiled until `epdf-ffi` builds.
The handle keeps its layout records by reading the page list when each verb settles.
`PageEditPlacement::Index` is gone and `Start` is new. The ffi's `move_pages_to_end` is gone: `End`
is the engine's own `'end'`, so the index the engine used to refuse is no longer sent.
`epdf_ffi::page::decode_pages` still reads the retired `pageObjectNumber` ref, and the plugin's
`signal` is not wired to the `CancelToken` yet; both belong to 13.33.
`crates/epdf-viewer-host/tests/viewer_host.rs:1844` uses `PageEditPlacement::Index`.
