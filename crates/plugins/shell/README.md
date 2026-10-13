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

`scripts/mutants.sh -p epdf-plugin-shell --test-package epdf-plugin-shell --file <file>`, file by file
over every module but `harness.rs`, catches 77 of 79 viable mutants (77 caught, 0 timeouts; 18
unviable). No new cases were needed. Both survivors are equivalent:

- `src/model.rs`: `initial_shell_state()` replaced with `Default::default()`. `ShellState` is
  `ShellSnapshot`, whose derived default is an empty `JsRecord` and an empty `Vec`, the same value.
- `src/record.rs`: the record visitor's `expecting` replaced with `Ok(Default::default())`. It only
  words a serde error for a non-object, and the public API takes typed values, so that text only
  ever reaches a `[shell harness]` error that no TypeScript outcome is compared with.

## Native exposure

Shell ships to Swift (`ShellCapability`, `2026-10-06-swift-plugins/12-shell.md`). It stores which
named surfaces and menus are open per document; the app still owns their UI. The commands plugin
toggles panel, modal and menu targets through it, and per-document panel state and snapshots come
with it, so a native reader wants it for the same reasons the web viewer does.

`props` is opaque by contract, so it crosses UniFFI as `props_json`, a JSON object in a string.
Anything that isn't an object is refused with `invalid-input`. The Swift layer adds `Codable`
helpers over the string and no schema of its own.
