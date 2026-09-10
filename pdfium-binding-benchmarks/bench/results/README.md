# Saved measurement data

This directory contains 56 committed JSON files: seven rounds for each of four
variants, in both Wasm and native modes. Each file records the environment,
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

These are historical measurements, not results from the current code. In
particular, the native data predates removal of an extra JavaScript copy in
`bench/arm.mjs`'s `readBytes` and `readU16`. The current correctness check also
includes a third fixture, `flipped-p0`, beyond the two reported in the original
results. Rebuild and rerun before making claims about current performance.

`bench/run.mjs` deletes existing JSON files for the selected mode in its output
directory. Use `--out` with a fresh directory to preserve these saved results,
and pass the same `--out` to `bench/compare.mjs`.
