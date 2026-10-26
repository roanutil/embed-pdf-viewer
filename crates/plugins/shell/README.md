# epdf-plugin-shell

`packages/plugin/shell` in Rust, re-ported in Task 13.28 to the current TypeScript
(`packages/plugin/shell/src/{controller,model}.ts`). All 40 cases in
`vectors/plugins/shell/reducer.json` and all 7 in `vectors/plugins/shell/plugin.json` replay with no
member-order allowance or outcome exception (`cargo test -p epdf-plugin-shell`), and
`tests/behavior.rs` pins what the vectors can't print.

The plugin is document-scoped: each document keeps its own surfaces and menus, the capability does
not resolve before a document opens (`no-shell-without-a-document`), and closing a document drops
its slice, so reopening it starts empty (`per-document-state`).

The modules follow the TypeScript files. `model.rs` holds the named transitions (`open_surface`,
`close_surface`, `toggle_surface`, `set_surface_props`, `close_all_surfaces`, the four menu verbs,
`snapshot_of` and `apply_snapshot`), each a pure function from `&Rc<ShellState>` to `Rc<ShellState>`
that hands back the same `Rc` where the TypeScript returns `state`. `reducer.json` calls them by
name. The controller applies them through `Controller::update_state` and derives every event in one
`on_state_change` listener: an opened or closed event per surface whose open flag changed, then the
menu closes, then the menu opens. The events are kernel event sources, so a failing listener goes
to the kernel report.

A surface record carries `focusRequest`, which a focused open raises without firing another opened
event and every close sets back to 0. Snapshots leave it out, and `apply_snapshot` restores every
record at 0.

## Identity

`getSurface`, `listOpenSurfaces` and `getSnapshot` are TypeScript `memoByKey`/`memo` reads. Rust
returns `Rc`s that stay the same while their input record, `surfaces` map or state is the same
`Rc`, and each memo holds the input `Rc` it compares with `Rc::ptr_eq`. `listOpenMenus` hands out
the state's own `Rc<Vec<String>>`. The `surfaceOf` cache has no bound, as in the TypeScript.

## Pinned TypeScript behaviour

Surfaces enumerate in JavaScript property order, integer-like ids first. `src/record.rs` is that
order; it is the same `JsRecord` `epdf-plugin-i18n` carries.

A record prints its members in the order the TypeScript built them. `openSurface` writes `open,
exclusive, props, focusRequest`, and a spread keeps the order it copies and appends what it adds,
so `updateSurfaceProps` on a record restored without props puts `props` after `focusRequest`
(`set-props-on-a-closed-surface`). `model::Members` tracks that order.

`getSurface('toString')` returns `{id: 'toString'}`: the record lookup finds the inherited function,
and the view spreads nothing from it. `SurfaceState::record` is `None` for that case. `isOpen` and
`close` on the same names read as absent on both sides, and `toggleSurface('valueOf')` opens an own
surface (`toggle-proto-member-opens`).

Two inputs are not ported. A surface id of `__proto__` sets the prototype of the TypeScript's
record instead of adding a surface, so the surface is readable through `isOpen` but never listed,
and the next transition's spread drops it; Rust stores it as an ordinary id. And
`updateSurfaceProps('toString', props)` finds the inherited function and stores `{props}` with no
open flag under that id; Rust ignores it, as it ignores any unknown id. A record object with a
member no surface record has (`{open: true, extra: 1}` in a snapshot) loses that member in Rust,
where the TypeScript spread keeps it. No vector reaches any of the three.

The mutation results recorded before Task 13.28 (77 of 79 viable mutants caught) measured the old
reducer port and no longer describe this code; the task did not rerun `scripts/mutants.sh`.

## Native exposure

Shell ships to Swift (`ShellCapability`, `2026-10-06-swift-plugins/12-shell.md`). It stores which
named surfaces and menus are open per document; the app still owns their UI. The commands plugin
toggles panel, modal and menu targets through it, and per-document panel state and snapshots come
with it, so a native reader wants it for the same reasons the web viewer does.

`props` is opaque by contract, so it crosses UniFFI as `props_json`, a JSON object in a string.
Anything that isn't an object is refused with `invalid-input`. The Swift layer adds `Codable`
helpers over the string and no schema of its own. `src/ffi.rs` and `tests/ffi.rs` were migrated by
hand in Task 13.28 (`focus`, `focus_request`, `ShellSnapshotSurface`, a closed event without props)
and are not compiled until Task 13.33.
