# epdf-plugin-outline

`packages/plugin/outline` in Rust, at the current TypeScript. All 20 cases in
`vectors/plugins/outline.json` replay with no member-order allowance or outcome exception
(`cargo test -p epdf-plugin-outline`). The vector is new with this crate: Task 13.31 wrote its
generator, `tooling/plugin-conformance/src/suites/outline.ts`.

The modules follow the TypeScript files. `model.rs` is `model.ts` (the tree, rows, reading-position
spots and the session transitions), `sync.rs` is `sync/tree.ts` (`foldOutline`, `predictOutline`,
`undoOf`, plus engine-core's `outlineItemOfDraft`, `outlineItemAfter`, `outlineItemWithChildren` and
`actionTreeOf`), `controller.rs` is `controller.ts`, and `state.rs` is `state.ts`.

The outline is one on-demand kernel mirror, `mirror/outline`: opening a document reads nothing, and
the first `ensure_loaded` reads it. Its value is the tree as JSON, `{items, children, incomplete}`,
with both maps keyed in JavaScript's order (array-index keys ascending, then `top`), so a store dump
prints what the TypeScript's does. Every read shows the mirror's `view()`, the confirmed tree with
this session's pending changes predicted on top. Each write is one change staged on `ctx.changes`
with its undo ops, so it shows before the engine answers, goes when the engine refuses, and its
confirmed event updates the mirror and fires `on_changed` before the write's promise settles.

The stage, actions, link, selection and i18n plugins are optional, as in `outline.plugin.ts`, and
are looked up when needed. The vector runs every activation path and the default title with and
without them. The attachment plugin is not a dependency.

The crate has no `uniffi` feature. `epdf-viewer-host` installs it (`ViewerConfig.outline`) and
`epdf-web` builds it by factory; no foreign handle exposes it yet, so Swift cannot reach it until
Task 14 adds one.

## Tests

`tests/conformance.rs` replays the vector with the real interaction, actions, stage, link,
selection, i18n and history plugins beside this one. `tests/behavior.rs` pins the event-before-
settlement order, a refused write's rollback, the build with no optional plugin, the local
refusals, and the turns each read and write settles on, measured on the TypeScript with `tsx`.

The unit tests need the `harness` feature to load. `build.rs` emits the rpath for
`libembedpdf.dylib` only under `harness`, as every sibling crate's does, but the dev-dependencies
link the engine whatever the features. So `cargo test --no-default-features --lib` aborts with
`Library not loaded: @rpath/libembedpdf.dylib`. Run `cargo test -p epdf-plugin-outline --lib`
instead; the pure build is checked with `cargo clippy --no-default-features --target
wasm32-unknown-unknown`.

## Rust-only divergences

None is reached by a vector or by an outline the engine sends.

- A loop in the tree's `children` (an item listed under its own descendant) is not entered again
  by `items_of`, `rows_of` or `subtree_of`. The TypeScript recurses until the stack overflows or,
  in `subtreeOf`, never ends. A key listed under two parents is still visited under each, as in
  the TypeScript.
- A selected-text title cut at 32,767 UTF-16 units through a surrogate pair drops the half the
  TypeScript keeps as a lone surrogate.
- `activate` without the link plugin hands a page destination to the stage's typed
  `PageDestination`. One of a known kind that does not decode (a member of the wrong type) still
  reveals its page and is reported to the kernel as `[outline] the stage cannot read destination
  ...`, where `controller.ts:212-216` passes the raw object to `goToDestination`. This is the link
  plugin's rule (`crates/plugins/link/src/controller.rs:388-410`). An engine read never sends such a
  destination.

## Cost of a change

The tree is JSON, so it can't share its items the way `sync/tree.ts` shares them with
`{ ...tree.items, [key]: item }`. A fold or prediction that changes the tree copies it once and
edits the copy in place; one that changes nothing copies nothing, and a run of predictions replayed
over each other copies it once (M39, `tree_copies_per_change` in `src/sync.rs`). On a 5,000-item
outline in a release build a rename costs about 3.3 ms per fold or prediction, from 9.7 ms when
each edit copied the tree again (15 copies for four predictions, now four). The TypeScript's spread
copies pointers, which this port would match only with a typed tree of shared items behind a mirror
token, as the form keeps its `FieldIndex`.
