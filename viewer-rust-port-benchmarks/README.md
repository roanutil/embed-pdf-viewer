# Viewer Rust port benchmarks

Three small viewers compare where TypeScript ends and Rust begins. They render
rectangles, support selection and dragging, and share expected geometry results.
They do not render PDFs or benchmark the full EmbedPDF viewer.

| Variant | Implementation | Calls into Wasm to generate a display list | Demo port |
| --- | --- | --- | --- |
| `typescript-baseline` | Geometry and state in TypeScript | 0 | 5171 |
| `rust-geometry-only` | Geometry in Rust; state in TypeScript | 1 per plain shape; up to 7 per anchored shape | 5172 |
| `rust-geometry-and-state` | Geometry and state in Rust | 1 with the row cache current; up to 3 when it needs checking or refreshing | 5173 |

A **crossing** means a call between JavaScript and Wasm. These counts describe
display-list generation, not all work in a React frame. Fewer crossings do not
necessarily mean faster execution: object conversion and the calculation itself
also take time. See the [historical results](bench/RESULTS.md) and
[measurement procedure](bench/README.md).

## Run a viewer

Use Node 24 for the documented setup and pnpm 10.34.0, as specified by the
viewer package manifests. The Rust variants also need a Rust toolchain and the
matching binding generator:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.127
```

Run inside the chosen variant directory:

```bash
pnpm install
pnpm build
pnpm test
pnpm dev
```

For either Rust variant, also run `cargo test` in that directory.
`scripts/build-wasm.mjs` checks the CLI version against the crate's exact
`wasm-bindgen` dependency. Build before starting a Rust demo so its generated
JavaScript and Wasm files exist.

Each viewer has its own pnpm workspace and Turbo tasks. TypeScript packages
export source files; Rust packages build Wasm before the demo build.
React development rendering can repeat work, so use the tests for operation
crossing counts instead of treating the demo counter as a precise frame cost.

## Anchored shapes

An anchored rectangle keeps the top-left of its unprojected bounds fixed.
At zoom above 1, its geometry is scaled inversely to the page zoom; it also
counter-rotates against the view. Below zoom 1, the scale correction is clamped,
so the rectangle shrinks with the page. This is the behavior implemented by
[`anchor.ts`](typescript-baseline/packages/shapes/src/anchor.ts), not a claim
that the demo implements every PDF annotation rule.

Scene generation and hit-testing use the same projected geometry. The prototype
is related to the repository's
[annotation anchor implementation](../packages/core/annotation/src/anchor.ts).

## Shared expected results

[`vectors/anchor-vectors.json`](vectors/anchor-vectors.json) contains 36 projection
cases and 15 hit-test cases. The TypeScript reference and both Rust implementations
read these frozen expectations. Tests do not regenerate them. A disagreement
requires investigation; it does not by itself establish which implementation
or expected value is wrong.

See [vector maintenance](vectors/README.md) before changing those expectations.
The broader motivation is in [the port plan](../docs/research/rust-core-port.md).

## Implementation lessons

The geometry-only wrapper can return the caller's original quad when no
transformation is needed. For anchored shapes, deciding that still calls the
Rust `normalizeDeg` function. For unanchored shapes, the decision returns before
that call.

The state-in-Rust wrapper notifies React after Rust's `dispatch` returns. That
avoids reentering the same Rust object while it is mutably borrowed. It also
caches a numeric version in TypeScript, so repeated React snapshot reads do not
call Wasm. See
[`store.ts`](rust-geometry-and-state/packages/core/src/store.ts) and
[its React tests](rust-geometry-and-state/packages/react/src/binding.test.tsx).
