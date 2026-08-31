# Shared conformance vectors

One frozen contract, three implementations. `anchor-vectors.json` is consumed by:

- `poc-1-ts/packages/shapes/src/vectors.test.ts` (TypeScript reference)
- `poc-2-rust-geometry/crates/geometry/tests/vectors.rs` (Rust geometry)
- `poc-3-rust-core/crates/core/tests/vectors.rs` (Rust core)

The values were generated once from the TypeScript reference with
`poc-1-ts/scripts/gen-vectors.mjs`, reviewed, then frozen. They are not
regenerated as part of any test run, which is the point: a Rust port that
disagrees with the frozen file has a bug, and regenerating would hide it.

Regenerate deliberately, and only when the intended behavior changes:

```
cd poc-1-ts && node scripts/gen-vectors.mjs
```
