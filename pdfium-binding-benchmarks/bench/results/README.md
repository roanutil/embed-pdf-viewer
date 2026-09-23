# Saved measurement data

This directory contains 63 committed JSON files: seven rounds for each of four
Wasm variants and each of five native ones. Each file records the environment,
fixture, round order, timings, and the sum accumulated from operation return values by the harness.

Run from `pdfium-binding-benchmarks` to aggregate the saved values:

```bash
node bench/compare.mjs
node bench/compare.mjs --native
```

These commands print median timings, the median of each round's 95th-percentile
batch timings, and round-to-round variation. Wasm mode also prints module sizes.
The tables in [RESULTS.md](../../RESULTS.md) use those values; their formatting
and explanatory text are maintained separately.

The native files were rerun on 2026-09-23, after the extra JavaScript copy was
removed from `bench/arm.mjs`'s `readBytes` and `readU16`; they include a fifth
arm, Swift, that the earlier run did not. The Wasm files are the original
measurements and still predate that fix, which does not affect them: the copy
was in the native memory helpers.

These remain measurements of saved builds on one machine, not results from the
current code. The current correctness check also includes a third fixture,
`flipped-p0`, beyond the two reported in the original results. Rebuild and
rerun before making claims about current performance.

`bench/run.mjs` deletes existing JSON files for the selected mode in its output
directory. Use `--out` with a fresh directory to preserve these saved results,
and pass the same `--out` to `bench/compare.mjs`.
