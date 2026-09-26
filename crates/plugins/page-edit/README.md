# epdf-plugin-page-edit

`packages/plugin/page-edit` in Rust, at the v3 contract. All 17 cases in
`vectors/plugins/page-edit.json` replay identically (`cargo test -p epdf-plugin-page-edit`).

The crate has the TypeScript's shape: `contract.rs` and `controller.rs`, with
`create_page_edit_controller` returning the api and no `connect`. Pages are addressed by `PageRef`,
placements are `After(ref)`, `Before(ref)`, `Index(n)` or `End`, and the verbs follow the v3 names
(`insertBlank`, `insertFromBytes`, `insertFromDocument`, `duplicate`, `extract`). Every verb but
`extract` runs on one `SerialQueue`, so a delete reaches the engine only after an earlier rotate
settles (`deferred-and-aborted`). Engine failures are mapped through `toPluginError`: a refused
edit is `permission-denied` with the engine's `PermissionDenied` as its cause
(`refused-by-the-engine`). `can_edit` returns `Result<bool, Error>` because the TypeScript's
`ctx.doc` getter can throw.

The turn costs follow the TypeScript. Calls through `ctx.doc` go through the guarded handle, whose
promises are plain, so awaiting one costs one turn. The raw engine, the source document
`insertFromBytes` opens and another document's handle answer with `AbortablePromise`s, which
`await` adopts as thenables in three turns (`await_adopted`), and a promise an `async` verb returns
costs the adoption turn (`returned`).

Two JavaScript behaviours are ported on purpose. `rotateBy` does its wrap in `f64`, so a delta of
`45`, `0.5` or `NaN` reaches the engine exactly as the TypeScript computes it, and the groups are
keyed the way a `Map` keys numbers (one `NaN` group). And an `index` placement that isn't an
integer makes `neighbourSize` read `pages[1.5]`, which throws
`TypeError: Cannot read properties of undefined (reading 'size')`; the Rust returns that error
verbatim (`add-blank-fractional-index`).

## Coverage

The page-edit suite reaches all 294 statements of the TypeScript package and 85 of its 88
branches. The three it cannot reach are the no-document arms in `controller.ts`, and none is
reachable from the public surface:

- `controller.ts:30`, `requireDoc`'s `if (!doc)`. The plugin is document-scoped, so `ctx.doc` is
  always the guarded handle; a session without a handle throws from the getter instead.
- `controller.ts:40`, the `ctx.document()?.pages ?? []` fallback. A capability captured before its
  document closed still reads the last registry the kernel staged
  (`a-captured-capability-outlives-its-document`).
- `controller.ts:75`, `ctx.doc?.` in `canEdit`, for the same reason as line 30.

## Pinned TypeScript behaviour

After its document closes, a captured capability's verbs still reach the engine: the guarded handle
calls the underlying method first and only then refuses the result with `instance-closed`
(`a-captured-capability-outlives-its-document`). The engine log shows the `pages.rotate` and
`pages.insertBlank` calls. That looks like a defect in `guarded-handle.ts`, whose comment promises
a closed instance's calls are refused.

`duplicate` of a page the registry doesn't know extracts it from the engine first, then rejects
`not-found` resolving the default placement after it (`duplicate`).

`insertFromBytes` with `pageIndexes` opens its source as `page-edit-source-<Date.now() in base 36>`,
so two subsets opened in the same millisecond would share an id. The verbs are serialized, so the
plugin itself never does that.

A placement object with none of `index`, `after` or `before` is not ported. The TypeScript reads
`placement.before` as `undefined` and throws reading its `pageObjectNumber`; `PagePlacement` is an
enum and cannot express it.

Mutation numbers are pre-v3, rerun pending. `cargo mutants -p epdf-plugin-page-edit --test-package
epdf-plugin-page-edit` caught all 45 viable mutants of the v2 crate.
