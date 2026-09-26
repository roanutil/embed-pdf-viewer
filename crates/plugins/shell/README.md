# epdf-plugin-shell

`packages/plugin/shell` in Rust, at the v3 contract. All 32 cases in
`vectors/plugins/shell/reducer.json` and all 6 in `vectors/plugins/shell/plugin.json` replay
identically (`cargo test -p epdf-plugin-shell`).

The plugin is document-scoped, not workspace-scoped: each document keeps its own surfaces and
menus, the capability does not resolve before a document opens (`no-shell-without-a-document`), and
closing a document drops its slice (`per-document-state`). The reducer is called directly with its
reference identity made visible (`same`), so every `return state` arm is pinned to a `None`.

The modules follow the TypeScript files: `contract.rs`, `model.rs` and `controller.rs`.
`create_shell_controller` returns the api and no `connect`. Every write goes through one diffing
`dispatch`, which fires `onSurfaceOpened` and `onSurfaceClosed` per surface whose open flag changed
and then `onMenuClosed` and `onMenuOpened` per menu (`events`). `ShellState` is `ShellSnapshot`,
since `getSnapshot` hands the state out as it is. `getSurface` and `listOpenSurfaces` build their
views on every call; the TypeScript memoizes them per record, which only reference identity could
observe. A failing listener goes to stderr, where the TypeScript writes to `console.error`.

## Coverage

The shell suites reach every statement and branch of the TypeScript package: 341 of 341 statements
and 103 of 103 branches.

## Pinned TypeScript behaviour

`listOpenSurfaces()` lists surfaces in JavaScript property order, which puts integer-like ids first.
`src/record.rs` is that order; it is the same `JsRecord` `epdf-plugin-i18n` carries, copied because
the two plugins don't depend on each other.

`getSurface('toString')` returns `{id: 'toString'}`: the record lookup finds the inherited
function, and the view spreads it into an object with only the id. `SurfaceState::open` is an
`Option<bool>` for that one case. `isOpen` and `close` on the same names read as absent on both
sides (`surfaces`).

Two inputs are not ported. A surface id of `__proto__` sets the prototype of the TypeScript's
record instead of adding a surface, so the surface is readable through `isOpen` but never listed,
and the next action's spread drops it; Rust stores it as an ordinary id. And
`updateSurfaceProps('toString', props)` finds the inherited function and stores `{props}` with no
open flag under that id; Rust ignores it, as it ignores any unknown id.

Mutation numbers are pre-v3, rerun pending. `cargo mutants -p epdf-plugin-shell --test-package
epdf-plugin-shell` caught 60 of 62 viable mutants in the v2 crate. Both survivors were equivalent:
`initial_shell_state()` replaced with `Default::default()` builds the same value, and the record
deserializer's `expecting` only words a parse error.
