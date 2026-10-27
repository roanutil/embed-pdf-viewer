# epdf-geometry

`packages/core/geometry` in Rust: every exported function, type and constant of `src/index.ts` and
`src/page-space.ts`. It also carries the pure part of `engine-core`'s geometry the viewer side calls:
the six page-space conversions in `packages/engine/core/src/geometry/pageSpace.ts`, and
`pdfQuadBounds` and `normalizePdfQuad` from `convert.ts`. All 648 cases in the four vector files
replay identically (`cargo test -p epdf-geometry`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/geometry-basics.json` | 140 | `boundsOfRects`, `intersectRects`, `rectFromCorners`, `rectContains`, `rectsOverlap`, rotations, `snapToDevice`, `displaySize`, `deviceHeightForWidth`, `NO_FRAME` |
| `vectors/core/geometry-affine.json` | 110 | `Mat2D` compose, invert, apply*, the builders, `angleOf`, `matrixToCss` |
| `vectors/core/geometry-page.json` | 118 | `rotateScaleMatrix`, `pageTransform`, the `engine-core` page-space conversions, `rectsOverlap` on flipped edges, `pdfQuadBounds` |
| `vectors/core/geometry-text-quad.json` | 280 | the named-corner quad helpers and `normalizePdfQuad` |

The generator (`tooling/plugin-conformance/src/suites/geometry.ts`) keeps some historical case
labels as function names: `textQuadEdge` calls `quadEdge`, `applyTextQuad` calls `applyQuad`, and
the old `pageGeometry` cases call `pageTransform`. `src/harness.rs` answers exactly the names the
vectors call, and `tests/conformance.rs` checks that no retired name is left in the table.
`tests/behavior.rs` covers what the vectors only sample: singular transforms refuse, and rotation,
crop and `userUnit` round trips give back the coordinates they started from.

## Where the port differs in shape

`PageRotation` is a newtype over `f64`, not an enum. The TypeScript union `0 | 90 | 180 | 270`
isn't checked at runtime, and every function gives other numbers an answer: `addRotations(-90, 0)`
is `-90`, `isQuarterTurn(45)` is false, `rotateScaleMatrix` treats 45 as 0. The vectors pin those
answers, so the type keeps the number. `PageRotation::DEG_0` through `DEG_270` are the four values
the type means.

`pageTransform` returns an object of closures in TypeScript. Here `PageTransform` holds the four
matrices it builds and exposes the closures as methods (`to_pixels`, `page_to_view_rect`, ...); it
serializes to the data members only. The vector entry is a wrapper that calls each method on probe
points and rects. Its result keys still say `contentToView` and `viewToContent`, the names the
methods had before 81bdc82ec renamed content space to page space. `samePagePlacement` takes two
`&PageTransform` where the TypeScript takes any object with the five members it compares.

The `engine-core` helpers live here because `epdf-engine`, which ports them for the native engine,
links PDFium and cannot build for wasm32, where the plugins that read `/QuadPoints` and redaction
regions run. They use this crate's `js` numerics. `epdf-engine`'s copies use `f64::min`, `f64::max`
and the platform `hypot`, which treat NaN and the last bit differently. `PdfPoint` and `PdfQuad` are
aliases of `Point` and `Quad`, as `PagePoint`, `PageRect`, `PageBox` and `PageQuad` are: the
TypeScript types are structurally the same. `PdfRect` declares its members left, right, bottom, top,
the order every TypeScript producer writes them (`normalizePdfRect`, `pdfRectOf`, `pdfQuadBounds`);
the interface itself declares left, bottom, right, top.

`invert` and `page_transform` return `Result<_, NonInvertible>` where the TypeScript throws
`Mat2D.invert: non-invertible matrix (det = 0)`; the message is the same.

The phantom space brands (`PointIn<S>`, `RectIn<S>`, `QuadIn<S>`, `Mat2D<From, To>`) are not
ported. They are type-only and carry no runtime field; the Rust types are the plain `Point`, `Rect`,
`Quad` and `Mat2D = [f64; 6]`. That costs the compile-time check that a `view` rect never meets a
`page -> view` matrix. `Space` is ported as an enum.

## JavaScript numerics

`src/js.rs` holds the built-ins whose Rust counterparts answer differently, for this crate and
every port downstream of it: `round` (`Math.round` rounds `-2.5` to `-2`), `max`/`min` (NaN wins),
`sign` (`Math.sign(0)` is `0`), `hypot` (V8's normalized Kahan sum from `math.tq`, because
`normalizePdfQuad` compares lengths against `1e-6`), and `number_to_string` (`Number#toString`, so
`matrixToCss` prints `1e+21` and `1e-7` as the TypeScript does). `Math.atan2`, `cos` and `sin` use
Rust's `f64` methods; the vectors' 1e-12 relative tolerance absorbs a last-bit difference from V8's
fdlibm, and no case prints a transcendental result through `matrixToCss`.

## Mutation testing

These counts predate the port to named quad corners and the `engine-core` helpers, and have not
been rerun since.

`scripts/mutants.sh -p epdf-geometry --test-package epdf-geometry --file <file>`, file by file over
every module but `harness.rs`, catches 507 of 514 viable mutants (507 caught, 0 timeouts; 3
unviable). Two of those are killed by the new unit test `sign_keeps_signed_zeros_and_nan` in
`js.rs`, checked by applying each by hand: `>` to `>=` and `<` to `<=` in `sign`. Inside this crate
only `normalizeQuad` calls `sign`, on two areas that already passed `|area| > 1e-6 * len * len`, so
no vector passes it zero. The downstream ports in `epdf-annotation-core` and `epdf-plugin-stage` can.

The seven survivors are all in `src/js.rs`, and all equivalent:

- `js.rs:23` and `js.rs:37`, `>` to `>=` in `max` and `<` to `<=` in `min`. Equal operands return
  one branch earlier, so the mutated comparison never sees them.
- `js.rs:76` `>` to `>=` in `hypot`: an equal magnitude sets `max` to the value it already holds.
- `js.rs:93` and `js.rs:95`, the Kahan compensation's `-` to `+`. With two arguments the first
  term's compensation is exactly zero (`sum` starts at zero, so `preliminary` equals `summand`),
  and the second term's is never read.
- `js.rs:112` `<` to `<=` in `number_to_string`: zero has already returned.
- `js.rs:133` `<` to `<=`: the scientific branch only runs for exponents of at least 21 or at most
  -7, so the exponent is never 0.

Killing the `normalizeQuad` mutants took the threshold cases at the end of the text-quad suite. An
upright quad that `zigzagWellFormed` wrongly rejects still comes out of the geometric fallback with
the zigzag's own labeling, so each near-threshold base is also replayed turned by 90, 180 and 270
degrees and tilted off-axis, where a wrong verdict changes the answer.
