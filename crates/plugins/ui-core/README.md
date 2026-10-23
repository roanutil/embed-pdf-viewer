# epdf-ui-core

`packages/core/ui` in Rust: the bar schema, the fit solver, the overflow projection, the viewer's
layout with its edits and its validation, and the shortcut parser. All 1,238 cases in
`vectors/core/ui-*.json` replay identically (`cargo test -p epdf-ui-core`).

| Vector file | Cases | Module |
|---|---|---|
| `core/ui-schema.json` | 48 | `schema.rs`, `layout.rs`, `layout_validate.rs` |
| `core/ui-solver.json` | 303 | `solver.rs` |
| `core/ui-projection.json` | 28 | `projection.rs` |
| `core/ui-transforms.json` | 13 | `layout.rs` |
| `core/ui-shortcuts.json` | 843 | `shortcuts.rs` |
| `core/ui-helpers.json` | 3 | `layout.rs` |

The package's own layout, validation and suggestion tests run as Rust in `tests/behavior.rs`. Where
the TypeScript asserts `toBe`, the test asserts `Rc::ptr_eq`: an edit keeps every bar, menu, panel
and group it didn't touch as the same allocation, as the TypeScript keeps the reference.

The generator is `tooling/plugin-conformance/src/suites/ui-core.ts`. Callbacks cross as tables:
`FitMetrics` is a width table with per-variant and per-group defaults, `ResolveMenuTarget` is a map
from command id to `{menu?}`, and `filterBar`'s `keep` and `projectStrip`'s `visible` are lists.
`harness.rs` builds the same callbacks from the same tables. The shortcut module reads no platform
global, since `isMac` is already an argument.

Six solver sweeps over four bars step the width from 0 to past the richest layout (every 15 to 25
pixels, plus one either side of known breakpoints), so the whole degradation ladder is pinned, not
just the points the package's own tests check.

## JavaScript semantics

`js.rs` holds what the TypeScript gets for free. `Record` iterates as `Object.entries` does
(integer-like keys first, then insertion order), which `validateLayout`'s problem list depends on. `JsMap` is a `Map` and prints as `{"$map": ...}`, which is
what `FitResult.units` and `FitResult.groups` are. `trim` uses the ECMAScript whitespace set, which
differs from `char::is_whitespace` on U+FEFF and U+0085. Lengths are UTF-16 code units, and
`capitalize` leaves a string whose first character is outside the BMP alone, because `charAt(0)`
takes half a surrogate pair. The solver sums widths in the TypeScript's order, so fractional widths
come out as the same `f64`.

The layout is plain data from people, so it is a `JsValue` (`value.rs`), not typed structs. It
keeps JavaScript's member order through spreads (a section added to a bar goes last), and it reads a
member of the wrong type the way the TypeScript does: skipped by the edits and named by
`validate_layout`. The layout cases match the TypeScript's member order with no entry in
`crates/plugin-conformance/member-order-allow.json`
(`layout_cases_keep_member_order_with_no_allowance` in `tests/conformance.rs`). Its entry for
`core/ui-transforms.json` `replace-everywhere` is gone. The two `core/ui-schema.json` entries stay: `group` returns a typed `BarGroup`, whose members print in one order where the
TypeScript prints the caller's options in theirs.

Case folding is Rust's `to_lowercase` and `to_uppercase`. They agree with V8 on every string the
vectors try, final sigma, `İ`, `ß` and `ŉ` included. A character added in a Unicode version one side
has and the other lacks would fold differently.

## Coverage

Stale as of Task 13.10: the counts below are for the package before the layout replaced the chrome
schema, and Task 14 re-measures them.

The generators reach all 1,207 lines of the package and 336 of its 337 branches. The miss is
`shortcuts.ts:65`, the `empty key` throw in `parseShortcut`. An empty last token becomes `+`, and
lowercasing a non-empty string never empties it, so no input reaches it. The Rust keeps the check.

## Mutation testing

Stale as of Task 13.10, for the same reason; Task 14 reruns it.

`cargo mutants -p epdf-ui-core --test-package epdf-ui-core` catches 279 of 284 viable mutants and
times out on 3, each of which makes the solver loop forever. Two survive, and both are equivalent:

- `solver.rs:171`, `<` to `<=` on the collapsed-group guard. A group only collapses when its
  importance is below `PINNED`, so a collapsed group is never pinned.
- `js.rs:70`, `>` to `>=` in `Record::insert`. Equal indices mean the same key, and an existing key
  is replaced before this line runs.

## Not ported

Inputs that break the TypeScript types are out of scope. The typed Rust rejects a `BarItem` variant
outside `Variant`, an importance that is not a small integer, and members the types don't declare,
all of which the TypeScript would pass through untouched. Because members are typed, a spread that
would carry an unknown member (`{...schema}`, `{...g}`) drops it in Rust. The one type-invalid
shape the solver could meet at runtime is a hand-built unit with an empty ladder, which
`normalizeBar` rejects. The TypeScript measures it at variant `undefined` and assigns `{kind:
'variant'}` with no variant; Rust treats it as unmeasured and assigns an empty string.

Three files of the package are not ported, and no vector covers them: most of `layout-compile.ts`
(`compileBar`, the edge and float stacks, `menuOf`, `omittedCommandsOf`), `toolbar-layout.ts` and
`tree-keys.ts`. They turn a layout into the toolbars and trees a viewer draws, and no Rust host draws
one yet. `layout_compile.rs` holds only what `validate_layout` reads: `edge_of`, `host_of`,
`modes_of` and the command names.

Three Rust-only divergences, each pinned by a test (`tests/behavior.rs`, and `value.rs` for the
third):

- Members are own members. `bars['constructor']` finds `Object.prototype`'s function in the
  TypeScript, which then edits it as a bar; Rust reads no bar there. For a key named `constructor`
  or `__proto__`, `formatShortcut` finds an `Object.prototype` member in `KEY_NAMES`
  (`packages/core/ui/src/shortcuts.ts:139`) and prints its missing name as nothing elsewhere
  (`Ctrl+`) and as `undefined` on a Mac (`⌃undefined`, `⌘undefined`). Rust prints `Ctrl+Constructor`,
  `⌃Constructor` and `⌘__proto__`. The same choice as `epdf-annotation-core`'s kind names and
  `epdf-fonts`' face names.
- A `null` bar or panel, or layout data that is `null`, is read as empty. The TypeScript throws a
  `TypeError` reading a member of one in `validateLayout` (beside a bar placed `before` or `after`
  another), `replace`, `removePanel`, `panels` with an object `present` or a `width`
  (`layout.ts:664`), and any edit of `createLayout(null)`.
- A string spread into an object splits into UTF-16 units in the TypeScript. Rust can't hold half
  a surrogate pair, so each half of one reads as U+FFFD. Only a bars, menus or panels record that is
  a string reaches this.

Options outside their types read as absent. `AddItemOptions`' `before`, `after`, `group` and
`section` and `CustomItem.terminal` are `Option<String>`, so a `null` is no option at all, where the
TypeScript tells `null` from `undefined`: `{ before: null, after: 'x' }` cuts before `'x'`,
`group: null` starts a group with id `null`, and `{ slot, terminal: null, importance: 2 }` keeps
importance 2 where Rust pins the item. Typed TypeScript and the vectors never pass these.
