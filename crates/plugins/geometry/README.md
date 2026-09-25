# epdf-geometry

`packages/core/geometry` in Rust: every exported function, type and constant of
`src/index.ts`. All 609 cases in the four vector files replay identically
(`cargo test -p epdf-geometry`):

| File | Cases | Covers |
|---|---|---|
| `vectors/core/geometry-basics.json` | 115 | `boundsOfRects`, `intersectRects`, rotations, `snapToDevice`, `displaySize`, `deviceHeightForWidth`, `NO_FRAME` |
| `vectors/core/geometry-affine.json` | 110 | `Mat2D` compose, invert, apply*, the builders, `angleOf`, `matrixToCss` |
| `vectors/core/geometry-page.json` | 104 | `rotateScaleMatrix`, `pdfToContentMatrix`, `pageGeometry`, `pageTransform` |
| `vectors/core/geometry-text-quad.json` | 280 | the `TextQuad` helpers and `normalizeQuad` |

The generators reach every statement (801/801) and every branch (93/93) of the TypeScript.

## Where the port differs in shape

`PageRotation` is a newtype over `f64`, not an enum. The TypeScript union `0 | 90 | 180 | 270`
isn't checked at runtime, and every function gives other numbers an answer: `addRotations(-90, 0)`
is `-90`, `isQuarterTurn(45)` is false, `rotateScaleMatrix` treats 45 as 0. The vectors pin those
answers, so the type keeps the number. `PageRotation::DEG_0` through `DEG_270` are the four values
the type means.

`pageTransform` returns an object of closures in TypeScript. Here `PageTransform` holds the four
matrices it builds and exposes the closures as methods (`to_pixels`, `content_to_view_rect`, ...);
it serializes to the data members only. The vector entry is a wrapper in
`tooling/plugin-conformance/src/suites/geometry.ts` that calls each method on probe points and
rects, and `src/harness.rs` mirrors it.

`pdfToContentMatrix({left, top})` is `pdf_to_content_matrix(left, top)`, the two numbers it reads.
`invert`, `page_geometry` and `page_transform` return `Result<_, NonInvertible>` where the
TypeScript throws `Mat2D.invert: non-invertible matrix (det = 0)`; the message is the same.

The phantom space brands (`PointIn<S>`, `RectIn<S>`, `QuadIn<S>`, `TextQuadIn<S>`,
`Mat2D<From, To>`) are not ported. They are type-only and carry no runtime field; the Rust types are
the plain `Point`, `Rect`, `Quad`, `TextQuad` and `Mat2D = [f64; 6]`. That costs the compile-time
check that a `pdf` rect never meets a `content -> view` matrix. `Space` is ported as an enum.

## JavaScript numerics

`src/js.rs` holds the built-ins whose Rust counterparts answer differently, for this crate and
every port downstream of it: `round` (`Math.round` rounds `-2.5` to `-2`), `max`/`min` (NaN wins),
`sign` (`Math.sign(0)` is `0`), `hypot` (V8's normalized Kahan sum from `math.tq`, because
`normalizeQuad` compares lengths against `1e-6`), and `number_to_string` (`Number#toString`, so
`matrixToCss` prints `1e+21` and `1e-7` as the TypeScript does). `Math.atan2`, `cos` and `sin` use
Rust's `f64` methods; the vectors' 1e-12 relative tolerance absorbs a last-bit difference from V8's
fdlibm, and no case prints a transcendental result through `matrixToCss`.

## Mutation testing

`cargo mutants -p epdf-geometry --test-package epdf-geometry` tests 449 mutants: 438 caught, 9
missed, 2 unviable. Every miss is equivalent:

- `js.rs:23` and `js.rs:37`, `>` to `>=` in `max` and `<` to `<=` in `min`. Equal operands return
  one branch earlier, so the mutated comparison never sees them.
- `js.rs:56` `>` to `>=` and `js.rs:58` `<` to `<=` in `sign`. `sign` is only called on
  `normalizeQuad`'s two areas after both passed `|area| > 1e-6 * len * len`, so neither is zero.
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
