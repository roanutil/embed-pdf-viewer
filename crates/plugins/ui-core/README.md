# epdf-ui-core

`packages/core/ui` in Rust: the chrome schema, the fit solver, the overflow projection, the schema
transforms and the shortcut parser. All 1,299 cases in `vectors/core/ui-*.json` replay identically
(`cargo test -p epdf-ui-core`).

| Vector file | Cases | Module |
|---|---|---|
| `core/ui-schema.json` | 61 | `schema.rs` |
| `core/ui-solver.json` | 303 | `solver.rs` |
| `core/ui-projection.json` | 28 | `projection.rs` |
| `core/ui-transforms.json` | 53 | `transforms.rs` |
| `core/ui-shortcuts.json` | 843 | `shortcuts.rs` |
| `core/ui-helpers.json` | 11 | `helpers.rs` |

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
(integer-like keys first, then insertion order), which `validateChrome`'s problem list and
`defineChrome`'s first error depend on. `JsMap` is a `Map` and prints as `{"$map": ...}`, which is
what `FitResult.units` and `FitResult.groups` are. `trim` uses the ECMAScript whitespace set, which
differs from `char::is_whitespace` on U+FEFF and U+0085. Lengths are UTF-16 code units, and
`capitalize` leaves a string whose first character is outside the BMP alone, because `charAt(0)`
takes half a surrogate pair. `splice_start` is `splice`'s start index for `addItem`'s `at`, `NaN`
and the infinities included. The solver sums widths in the TypeScript's order, so fractional widths
come out as the same `f64`.

Case folding is Rust's `to_lowercase` and `to_uppercase`. They agree with V8 on every string the
vectors try, final sigma, `İ`, `ß` and `ŉ` included. A character added in a Unicode version one side
has and the other lacks would fold differently.

## Coverage

The generators reach all 1,207 lines of the package and 336 of its 337 branches. The miss is
`shortcuts.ts:65`, the `empty key` throw in `parseShortcut`. An empty last token becomes `+`, and
lowercasing a non-empty string never empties it, so no input reaches it. The Rust keeps the check.

## Mutation testing

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

`chromeHelpers` is a struct of function pointers, `CHROME_HELPERS`, with the member names in
`CHROME_HELPER_KEYS`. The `identity` case asserts that each member is the module's own function,
which the struct satisfies by construction.
