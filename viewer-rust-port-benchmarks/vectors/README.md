# Shared expected geometry results

[`anchor-vectors.json`](anchor-vectors.json) contains 36 projection cases and
15 hit-test cases. These suites read it:

- [TypeScript reference](../typescript-baseline/packages/shapes/src/vectors.test.ts)
- [Rust geometry](../rust-geometry-only/crates/geometry/tests/vectors.rs)
- [Rust geometry and state](../rust-geometry-and-state/crates/core/tests/vectors.rs)

The [generator](../typescript-baseline/scripts/gen-vectors.mjs) uses the
TypeScript reference. Tests read the frozen file without regenerating it.
A disagreement requires checking the implementation and the expected behavior;
regenerating the file would otherwise hide the disagreement.

After an intentional behavior change, run from `viewer-rust-port-benchmarks`:

```bash
(cd typescript-baseline && pnpm gen:vectors)
```

Install the baseline's dependencies first. The package script uses `tsx` to
resolve its TypeScript imports. Review the resulting diff and run all three
vector suites before accepting new expectations.
