# epdf-annotation-core

`packages/core/annotation` in Rust: every exported function, type and constant of its 17 source
files, plus the module-level exports the package index leaves out (`endingPoints`,
`endingNodes`, `cloudyPolyPath`, `chordThrough`, `apSizeChanged`, `rectsIntersect`,
`rectHandlesFor`, `rotatePoint`, `paintOrder`, `groupUnionBounds`, `blendFor`,
`layoutRedactLabel`, `kindTakesLink`, `annotsInBox`, `calloutBox`, `calloutUprightRot`). All
6,996 cases in the 17 vector files replay identically (`cargo test -p epdf-annotation-core`):

| File | Cases | Covers |
|---|---|---|
| `annotation-geometry-basics.json` | 331 | rect math, rotation helpers, upright placement, `fitStampBox`, group scaling, the rotate knob, guide chords, the SAT test |
| `annotation-geometry-shapes.json` | 3,333 | every per-geom function crossed with 52 shapes (each `Geom` variant rotated and degenerate), borders and stroke widths, and every handle a shape offers plus bogus ones |
| `annotation-geometry-hit.json` | 248 | `geomHit` over probe grids at the margin boundary, every line ending as a target |
| `annotation-geometry-text.json` | 95 | callout connection, caret boxes from text anchors, the PDF/content bridge |
| `annotation-endings.json` | 88 | the ten `/LE` endings at four angles and stroke widths |
| `annotation-cloudy.json` | 343 | cloudy rect, ellipse and polygon paths, intensities -1 to 3 including 0 and fractions |
| `annotation-kinds.json` | 137 | the kind table, caps and prop specs for every subtype, unknown and prototype names |
| `annotation-flags.json` | 650 | the `/F` predicates for every flag, authority split, conversation and substrate planes |
| `annotation-groups.json` | 15 | group keys, members, caps, attached link children |
| `annotation-props.json` | 533 | `readProp` and `applyProps` for 26 annotations and 19 patches, `defaultsFor` |
| `annotation-anchor.json` | 582 | `noZoom`/`noRotate` projection and its inverse across modes and views |
| `annotation-snap.json` | 17 | move snapping guides |
| `annotation-hit.json` | 17 | `hitTest` and `cursorAt` for 19 selections over 35 points on a mixed page, plus 38 edge calls |
| `annotation-scene.json` | 274 | painted scenes per kind, markup, redaction at rest and on hover, label layout |
| `annotation-placement.json` | 194 | click placement, click geometry, ink straightening |
| `annotation-view.json` | 30 | 114 idle, selected and mid-gesture models through every view selector |
| `annotation-update.json` | 109 | 251 message runs through `update`, plus the rotate, callout and marquee helpers |

A case can carry many calls: `runs`, `viewMany`, `hitCalls`, `hitProbe`, `selectionAnchors` and
`groupProbe` evaluate a list against one page, so a 22-annotation page is written once per batch.
The files come to 15 MB.

The generators reach 6,447 of 6,456 statements and 2,017 of 2,035 branches of the package
(`vectors/coverage.json`). What they don't reach can't be reached from the public surface:

- `cloudy.ts` lines 66, 67 (`cosine`/`sine` of a zero hypotenuse; callers skip zero-length
  segments first), 110 to 113 and 132 (an arc of zero span; every arc the curls draw has a fixed
  non-zero span), 232 and 290 to 296 (a zero-length polygon edge; skipped at line 287), 311 (the
  `?? ANGLE_34` fallback; the index is always in range), 347 and 428 (a flattened ellipse with a
  radius at or below zero; the adjusted radii are at least 0.1), 453 (a zero flattened segment).
  Line 359 (two coincident curl centres) is reachable only at zero stroke, where it is a last-bit
  question; see below.
- `geometry.ts` 411 (`?? 0` on a number), 668 (a callout leader of fewer than two points),
  1118 (`rot ?? 0` inside a test that `rot` is set).
- `update.ts` 1195 (a caret box is at least 1 pt or NaN, never `<= 0`), 1361 (a group union
  always holds its first member).
- `view.ts` 358 (the union is taken over members `sel` already found), 571 (a selectable
  selected id always has bounds on its page).

## Where the port differs in shape

`update(m, msg)` returns `(Option<Model>, Vec<Effect>)`. `None` is the TypeScript handing back
the same model object, which a store takes as "no change"; `Some` is a new model even when it
equals the old one, as `{ ...m, draft: null }` is. The vectors record which (`same` per step).
`apply` gives the model either way.

The TypeScript `Vec` is `Point` (and `Vec2`), since a `Vec` would shadow Rust's. Handles are
`&str`, as in the TypeScript, so an unknown handle names no edge exactly where it did before.

`Annot.data` is the engine `AnnotationDTO` as a `serde_json::Value`: the model reads five members
of it (`subtype`, `state`, `stateModel`, `contents`, and a link child's `target`) and carries the
rest. `AnnotationRef`, `PdfLinkTarget`, `PdfDestination`, `AnnotationFlags` and the enums
(`LineEnding`, `BlendMode`, `Intent`) are typed serde mirrors of `@embedpdf/engine-core/runtime`.

A member that may be absent, `null` or a value (`interiorColor` in a patch, `link`) is
`js::Nullable`. `AnnotationPropsPatch` has a hand-written `Deserialize` that records the order its
keys arrived in and keeps members no key names in `extra`, because the `props` patch effect
reports `Object.keys(patch)` verbatim and `defaultsFor` spreads unknown members into the bag.
`ClickCreate` keeps all five members optional: the TypeScript tells a box from a segment by
`'length' in policy` in one place and `'width' in policy` in two others, and an object with both
answers differently to each.

Callbacks become data. `selectionAnchor` takes three `&dyn Fn(pon)` lookups; the vector wrapper
passes tables keyed by page object number. `annots_in_box`, `hit_test` and `PointerInput.inert`
take a `js::IdSet`, which reads a canonical `{"$set": [...]}`.

## Not ported

These inputs answer differently, and no vector feeds them:

- Unknown enum strings. `LineEnding`, `BlendMode`, `Border.kind`, a `PdfLinkTarget` that doesn't
  match its union fail to decode; the TypeScript takes any string (`endingNodes` with an unknown
  ending returns `[]`), and `linkOf` returns an unparseable child target verbatim where the Rust
  returns `None`.
- A `null` for a member the type says is a string or number (`color: null` in a patch).
- An id or subtype that names an `Object.prototype` member (`constructor`, `toString`). The
  TypeScript finds a function there; where that only changes an answer (`groupResizeAnchor`), the
  port reproduces it, and where the TypeScript then throws (`m.byId['constructor'].flags`), the
  port doesn't try.
- A missing id in a move draft: snapping and the move commit read `byId[id]` unguarded and throw a
  `TypeError`; the Rust panics at the same read.
- `geomEqual` is `JSON.stringify(a) === JSON.stringify(b)`, which is member-order sensitive. The
  Rust compares values, so a caller-built handle draft whose `base` lists its keys in another
  order than `geomDragHandle` writes them counts as unchanged here and changed there.
- `setText` on a DTO that is a string (the spread copies its characters).
- A non-finite `rot` on a box: `rotatedHandleCursor` reads past its table and the handle has no
  `cursor`; here the cursor is `""`.

## JavaScript numerics

`src/js.rs` adds `to_fixed` (`Number#toFixed`: the exact decimal rounded half up, where Rust's
`{:.N}` breaks exact ties to even, so `(0.03125).toFixed(4)` is `0.0313`), `fixed`
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

In progress. Each file runs on its own (`scripts/mutants.sh -p epdf-annotation-core --test-package
epdf-annotation-core --file <file>`); `harness.rs` is excluded and `lib.rs` holds re-exports only.
A miss is either killed by a new case or listed below with the reason it can't be. "Killed after"
counts the misses a later case kills, each confirmed by applying the mutant by hand and watching the
replay fail.

| File | Caught | Timeouts | Missed | Unviable | Killed after | Accepted |
|---|---|---|---|---|---|---|
| `geometry.rs` | 1,156 | 0 | 35 | 20 | 21 | 14 |

Timeouts count as caught: each is a mutant that loops forever.

The 17 files the first pass completed (1,615 caught, 3 timeouts, 30 missed, 145 unviable, before
`84f73b5ee` and `90be0fb5a` added cases) and `cloudy.rs` are being re-run.

### Accepted misses

`geometry.rs`:

| Line | Mutant | Why no case kills it |
|---|---|---|
| 139 | delete the `"se"` arm of `opposite_handle` | Equivalent. `group_resize_anchor` falls back to `nw`, which is `se`'s opposite anyway, and in `resize_rotated_rect` the anchor cancels: the new centre is `c0 + R(next_centre - c0)` whatever the anchor is. |
| 466 | `\|\|` to `&&` in `obb_from_geom` | Equivalent. The two differ only at `rot` 0, where `geom_rotate_about` by `-0` returns the geom unchanged, and at `NaN`, where the final rotation makes every corner `NaN` either way. |
| 757 | `>` to `>=` on the baseline length in `caret_geom_from_anchor` | Equivalent. A zero length means `bx` and `by` are zeros, and `atan2` of zeros gives the 0 the other arm returns. Only a `-0` `bx` gives 180, and canonical JSON has no `-0`. |
| 762 | `<` to `<=` against `CARET_ROT_EPSILON` | Unreachable. `normalize_deg` returns a value near 0.05 as `(x + 360) - 360`, a multiple of 2^-44, and 0.05 isn't one. The upper edge, `> 360 - 0.05`, is reachable and killed (`rot-at-the-upper-edge`). |
| 840 | `<` to `<=` and to `==` on the join's direction change in `stroke_outline_points` | Equivalent. The points only reach `union_rect` in `geom_visual_bounds`. A join that turns by less than 1e-9 is straight to within `h * theta^2 / 8`, so its miter lands on corners already in the set. |
| 846 | `<` to `<=` on the bisector length | Unreachable. Near a reversal that length is rounding noise of order 1e-16 over the angle, so it never lands on exactly 1e-9. |
| 851 | `<` to `<=` on the bisector orientation test | Unreachable. The dot product is exactly 0 only when the two directions are parallel, and both parallel cases are skipped above it. |
| 856 | `<` to `<=` on the half-angle cosine | Unreachable, for the reason 846 is; only a negative stroke width could observe it. The `==` mutant on the same line is killed (`reversal-negative-stroke`). |
| 961 | the `*rot != 0.0` guard in `selection_bounds` to `true` | Equivalent. `rotated_aabb(rect, 0)` returns `rect`, which is what `geom_bounds` gives a text box. |
| 1031 | `\|\|` to `&&` on the ellipse radii in `ending_nodes_hit` | Equivalent. The only ellipse node is the circle ending, placed with rotation 0 from `(-r, -r)` and `(r, r)`, so its radii are always equal. |
| 1047, 1165 | `>` to `>=` on `len > 2` before a closing edge | Equivalent. With two points the closing edge is the first segment, already tested. |
| 1191 | `&&` to `\|\|` on `rot` in `geom_handles` | Unreachable by a vector. At `rot` 0 the mutant rotates by 0, which is exact. Only a `NaN` `rot` tells them apart, and every consumer carries the handle's cursor, where the port already differs for a non-finite `rot` (see "Not ported"). |
