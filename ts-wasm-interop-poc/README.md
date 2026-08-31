# ts-wasm-interop-poc

Three copies of the same toy viewer, at three stages of a Rust port. The point is to put real
numbers on the interop questions in `docs/research/rust-core-port.md`, which argues for porting
bottom-up and warns that a fine-grained WASM boundary is worse than no port at all.

| | `poc-1-ts` | `poc-2-rust-geometry` | `poc-3-rust-core` |
|---|---|---|---|
| Implementation | all TypeScript | geometry in Rust | geometry + brain + store in Rust |
| Crossings per frame | 0 | 1 per unanchored shape, 7 per anchored one | 1, at any shape count |
| WASM shipped | none | 130.7 kB | 277.2 kB |
| Demo bundle | 152.7 kB JS | 163.2 kB JS + 133.9 kB wasm | 162.2 kB JS + 283.9 kB wasm |
| Demo port | 5171 | 5172 | 5173 |

The measured cost of one crossing, from the `measure` button in the poc-2 demo (20,000 calls each,
Chrome on an M-series Mac): 1.015 µs through a flat `Float64Array`, 2.735 µs through
`serde-wasm-bindgen` objects, 0.225 µs for a Rust to TypeScript callback. So the ergonomic object
surface costs about 2.7x the flat one, and calling back into JS is the cheapest direction.

## The domain

Rectangles on a page, with the one interesting behavior lifted from the real annotation core: a
shape can be `anchored`, which exempts it from the view's zoom the way ISO 32000-2 `/NoZoom` does.
An anchored shape holds its bounds top-left fixed on the page and its size fixed on screen, and the
projection is a similarity about that anchor. Rendering, hit-testing and dragging all read the same
projection, so what you see and what you click cannot disagree.

That behavior is what makes the POC worth anything: it is the smallest function that is pure math,
called per shape per frame, and load-bearing for correctness. `packages/core/annotation/src/anchor.ts`
in the real repo is the same shape.

## The stack

Same as the main repo, minus what a toy does not need: pnpm workspaces, turbo, TypeScript 5.9 with
`moduleResolution: bundler`, vitest, and Vite for the demo. Packages export `./src/index.ts`
directly, exactly as the real repo's `exports` field does, so there is no per-package build step and
no `tsdown`. The wasm packages do have a real `build` script, and turbo sequences it ahead of the
demo, which is the pipeline question that actually matters here.

## Running them

You need Node 24, pnpm 10.34, a Rust toolchain with the `wasm32-unknown-unknown` target, and
`wasm-bindgen-cli` at the exact version the crates pin.

```
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
```

Then, in any of the three directories:

```
pnpm install
pnpm build      # builds the wasm first, then the demo
pnpm dev        # http://localhost:5171 / 5172 / 5173
pnpm test
cargo test      # poc-2 and poc-3 only
```

`scripts/build-wasm.mjs` checks the CLI version against the `wasm-bindgen = "=0.2.127"` pin in
`Cargo.toml` and fails with the exact `cargo install` command if they differ. A mismatch there
produces glue that is wrong in ways that only show up at runtime, so it is worth failing early.

> Note: the demos run under React `StrictMode`, which double-invokes renders in development. The
> crossing counter in the UI therefore shows roughly twice the per-frame number. The exact figures
> are asserted in the test suites, not read off the screen.

## The shared contract

`vectors/anchor-vectors.json` holds 36 projection vectors and 15 hit-test vectors, generated once
from the poc-1 TypeScript reference and then frozen. Three implementations read that one file:

- `poc-1-ts/packages/shapes/src/vectors.test.ts`
- `poc-2-rust-geometry/crates/geometry/tests/vectors.rs`
- `poc-3-rust-core/crates/core/tests/vectors.rs`

It is never regenerated as part of a test run. That is the whole idea: a Rust port that drifts from
the reference fails a test instead of being discovered later in a browser. This is the mechanism the
port plan leans on for wave 2, where `packages/engine/core/src/conformance/runAnnotationMutationConformance.ts`
already provides 2,255 lines of the same thing for free.

## What we learned that the plan did not predict

The identity short-circuit survives a leaf port, as long as the DECISION stays in TypeScript.
`anchoredGeom` returns its input unchanged when the projection is the identity, and the plan treats
losing that as a cost of porting. It isn't, quite: in poc-2 `projectQuad` still returns the caller's
own object, because `anchorFactors` decides in TypeScript and the identity path never calls into
wasm at all. What you lose is the free-ness of the decision, not the short-circuit.

Two bugs showed up that no amount of planning would have caught, both in poc-3 and both documented
at their fix sites. A `&mut self` export must not call back into JS: we had Rust notify a JS change
listener from inside `dispatch`, React re-read `core.version` under the mutable borrow, and the
damage surfaced much later as `attempted to take ownership of Rust value while it was borrowed` on
`free()`. And `useSyncExternalStore` calls `getSnapshot` several times per render, so reading a
wasm-bindgen getter there cost six crossings per re-render for a value we already knew had changed.
Mirroring the version on the TypeScript side made a re-render free.

Neither is exotic. Both are the kind of thing that decides whether a port feels good or feels
cursed, and both are invisible until you have a React tree actually rendering.
