# epdf-stage-core

`packages/core/stage` in Rust: the camera, the two layouts and their spatial index, anchors, scroll
math and zoom intent. All 300 cases in the three vector files replay identically
(`cargo test -p epdf-stage-core`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/stage-camera.json` | 165 | projection, `zoomAround`, `panByScreen`, `centerOnWorld`, `travelRange`, `clampCamera`, `scrollMetrics`, `cameraFromScroll`, `placeCamera`, `revealCamera`, `resolveZoom`, the constants |
| `vectors/core/stage-layout.json` | 103 | `groupPages`, `linearLayout`, `gridLayout`, and each scene's `query`, `nearestItem` and `itemOfPage` |
| `vectors/core/stage-anchor.json` | 32 | `anchorAtPoint`, `cameraForAnchorAtScreen`, `anchorFromCamera`, `cameraFromAnchor` |

The generators reach every statement (919/919) and every branch (195/195) of the TypeScript.

`Point`, `Size`, `PageRotation`, `PageFrame` and `NO_FRAME` are re-exported from `epdf-geometry`, as
the TypeScript re-exports them. The TypeScript's own `Rect` is structurally the geometry `Rect`, so
the crate uses that one too rather than a second type with the same four fields.

## Where the port differs in shape

A TypeScript `Scene` carries three closures. `Scene` here holds the index they close over and
exposes them as `query`, `nearest_item` and `item_of_page`; it serializes to its data members. The
vector entries are wrappers in `tooling/plugin-conformance/src/suites/stage.ts` that build the
scene and run each query on probes, reporting items by index, and `src/harness.rs` mirrors them.
`nearest_item` returns `None` where the TypeScript returns `undefined` (an empty scene, a NaN
coordinate on a grid), and `item_of_page` returns `None` where it returns `-1` (an empty scene).

The TypeScript throws in four places, and the port returns `Err(StageError)` with the same name and
message: a grouping that names a missing page (`TypeError`, reading `'userUnit'`), an anchor on an
empty scene (reading `'pages'`) or on an item with no pages (reading `'pageIndex'` or `'x'`), and a
grid whose column count comes out NaN (`RangeError: Invalid array length`). The cost is a `Result`
on `linear_layout`, `grid_layout` and the four anchor functions, all of which succeed on any input
a viewer builds itself.

The empty scene the stage plugin lays out instead of those throws (review findings M27 and R5) is
not here. It lives in `crates/plugins/stage/src/services/scene.rs`, which turns an `Err` from
either layout into `linear_layout(&[], &[], ..)`. The `edge_inputs` tests in
`tests/conformance.rs` pin what that fallback needs from this crate: an empty layout succeeds, and
a missing page or a NaN column count fails without a panic. They also pin the TypeScript's answers
for zero, NaN and infinite viewports, zooms and column counts.

Default parameters become explicit arguments: `place_camera` takes `padding`, `align` and
`direction` (`0`, `AlignmentValue::default()`, `Direction::Ltr` in the TypeScript), and
`reveal_camera` and `resolve_zoom` take `padding` (`0`).

## Not ported

Page counts and page indexes are `usize`. The TypeScript accepts any number: `groupPages(2.5)`
gives three groups, `itemOfPage(-1)` answers `0`, `cameraForAnchorAtScreen` with a NaN page index
falls back to the first item's first page. None of those inputs can be written in Rust, and no
vector carries them.

A grid with a fractional `columns` lays out exactly as the TypeScript does, but its `query` differs:
cells that index between items make the TypeScript push `undefined` into the result, and the port
leaves them out. The suite's wrapper drops those holes before writing the vector.

`null` and `undefined` are one thing here (`None`). Three options read them differently in the
TypeScript, and no vector passes `null` for them: `viewUnitsPerPoint: null` scales every page by
`0` (a default parameter only replaces `undefined`), `lineWidth: null` derives the column count
from `gap / (cell + gap)`, and `cameraFromScroll`'s `left: null` scrolls to `0` instead of leaving
the axis.

A `ZoomSpec` whose `mode` is not one of the four `ZoomMode` values takes the TypeScript's
`default:` arm, which is the automatic zoom. `ZoomMode` is a closed enum here, so that input does
not decode.

## Mutation testing

`cargo mutants -p epdf-stage-core --test-package epdf-stage-core` tests 544 mutants: 518 caught, 11
caught by timeout (a binary search or a grid loop that no longer terminates), 5 missed, 10
unviable. Every miss is equivalent:

- `lib.rs:629` and `lib.rs:662`, `rows - 1` to `rows + 1` and to `rows / 1` in the grid's `query`
  and `nearest_item`. Both bound `upper_bound(row_top, y) - 1`, and `upper_bound` never exceeds
  `rows`, so the bound only ever meets a value it already allows.
- `lib.rs:711`, `>` to `>=` in `measure_item`. An equal height sets `h` to the value it holds (a
  `-0` page height included: every use of `h` compares it or folds it with `max` against `0`).
