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

Incomplete. The run went file by file (`scripts/mutants.sh --file`) and stopped before it
finished, so this is what 18 of the crate's 20 source files show (`harness.rs` is excluded),
not the crate:

| Files | Caught | Timeouts | Missed | Unviable |
|---|---|---|---|---|
| 17 complete: `anchor`, `endings`, `flags`, `group`, `hit`, `ink`, `js`, `kinds`, `links`, `placement`, `plane`, `props`, `scene`, `snap`, `types`, `update`, `view` | 1,615 | 3 | 30 | 145 |
| `cloudy.rs`, cut off partway | 411 | 12 | 19 | 0 |
| `geometry.rs` | not run | | | |

Timeouts count as caught: each is a mutant that loops forever. `lib.rs`, the other file not run,
holds re-exports only.

The 49 misses have not been triaged one by one. Two commits after the first pass
(`84f73b5ee`, `90be0fb5a`) added cases for the ones found then, so some of these may already be
dead: the counts come from runs that started before those cases landed. `geometry.rs` is the
largest file and the one most likely to hide survivors; it is the next run to do.
