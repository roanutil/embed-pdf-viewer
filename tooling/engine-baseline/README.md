# Phase 0 geometry baseline

The number every later `pages.geometry` claim is measured against.
`multiplatform-plan.md` Section 10 asks for it in one sentence: "Phase 0
measurement of `pages.geometry` runs through the unchanged service boundary
with run materialization, transfer and allocation counted."

So this times `doc.page(pon).geometry.read()`, not a reader in isolation. The
borrow, the envelope, the wire and the decode are all inside the measurement,
because they are all inside what a caller pays. Two arms, one runtime:
`typescript` is the shipping `PageGeometryReader`, `rust` is
`RustPageGeometryReader`, and the only difference between them is where the
glyph loop runs.

## Running it

The `rust` arm needs a runtime with the core linked in, which no published
payload is. Build one with the recipe in
`tooling/engine-conformance/README.md`, swap it into
`packages/engine/runtime/npm/wasm32/lib`, then:

```bash
pnpm --filter @embedpdf/tooling-engine-baseline run baseline
```

`--expose-gc` is in the script and is not optional: the allocation pass needs a
forced collection between readings, and reporting whatever the heap happened to
look like would be worse than refusing to report.

`BASELINE_ROUNDS`, `BASELINE_ITERATIONS` and `BASELINE_WARMUP` override the
protocol for a quick check. The committed numbers are 7 rounds of 20.

## What the protocol is for

Each rule here exists because the earlier benchmark study on this branch found
a bug that the rule would have caught. Section 3 records three of them, none
caught by the fairness gate of the day.

The **answer gate runs before any timing**, over every fixture. A wrong arm can
be fast, and no timing table shows it. It is structural rather than textual,
because `PdfRect` is declared `left, bottom, right, top` and the TypeScript
reader builds its literals `left, right, bottom, top`: the same rectangle,
different key order, and a text comparison would report a divergence that is
not one.

**Arm order rotates by round.** A laptop's clock drifts over a multi-minute run
and background load decays within each round, so a fixed order hands the first
position to the same arm every time.

**Results are consumed.** The reads return a large object graph, and an engine
that can prove nothing reads it may skip building it.

**Medians and p95, never means**, and the raw per-iteration samples are kept in
`results/` so the summary can be recomputed rather than believed. The toolchain
that produced them is recorded beside them, including the sha256 of the exact
wasm that was loaded, which is what says whether the Rust core was in it.

## What this does not measure

`InlineTransport` has no thread boundary, so **transfer is not exercised**.
Section 10 asks for it to be counted and this does not count it. A browser
number needs the `tooling/viewer-e2e/` harness and a real worker; until then,
treat these as the decode-and-materialize cost without the structured clone.

There is no **optimized TypeScript arm** yet. Section 9 requires one before any
release-optimization comparison. These numbers are a baseline for regression,
not an argument that one implementation is faster than the other could be.
