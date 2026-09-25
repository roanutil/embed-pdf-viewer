# epdf-plugin-shell

`packages/plugin/shell` in Rust. All 25 cases in `vectors/plugins/shell/reducer.json` and all 4 in
`vectors/plugins/shell/plugin.json` replay identically (`cargo test -p epdf-plugin-shell`).

The plugin is document-scoped, not workspace-scoped: each document keeps its own surfaces and
menus, the capability does not resolve before a document opens (`no-shell-without-a-document`), and
closing a document drops its slice (`per-document-state`). The reducer is called directly with its
reference identity made visible (`same`), so every `return state` arm is pinned to a `None`.

The generators reach every line and branch of the TypeScript package (statements 215/215, branches
57/57).

`openSurfaces()` lists surfaces in JavaScript property order, which puts integer-like ids first.
`src/record.rs` is that order; it is the same `JsRecord` `epdf-plugin-i18n` carries, copied because
the two plugins don't depend on each other.

One input is not ported: a surface id of `__proto__`. `surfaces['__proto__'] = ...` sets the
prototype of the TypeScript's record instead of adding a surface, so the surface is readable
through `isOpen` but never listed, and the next action's spread drops it. Rust stores it as an
ordinary id. Other `Object.prototype` names (`toString`, `constructor`) read as absent on both
sides, and `surfaces` pins that.

`cargo mutants -p epdf-plugin-shell --test-package epdf-plugin-shell` catches 60 of 62 viable
mutants. Both survivors are equivalent: `initial_shell_state()` replaced with
`Default::default()` builds the same value, and the record deserializer's `expecting` only words a
parse error.
