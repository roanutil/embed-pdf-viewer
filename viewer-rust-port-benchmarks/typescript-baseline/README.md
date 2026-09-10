# TypeScript baseline

This viewer implements geometry and state in TypeScript. It is the reference
for the two Rust variants and does not load Wasm.

| Path | Responsibility |
| --- | --- |
| `packages/geometry` | Points, rectangles, quads, transforms, and containment |
| `packages/shapes` | State updates, effects, scene generation, and hit-testing |
| `packages/store` | State subscriptions and effect handlers |
| `packages/react` | React bindings and the SVG canvas |
| `apps/demo` | Vite application |

Run from this directory:

```bash
pnpm install
pnpm test
pnpm dev
```

The demo uses port 5171. `pnpm build` creates the production demo.

## Geometry behavior

[`anchor.ts`](packages/shapes/src/anchor.ts) scales anchored shapes by
`1 / max(zoom, 1)` and counter-rotates them around their bounds' top-left corner.
When no transformation is needed, `projectQuad` returns the original quad
object. [`shapes.test.ts`](packages/shapes/src/shapes.test.ts) checks both
coordinates and object identity.

Below zoom 1, the correction stays at 1, so anchored shapes shrink with the page.
This is the prototype's explicit rule; the tests record it.

[`scene.ts`](packages/shapes/src/scene.ts) produces a display list for the whole
model. The state-in-Rust variant uses the same whole-scene operation to reduce
calls between JavaScript and Wasm.

## Update expected results

Only after an intentional behavior change, run `pnpm gen:vectors` to replace
`../vectors/anchor-vectors.json`. Review the diff and run all three variants'
vector tests. Do not regenerate expectations merely to make a failing test pass.
