# Shared conformance vectors

One frozen contract, three implementations. `anchor-vectors.json` is consumed by:

- `typescript-baseline/packages/shapes/src/vectors.test.ts` (TypeScript reference)
- `rust-geometry-only/crates/geometry/tests/vectors.rs` (Rust geometry)
- `rust-geometry-and-state/crates/core/tests/vectors.rs` (Rust core)

The values were generated once from the TypeScript reference with
`typescript-baseline/scripts/gen-vectors.mjs`, reviewed, then frozen. They are not
regenerated as part of any test run, which is the point: a Rust port that
disagrees with the frozen file has a bug, and regenerating would hide it.

Regenerate deliberately, and only when the intended behavior changes:

```
cd typescript-baseline && node scripts/gen-vectors.mjs
```
