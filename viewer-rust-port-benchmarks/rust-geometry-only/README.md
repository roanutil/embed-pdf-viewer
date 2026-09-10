# Rust geometry only

This variant moves geometry calculations into Rust/Wasm. State updates and
subscriptions remain in TypeScript. Its demo and tests also handle Wasm
initialization and count calls across the language boundary.

| Path | Responsibility |
| --- | --- |
| `crates/geometry/src/core.rs` | Geometry functions that can be tested natively |
| `crates/geometry/src/lib.rs` | Wasm exports using flat numeric arrays or serialized objects |
| `packages/geometry` | TypeScript wrappers and initialization |
| `packages/shapes` | TypeScript state updates and scene generation |

Run from this directory:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
pnpm install
pnpm build
pnpm test
```

Then use `pnpm dev` for the demo on port 5172, or `cargo test` for native Rust
tests. The build places generated files in `packages/geometry/wasm`.

## Calls per shape

[`scene.ts`](packages/shapes/src/scene.ts) calls geometry through
[`anchor.ts`](packages/shapes/src/anchor.ts). A plain shape needs one Wasm call
for `rectQuad`. An anchored shape at zoom 1 and rotation 0 needs three: that
quad plus two `normalizeDeg` calls used to decide whether it is projected.

When both scale correction and counter-rotation are needed, an anchored shape
uses seven calls: `rectQuad`, two calls from `anchorFactors`, `quadBounds`,
`quadScaleAbout`, another `normalizeDeg`, and `quadRotateAbout`.
The [shape tests](packages/shapes/src/shapes.test.ts) assert the measured cases.
Seven is not the count for every anchored shape at every view.

## Object identity and initialization

`projectQuad` returns the caller's quad unchanged when no transformation is
needed. For an anchored shape, deciding this still calls Rust's `normalizeDeg`;
for an unanchored shape, the decision makes no Wasm call.

By contrast, passing a quad through `quadScaleAbout` produces a new JavaScript
object even when its coordinates are unchanged. The
[geometry tests](packages/geometry/src/geometry.test.ts) check that distinction.

Call `await initGeometry()` before using geometry functions. Test fixtures that
need Wasm must be created after initialization, rather than while the test file
is being loaded. When a host logger is supplied, initialization registers it
again even if the module is already loaded.

## Callback measurement

Rust can call a JavaScript logger through `setHostLogger`, `greetHost`, and
`benchHostCalls`. The demo's measurement button compares flat-array calls,
serialized-object calls, and callbacks. It installs a logger that increments a counter for callback
timing so that React updates are not included. These are local measurements;
there is no committed raw sample set for the older per-crossing figures.
