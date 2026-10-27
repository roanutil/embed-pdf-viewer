# epdf-annotation-core

`packages/core/annotation` in Rust: the modules of `src/` and of its `kinds/`, `record/`,
`shapes/` and `update/` directories, plus the module-level exports the package index leaves out
(`endingPoints`, `endingNodes`, `cloudyPolyPath`, `chordThrough`, `rectsIntersect`,
`rectHandlesFor`, `rotatePoint`, `paintOrder`, `groupUnionBounds`, `blendFor`,
`layoutRedactLabel`, `annotsInBox`, `calloutBox`, `calloutUprightRot`, `distanceCaptionWidth`,
`pointInPoly`). The pure part of `@embedpdf/engine-core/runtime` the package calls is ported too,
in `src/engine/` (see below). All 7,505 cases in the 20 vector files replay identically
(`cargo test -p epdf-annotation-core`):

| File | Cases | Covers |
|---|---|---|
| `annotation-geometry-basics.json` | 294 | rect math, rotation helpers, upright placement, `fitStampBox`, group scaling, the rotate knob, guide chords, the SAT test |
| `annotation-geometry-shapes.json` | 3,279 | every per-geom function crossed with 52 shapes (each `Geom` variant rotated and degenerate), borders and stroke widths, and every handle a shape offers plus bogus ones |
| `annotation-geometry-hit.json` | 248 | `geomHit` over probe grids at the margin boundary, every line ending as a target |
| `annotation-geometry-text.json` | 59 | callout connection, caret boxes from text anchors, the PDF/content bridge |
| `annotation-endings.json` | 88 | the ten `/LE` endings at four angles and stroke widths |
| `annotation-cloudy.json` | 354 | cloudy rect, ellipse and polygon paths, intensities -1 to 3 including 0 and fractions |
| `annotation-kinds.json` | 109 | the kind table, caps and prop specs for every subtype, unknown and prototype names |
| `annotation-flags.json` | 650 | the `/F` predicates for every flag, authority split, conversation and substrate planes |
| `annotation-groups.json` | 15 | group keys, members, caps, attached link children |
| `annotation-props.json` | 532 | `readProp` and `applyProps` for 26 annotations and 19 patches, `defaultsFor` |
| `annotation-anchor.json` | 582 | `noZoom`/`noRotate` projection and its inverse across modes and views |
| `annotation-snap.json` | 17 | move snapping guides |
| `annotation-hit.json` | 17 | `hitTest` and `cursorAt` for 19 selections over 35 points on a mixed page, plus 38 edge calls |
| `annotation-scene.json` | 276 | painted scenes per kind, markup, redaction at rest and on hover, label layout |
| `annotation-placement.json` | 194 | click placement, click geometry, ink straightening |
| `annotation-view.json` | 30 | 114 idle, selected and mid-gesture models through every view selector |
| `annotation-update.json` | 112 | 251 message runs through `update`, plus the rotate, callout (page slide included) and marquee helpers |
| `annotation-measurement.json` | 496 | distance layouts across 7 lines and 16 appearances, shape captions, readout labels for 30 `/Measure` dictionaries, selection frames, `textPlateInset`, `geomResetRotation` about a pivot |
| `annotation-richtext.json` | 114 | the run algebra: offsets, splits, styling ranges, normalisation, plain projection |
| `annotation-measurement-model.json` | 39 | 53 runs over measured and rich-text annotations (distance and shape creation, caption and leader drags, `createAnnot`, `setRichText`, bold props), their hit test, scene, props and 25 view states |

A case can carry many calls: `runs`, `viewMany`, `hitCalls`, `hitProbe`, `selectionAnchors` and
`groupProbe` evaluate a list against one page, so a 22-annotation page is written once per batch.
The files come to 54 MB. The three measurement and rich-text files come from
`tooling/plugin-conformance/src/suites/annotation-core-measurement.ts`, which reuses the other
suites' wrappers.

`vectors/coverage.json` was last written at `ec3870988`, before `update.ts`, `kinds.ts` and the
record helpers split into directories, so it still lists the old 23 files. By it the generators
reach 8,335 of 8,344 statements and 2,595 of 2,621 branches. The line numbers below are from that
earlier layout; what the list names can't be reached from the public surface:

- `cloudy.ts` lines 66, 67 (`cosine`/`sine` of a zero hypotenuse; callers skip zero-length
  segments first), 110 to 113 and 132 (an arc of zero span; every arc the curls draw has a fixed
  non-zero span), 232 and 290 to 296 (a zero-length polygon edge; skipped at line 287), 311 (the
  `?? ANGLE_34` fallback; the index is always in range), 347 and 428 (a flattened ellipse with a
  radius at or below zero; the adjusted radii are at least 0.1), 453 (a zero flattened segment).
  Line 359 (two coincident curl centres) is reachable only at zero stroke, where it is a last-bit
  question; see below.
- `geometry.ts` 412 (`?? 0` on a number), 669 (a callout leader of fewer than two points),
  1117 (`rot ?? 0` inside a test that `rot` is set).
- `measurement.ts` 421 (the `return false` for an ending that is neither a poly nor an ellipse;
  `endingNodes` makes no other kind).
- `richtext.ts` 214 (`previous?.style` after the run loop; the loop returns whenever the
  paragraph has a run, so `previous` is always undefined there).
- `update.ts` 1534 (a caret box is at least 1 pt or NaN, never `<= 0`), 1700 (a group union
  always holds its first member).
- `view.ts` 430 (the union is taken over members `sel` already found), 658 (a selectable
  selected id always has bounds on its page).

## Where the port differs in shape

A model is a `Session` plus the records it views, as in the TypeScript. `update(&model, &message)`
returns an `UpdateResult` of the next `session`, a `change` (`put`, `drop`, `patches`) and the
`effects`, or a `CoreError` where the TypeScript throws (`name` is `EngineError`, `TypeError` or
`Error`; an engine error carries its `InvalidArg` code and `details`). The plugin lays `change` on
its records. A session's members are `Rc`s, so `Session::same` answers what `===` on the
TypeScript's object answers, and the vectors' `same` per step compares that.

A record's `annotation` is the engine's annotation as a JSON object (`Rc<serde_json::Map>`, in
insertion order), read through `record::kind_of`, `shape_of`, `style_of` and `text_of` as the
TypeScript reads it. Drafts and patches are JSON objects too. The typed values the core returns
(`ModelAnnotation`, the `Shape` union, drafts, scene nodes, effects) are `js_object!` structs that
remember the order their members were read or written in, so a value prints in the order the
TypeScript's object literal or spread gives it; `src/object.rs` holds the `Json` trait and the
macros. A member that may be absent, `null` or a value is `Nullable`. `ClickCreate` stays a JSON
object, because the TypeScript tests `'length' in policy` and `'width' in policy` and carries
members neither test names.

`src/engine/` is what the core calls from `@embedpdf/engine-core/runtime`, ported without PDFium so
the crate still builds for `wasm32-unknown-unknown`: the kind facts and defaults, the page-space
to file-space mirror, the predictor (`resolveAnnotationDraft`, `resolveAnnotationPatch`, the rect
command, `annotationPatchBetween`, `mergeAnnotationPatch`), `appearanceChangeOf`, and the
measurement readout and format. `src/engine/kind_facts.rs` is generated from the TypeScript tables
by `node_modules/.bin/tsx crates/plugins/annotation-core/scripts/kind-facts.ts | rustfmt --edition
2024 --config-path rustfmt.toml`, run from the repository root. Destinations
(`linkTarget`, `actions`) cross the mirror unchanged; `src/engine/space.rs` says why the result is
the same. `epdf-engine` keeps its own copies on PDFium's side.

`richtext.rs` counts offsets in UTF-16 code units, as the TypeScript does. A rich document's body
and a paragraph's own properties are JSON maps, carried verbatim.

The TypeScript `Vec` is `Point` (and `Vec2`), since a `Vec` would shadow Rust's. Handles are
`&str`, as in the TypeScript, so an unknown handle names no edge exactly where it did before.

Callbacks become data. `selectionAnchor` takes three `&dyn Fn(pon)` lookups; the vector wrapper
passes tables keyed by page object number. `annots_in_box`, `hit_test` and `PointerInput.inert`
take a `js::IdSet`, which reads a canonical `{"$set": [...]}`.

## Not ported

These inputs answer differently on purpose, each pinned by a test in `tests/`:

- A ref of a kind no key is defined for (review finding N6). `annotationKey` keys it `undefined`,
  so every such record would share one key; `record::annotation_key` returns `None`, and
  `record::from_dto` and `irt_of` find nothing for it (`tests/behavior.rs`).
- A name that `Object.prototype` has (`constructor`, `toString`, `__proto__`). The TypeScript
  finds the prototype's member in its kind table and later reads of it throw; `kinds::kind_named`
  reads it as an unknown kind that is only shown. The harness reproduces the TypeScript's answers
  for those names so the vectors still pin them, and `groupResizeAnchor`'s `center` is reproduced
  in the core because it changes only an answer.
- Hostile geometry (review finding M20), where the TypeScript would allocate until it runs out
  or loop forever. A cloudy border with more than a million curls has an empty path and outline,
  and its bounds are the box grown by half the stroke (`tests/hostile_geometry.rs`). A cloud
  outline that would flatten to more than 1,000,000 points, from a hostile `/BE` intensity or
  `/BS` width, is empty (`MAX_OUTLINE_POINTS` in `src/cloudy.rs`, same test file). A squiggly
  underline whose baseline would take more than a million segments draws nothing (`src/scene.rs`
  unit tests).
- Sorting stays V8's TimSort (review finding N5), so an inconsistent comparator leaves elements
  where V8 does (`tests/js_sort.rs`).
- A record id in `forget`'s list or a snap's ids that the view doesn't hold. Snapping skips it
  where the TypeScript reads `byId[id]` unguarded; a gesture whose record is forgotten ends
  (`tests/behavior.rs`). A move, resize or caption draft whose record is missing but not
  forgotten throws the TypeScript's own `TypeError`.
- A `setSnap` patch the typed `SnapSettings` can't hold (`alignment: "yes"`): the TypeScript
  spreads it in; the port returns the model unchanged (`tests/behavior.rs`).
- A malformed `/Measure` (`src/engine/measure.rs`), read as `epdf-engine`'s copy reads it since
  Task 14.3 (`tests/engine_cross_check.rs` runs both on the same inputs). A number or string in a
  format list is a format with no members, as the TypeScript reads it; a falsy measure is none and
  a falsy `y` is no `y`. Kept: a `null` entry is the same empty format where
  `validNumberFormat(null)` throws; a missing `x`, `distance` or `area` list reads as empty (no
  scale, or no format) where the TypeScript throws a `TypeError`; a missing `unit` prints `""`
  where the TypeScript prints `"undefined"`; a member of the wrong type (`conversion: "2"`) reads
  as absent where `Math.fround` coerces it; a `fraction` that isn't a string reads as `decimal`.
  No vector holds one.
- `setRichText` on a free-text annotation with no `richText`: the TypeScript reads
  `richText.body` and throws a `TypeError`; the port takes the body as empty. No vector holds one.
- A rich-text cut inside a surrogate pair. `slice` keeps the lone surrogate; a Rust string cannot,
  so the half becomes U+FFFD.
- `rangeHasStyle` over a non-empty range of a document with no paragraphs reads `paragraphs[0]`
  and throws; the port answers `false`.
- A `null` member in a rich-text style delta. `cleanDelta` keeps it; the typed delta reads it as
  absent.

## JavaScript numerics

`src/js.rs` re-exports `epdf_geometry::js::to_fixed` (`Number#toFixed`) and adds `fixed`
(`Number(x.toFixed(d))`), `string_to_number` (`Number(string)`, for `Number(handle.slice(1))`,
which takes `" 1"`, `"0x1"` and `""` as numbers) and `utf16_len`. Everything else comes from
`epdf_geometry::js`.

`Math.sin`, `cos`, `atan2` and `acos` are Rust's `f64` methods, which call the platform libm; V8
uses fdlibm. They can differ in the last bit, which the 1e-12 tolerance absorbs, except where a
comparison sits exactly on that bit. Two such places exist, and the vectors stay off both: a
cloudy ellipse at zero stroke places its last curl by comparing an accumulated perimeter with the
curl advance at zero tolerance, and `straightenInkStroke` compares an `atan2` angle with the snap
threshold (the cases sit at 17.6 and 17.7 rather than on `100 * tan(10°)`).

## Mutation testing

These counts are from the port before the re-port to the current `packages/core/annotation`
layout, and the line numbers point into it: `measure.rs` and `update.rs` are gone (their code is
now in `src/engine/` and `src/update/`), and `tests/api.rs` with them. The table is stale and the
run has not been repeated on the current modules; the in-diff run over Task 14's changed lines (`4a0b9c126..172f5369d`, 2026-10-27) caught 4 of 4 viable mutants (3 unviable).

`scripts/mutants.sh -p epdf-annotation-core --test-package epdf-annotation-core --file <file>`,
file by file over every module but `harness.rs`, catches 4,577 of 4,669 viable mutants (4,556
caught, 21 timeouts; 213 unviable). `lib.rs` only re-exports and has no mutants. The run itself, at
`afb842b48`, caught 4,458 and missed 190. Cases added afterwards kill 98 of those 190, and the
counts above include them: 93 through new vectors, 5 through unit tests in `tests/api.rs` for
`PageRef::same_page` and `MeasurementAppearance::is_polygon`, which nothing in the crate calls. Each
was confirmed by applying the mutant by hand and watching `cargo test` fail, not by a second run.
The other 92 are equivalent or unreachable, each listed below. Timeouts count as caught: each is a
mutant that loops forever.

| File | Caught | Timeouts | Missed | Unviable | Killed after | Accepted |
|---|---|---|---|---|---|---|
| `anchor.rs` | 72 | 0 | 0 | 5 | | |
| `cloudy.rs` | 639 | 18 | 32 | 1 | | 32 |
| `endings.rs` | 66 | 0 | 1 | 2 | | 1 |
| `flags.rs` | 47 | 0 | 0 | 2 | | |
| `geometry.rs` | 1,180 | 0 | 14 | 21 | | 14 |
| `group.rs` | 18 | 0 | 0 | 1 | | |
| `hit.rs` | 97 | 0 | 9 | 7 | 8 | 1 |
| `ink.rs` | 45 | 0 | 0 | 0 | | |
| `js.rs` | 149 | 0 | 6 | 11 | | 6 |
| `kinds.rs` | 33 | 0 | 0 | 17 | | |
| `links.rs` | 5 | 0 | 0 | 2 | | |
| `measure.rs` | 243 | 0 | 41 | 8 | 24 | 17 |
| `measurement.rs` | 258 | 0 | 18 | 12 | 14 | 4 |
| `measurement_shape.rs` | 184 | 0 | 36 | 7 | 35 | 1 |
| `placement.rs` | 63 | 0 | 0 | 2 | | |
| `plane.rs` | 35 | 0 | 0 | 3 | | |
| `props.rs` | 41 | 0 | 0 | 23 | | |
| `richtext.rs` | 142 | 0 | 3 | 6 | 1 | 2 |
| `scene.rs` | 225 | 3 | 1 | 5 | | 1 |
| `selection.rs` | 17 | 0 | 0 | 1 | | |
| `snap.rs` | 62 | 0 | 0 | 3 | | |
| `types.rs` | 64 | 0 | 7 | 9 | 4 | 3 |
| `update.rs` | 593 | 0 | 17 | 38 | 10 | 7 |
| `view.rs` | 180 | 0 | 5 | 27 | 2 | 3 |
| Total | 4,458 | 21 | 190 | 213 | 98 | 92 |

### Accepted misses

`cloudy.rs` (the path strings print four decimals, so a last-bit difference in an angle never
shows):

| Line | Mutant | Why no case kills it |
|---|---|---|
| 122 | eight arithmetic mutants in `arc_segment`'s zero-span `move_to` | Unreachable. Only `add_corner_curl` passes `add_move_to`, and its arc spans a fixed -22°. |
| 169 | `<` to `<=` on `angle_todo` in `get_arc` | Unreachable. A zero sweep needs `b` to equal `angle_cur - alpha` to the last bit, which only the platform `atan2` and `acos` could arrange. |
| 175 | `*` to `/` and `+` to `-` in `get_arc`'s `move_to` | Equivalent. Only the plain ellipse moves there, from angle 0: `cos 0` is 1 and `sin 0` is 0. |
| 177 | `>` to `>=` on `ANGLE_90` in `get_arc` | Equivalent for the full circle, whose segments end at the same angles either way; a corner curl's sweep never lands on a quarter turn to the last bit. |
| 182 | `>` to `>=` on the last segment | Equivalent. A zero-span `arc_segment` returns before drawing. |
| 213, 220 | `+` to `-` on `ANGLE_180` | Equivalent. The angles differ by a full turn. |
| 229 | `<` to `<=` in `output_curl_template` | Equivalent. A template's length is a multiple of 3. |
| 253, 318 | `-1` to `1` in `compute_params_polygon`, and `!started` to `started` | Unreachable. Both belong to a zero-length edge, which `cloudy_polygon_impl` skips first (TypeScript lines 232 and 290 to 296, above). |
| 290 | `<` to `<=` on the 0.5 radius floor | Equivalent. At 0.5 the floor assigns 0.5. |
| 382 | `\|\|` to `&&` in `flatten_ellipse` | Unreachable. The adjusted radii are at least 0.1 (TypeScript line 347, above). |
| 403 | `<` to `<=` and `==` on -1, `>` to `>=` on 1 in `compute_params_ellipse` | Equivalent. The argument is a length over a radius, never negative, and at exactly 1 `acos` gives the same 0. |
| 450, 458 | `>` to `>=` on `2 * radius_adj` | Unreachable without a last-bit comparison: `radius_adj` is `sin(12°)` times the radius, so the equality would be decided by the platform `sin`, which the vectors stay off (see "JavaScript numerics"). |
| 468 | `<` to `<=` and `==` on `flat.len()` | Unreachable. A flattened ellipse has at least 33 points; the empty case needs a radius at or below zero. |
| 484, 487 | `<` to `<=` on the 0.5 and 3.0 radius thresholds | Unreachable without a last-bit comparison: the radius comes from a perimeter summed from platform `sin` and `cos`. |
| 502 | `==` to `!=`, `-` to `+` and `/` in `i == flat.len() - 2` | Equivalent. That clause only sends a short last segment into the branch that places curls, where it places none and sets `remain`, which nothing reads after the loop. |

`geometry.rs`:

| Line | Mutant | Why no case kills it |
|---|---|---|
| 139 | delete the `"se"` arm of `opposite_handle` | Equivalent. `group_resize_anchor` falls back to `nw`, which is `se`'s opposite anyway, and in `resize_rotated_rect` the anchor cancels: the new centre is `c0 + R(next_centre - c0)` whatever the anchor is. |
| 467 | `\|\|` to `&&` in `obb_from_geom` | Equivalent. The two differ only at `rot` 0, where `geom_rotate_about` by `-0` returns the geom unchanged, and at `NaN`, where the final rotation makes every corner `NaN` either way. |
| 758 | `>` to `>=` on the baseline length in `caret_geom_from_anchor` | Equivalent. A zero length means `bx` and `by` are zeros, and `atan2` of zeros gives the 0 the other arm returns. Only a `-0` `bx` gives 180, and canonical JSON has no `-0`. |
| 763 | `<` to `<=` against `CARET_ROT_EPSILON` | Unreachable. `normalize_deg` returns a value near 0.05 as `(x + 360) - 360`, a multiple of 2^-44, and 0.05 isn't one. The upper edge, `> 360 - 0.05`, is reachable and killed (`rot-at-the-upper-edge`). |
| 841 | `<` to `<=` and to `==` on the join's direction change in `stroke_outline_points` | Equivalent. The points only reach `union_rect` in `geom_visual_bounds`. A join that turns by less than 1e-9 is straight to within `h * theta^2 / 8`, so its miter lands on corners already in the set. |
| 847 | `<` to `<=` on the bisector length | Unreachable. Near a reversal that length is rounding noise of order 1e-16 over the angle, so it never lands on exactly 1e-9. |
| 852 | `<` to `<=` on the bisector orientation test | Unreachable. The dot product is exactly 0 only when the two directions are parallel, and both parallel cases are skipped above it. |
| 857 | `<` to `<=` on the half-angle cosine | Unreachable, for the reason 847 is; only a negative stroke width could observe it. The `==` mutant on the same line is killed (`reversal-negative-stroke`). |
| 962 | the `*rot != 0.0` guard in `selection_bounds` to `true` | Equivalent. `rotated_aabb(rect, 0)` returns `rect`, which is what `geom_bounds` gives a text box. |
| 1032 | `\|\|` to `&&` on the ellipse radii in `ending_nodes_hit` | Equivalent. The only ellipse node is the circle ending, placed with rotation 0 from `(-r, -r)` and `(r, r)`, so its radii are always equal. |
| 1048, 1166 | `>` to `>=` on `len > 2` before a closing edge | Equivalent. With two points the closing edge is the first segment, already tested. |
| 1192 | `&&` to `\|\|` on `rot` in `geom_handles` | Unreachable by a vector. At `rot` 0 the mutant rotates by 0, which is exact. Only a `NaN` `rot` tells them apart, and every consumer carries the handle's cursor, where the port already differs for a non-finite `rot` (see "Not ported"). |

The other files:

| Line | Mutant | Why no case kills it |
|---|---|---|
| `endings.rs` 42 | `a + PI` to `a - PI` for the reversed endings | Equivalent. The two angles are a full turn apart. |
| `hit.rs` 245 | `selected.len() > 1` to `>=` in `hit_test` | Equivalent. A single selection only reaches the group branch when it isn't selectable, and then it is either not interactive (so not transformable) or of the one kind whose caps say unselectable, whose group caps are all false: the branch returns nothing. |
| `js.rs` 23 | `-1074` to `1074`, and `\|` to `^`, in `decompose` | Equivalent. A subnormal times `10^digits` is never an exact tie either way, and the fraction never has bit 52 set. |
| `js.rs` 34 | the `s > 0` guard in `exact_tie` to `false` | Equivalent. The last arm answers `false` for a positive shift too, since `(-s) as u32` wraps far past any trailing-zero count. |
| `js.rs` 323 | `>` to `>=` in `Record::insert` | Equivalent. Two keys with the same array index are the same key, and that case returns earlier. |
| `js.rs` 362, 423 | the `Record` and `IdSet` visitors' `expecting` to `Ok(())` | Unobservable. It only writes a decode error message, and no vector holds a malformed record or set. |
| `scene.rs` 205 | outer `nodes.len() < 400` to `<=` in `layout_redact_label` | Equivalent. The inner loop's own `< 400` guard stops every push. |
| `types.rs` 689 | delete `!` in `AnnotationPropsPatch::spread`'s key order | Unobservable here. The spread's order only reaches `set_defaults`, whose output is a JSON object (member order isn't compared, and a repeated key collapses on decode); the `props` effect's key list comes from the message's own patch. |
| `types.rs` 732 | `PatchVisitor::expecting` to `Ok(())` | Unobservable, as for `js.rs` 362. |
| `types.rs` 972 | the `is_segment()` guard in `segment_policy` to `true` | Equivalent. Its one caller is the line draft, and `click_create_geom` gives a line nothing from a box placement, just as it gets nothing without a policy. |
| `update.rs` 266 | `same()` to `Default::default()` | Equivalent. `(None, Vec::new())` is the default. |
| `update.rs` 769 | `delta.abs() < 0.01` to `<=` | Unreachable without a last-bit comparison. The delta is a difference of `atan2` angles; a snapped delta from a still pointer is `arc_to`, which is `x - 180` for an `x` near 180 and so a multiple of 2^-45, never 0.01. |
| `update.rs` 789 | both `< 1e-4` to `<=` on the group factors | Unreachable. `sx - 1` is a multiple of `ulp(sx)`, at least 2^-53 near 1, and 1e-4 isn't one. |
| `update.rs` 1247 | the `dr != 0.0 && !dr.is_nan()` guard to `true`, and `&&` to `\|\|` | Equivalent. A `dr` of 0 gives an upright rotation of 0, and a `NaN` one gives `NaN`, which `upright_box` and `rot_of` both treat as 0. |
| `update.rs` 1677 | `\|\|` to `&&` on the caret box | Equivalent. The caret box is square. |
| `view.rs` 118 | the `mode.zoom` guard in `eff_style` to `true` | Equivalent. Without `zoom` the anchor scale is 1, so the stroke divides by 1. |
| `view.rs` 479, 591 | `sel.len() > 1` to `>=` | Equivalent. Both follow a `sel.len() == 1` branch. |

`measure.rs`. The boundary tests compare cross products with 0. For `fround` coordinates of similar
magnitude those products are exact, and the arguments below assume so; a counterexample would need
a rounding coincidence.

| Line | Mutant | Why no case kills it |
|---|---|---|
| 221 | `+=` to `-=` in `polygon_area` | Equivalent. Negating every term negates the sum, and the area is its absolute value. |
| 230 | `>` to `>=` on `boundary.len()` | Equivalent. A one-point boundary pops to empty instead of staying at one point, and both fail the `< 3` test. |
| 233 | `\|\|` to `&&` on the size and area test | Equivalent. Fewer than three points still have zero area and return. A zero-area boundary of three or more points is collinear or self-intersecting, and the fold, touch and crossing tests below reject every such boundary. |
| 254 | `*` to `/` on either product, `+` to `*` between them, `<` to `==` and `<=` on `doubles_back` | Equivalent. The test only counts where `cross(a, b, next)` is 0. Straight through, both steps point the same way: each axis ratio is positive or `NaN`, both products are at least 0, so none of these makes the dot negative, and the dot is 0 only at a repeated vertex, which `same` rejects on the next edge. A real fold also puts a vertex on a non-adjacent edge for four or more points, which `on_segment` rejects, and a folded triangle has zero area. The other 11 mutants on the line are killed (`area-straight-*`). |
| 265, 266, 267 | each `\|\|` to `&&` in the `on_segment` chain | Equivalent. Unless all four points are collinear, any endpoint lying on the other segment also makes the sign test below report a crossing. Two collinear segments that meet have both endpoints of one inside the other, or one endpoint of each inside the other, and each mutated chain still holds for those. |
| 370 | both `\|\|` to `&&` in `format_measurement` | Equivalent. Both callers have already rejected an empty or invalid format list, and an infinite value fails the loop's own `is_finite` check on the first format. |
| 429, 430, 431 | `is_dimension` to `true`, and each `&&` to `\|\|` | Unreachable. The only caller is `measurement_readout`, and the port always passes it a matching pair: `line` and `LineDimension` from `distance_label`, `polygon` and `PolygonDimension` or `polyline` and `PolyLineDimension` from `shape_measurement_readout`. |

The other new survivors:

| Line | Mutant | Why no case kills it |
|---|---|---|
| `measurement.rs` 369 | `*` to `+` in `2.0 * CAPTION_PADDING` | Equivalent. 2 + 2 is 2 * 2. |
| `measurement.rs` 372 | `>` to `>=` on `along.y` | Equivalent. `along` is a unit vector, `(1, 0)` for a zero-length line, so where `along.x` is 0 `along.y` is 1 or -1. |
| `measurement.rs` 425, 426 | `angle + pi` to `angle - pi` | Equivalent, as for `endings.rs` 42: the angles are a full turn apart. |
| `measurement_shape.rs` 211 | `<` to `<=` in the widest-span loop | Unreachable. It would read past the end only for an odd number of intersections, and a closed ring crosses the middle height an even number of times: each vertex's above-or-below flag is read once, and round the cycle it changes an even number of times. |
| `richtext.rs` 163 | the `a.len() == b.len()` guard in `same_decoration` to `true` | Equivalent. Sorted lists of different lengths are unequal anyway. |
| `richtext.rs` 325 | `>` to `>=` in `style_at` | Equivalent. At a boundary past 0 the last non-empty run before it ends there and has already matched; if every earlier run is empty, the boundary is 0 and the next test returns the same run. |
