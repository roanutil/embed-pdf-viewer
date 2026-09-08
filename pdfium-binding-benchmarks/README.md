# pdfium-binding-benchmarks

A spike, not a dependency. Nothing published depends on this directory, and a dead end costs one
`rm -rf pdfium-binding-benchmarks`.

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

`-O3` makes `em++` run `wasm-opt` over the linked module. emsdk 3.1.72 bundles `wasm-opt` v119, which
rejects two wasm feature names present in rustc 1.98's precompiled std. The numbers in `RESULTS.md`
were produced on a machine where a newer `wasm-opt` had been symlinked over that bundled binary; a
fresh `emsdk.sh` install needs a compatible Binaryen. Replacing SDK binaries is not
required: set `EM_BINARYEN_ROOT` to a Binaryen installation containing `bin/wasm-opt`.
The [September 8 audit](../docs/research/platform-gap-audit.md) rebuilt the multiplatform
prototype with the same Emscripten 3.1.72 and Binaryen 132 using this override.
Emscripten may take Binaryen from its config rather than `PATH`, so select it explicitly.

`node --test bench/run.test.mjs` is the smoke test: two short rounds, checked for plausible shapes
(render at 4x costs more than at 1x, the glyph loop dwarfs a single null call), not for
publishable numbers. It passes `--out` a temp directory, because `bench/results/` holds the 56
committed files behind every table in `RESULTS.md` and any run clears the ones it is about to
replace. Use `--out` for any exploratory run you don't want in the published set.

## Native

Same four arms, same gate, same fixture, run against native addons instead of wasm modules:

```bash
bash build/fetch.sh darwin-arm64          # pulls the prebuilt libembedpdf for this target
npm install                               # cmake-js and node-addon-api, both devDependencies here
bash build/build-native.sh darwin-arm64   # builds arms b, c and d
node bench/run.mjs --native --rounds 7    # fairness gate, then seven interleaved rounds
node bench/compare.mjs --native           # medians and p95, as markdown
```

`npm install` has to run inside `pdfium-binding-benchmarks` before the native build. `build/build-native.sh`
shells out to `npx cmake-js`, and `build/CMakeLists.txt` resolves `node-addon-api` by running
`node -p "require('node-addon-api').include"` from the spike root, so both packages need to be
installed here. Nothing else in this directory documents that.

`build/build-native.sh` builds arms b, c, and d only. Native arm a is
`packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node`, the shipping addon, and
`packages/engine/runtime/.gitignore` ignores it, so a fresh clone doesn't have it. Build it from
`packages/engine/runtime` with `pnpm build:target darwin-arm64`. `bench/run.mjs` checks for all three
native artifacts before running and, if arm a's is missing, tells you to run `build-native.sh`; that
script will not produce it, so run the `pnpm build:target` command above instead.

The native half is macOS arm64 only: `darwin-arm64` is hardcoded at `bench/run.mjs:45` and
`bench/arm.mjs:182`. There is no `--native` path for any other OS or architecture.
