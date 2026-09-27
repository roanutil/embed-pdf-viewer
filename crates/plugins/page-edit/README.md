# epdf-plugin-page-edit

`packages/plugin/page-edit` in Rust, at the v3 contract. All 19 cases in
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

`scripts/mutants.sh -p epdf-plugin-page-edit --test-package epdf-plugin-page-edit --file <file>`,
file by file over every module but `harness.rs`, catches 58 of 66 viable mutants (58 caught, 0
timeouts; 63 unviable). Nine of those are killed by the `insert-a-subset-names-the-missing-index` and
`rotate-by-a-page-without-a-rotation` cases, checked by applying each mutant by hand: the seven in
`base36` (the run's clock sat at 0, which returns before the loop) and the two `&&` to `||` swaps in
`rotate_by`'s NaN grouping and `insert_from_bytes`'s index check. No mutant survives.

The other eight were in the crate's own `js_string`, which no longer exists. The new case showed it
was wrong: it cast every integer below `1e21` to `i64`, so index `1e20` printed as
`9223372036854775807`, and it printed `1e21` and `1e-7` in plain notation. The error message now
uses `epdf_geometry::js::number_to_string`, whose mutants are counted in `epdf-geometry`.
