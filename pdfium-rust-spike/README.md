# pdfium-rust-spike

A spike, not a dependency. Nothing published depends on this directory, and a dead end costs one
`rm -rf pdfium-rust-spike`.

It measures what it costs to put a layer between TypeScript and the PDFium fork in
`packages/engine/runtime`, across four wasm builds of the same PDFium archive:

- **a**, direct: `libembedpdf.a` linked by `em++`, called from TypeScript through `cwrap`. This is
  what ships today.
- **b**, Rust thin: one `extern "C"` Rust function per PDFium call the workloads touch, added to the
  same link. Same call count as arm a, one extra wasm frame per call. This is the wrapper tax.
- **c**, Rust coarse: four Rust functions that each run a whole loop (glyph geometry, text
  extraction, render, search) inside Rust and hand back one packed buffer, so TypeScript crosses
  once per operation instead of once per PDFium call.
- **d**, C++ coarse: arm c's four operations, written in C++ instead of Rust, linked as a plain
  `.cc` file on the same command line. The control: it separates "coarsening the crossing helped"
  from "Rust helped."

The reasoning behind the four arms, the ten workloads, and the predictions made before measuring
are in [`docs/research/pdfium-rust-wrapper-benchmarks.md`](../docs/research/pdfium-rust-wrapper-benchmarks.md).
Results are in [`RESULTS.md`](./RESULTS.md).

## Reproducing

```bash
bash build/fetch.sh wasm32       # pulls the prebuilt libembedpdf.a the shipping build uses
bash build/link-wasm.sh          # builds crates/shim and crates/ops, links all four arms
node bench/run.mjs --rounds 7    # fairness gate, then seven interleaved rounds of a, b, c, d
node bench/compare.mjs           # medians, p95, and the wasm size table, as markdown
```

`build/link-wasm.sh` needs `em++` on `PATH`; `source build/emsdk.sh` first if it isn't. The gate in
`bench/run.mjs` runs before any timing and aborts if any two arms disagree on an answer: a wrong arm
can be fast, and no timing table would reveal it on its own.

`node --test bench/run.test.mjs` is the smoke test: two short rounds, checked for plausible shapes
(render at 4x costs more than at 1x, the glyph loop dwarfs a single null call), not for
publishable numbers.
