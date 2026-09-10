# PDFium binding benchmarks

This standalone experiment compares four ways to call PDFium from JavaScript.
The WebAssembly (Wasm) builds link the same PDFium archive:

| Variant | Implementation | Work done per call |
| --- | --- | --- |
| a: direct | Emscripten exports called through `cwrap` | One PDFium function |
| b: Rust wrapper | Rust forwards calls to PDFium | One PDFium function |
| c: Rust operations | Rust runs geometry, text, render, and search operations | One whole operation |
| d: C++ operations | C++ implements the same four operations | One whole operation |

The scripts use the term **arm** for a variant and **coarse** for an API that
performs a whole operation. Comparing c with d helps distinguish the benefit of
fewer JavaScript calls from differences between the Rust and C++ implementations.

See [the experiment design](../docs/research/pdfium-rust-wrapper-benchmarks.md)
and [the saved results and limitations](RESULTS.md).

## Run the Wasm benchmark

Run these commands in Bash from `pdfium-binding-benchmarks` (start `bash` first
if your interactive shell is Zsh):

```bash
bash build/fetch.sh wasm32
source build/emsdk.sh
bash build/link-wasm.sh
node bench/run.mjs --rounds 7 --out /tmp/pdfium-binding-review
node bench/compare.mjs --out /tmp/pdfium-binding-review
```

The fetch script uses the pin in
[`engine-runtime-build.json`](../packages/engine/runtime/engine-runtime-build.json).
The link script builds the Rust crates and all four Wasm modules. It requires
`em++` and the Rust toolchain specified in `rust-toolchain.toml`.

The [September 8 build audit](../docs/research/platform-gap-audit.md) records
Emscripten 3.1.72 with Binaryen 132 for the related multiplatform prototype.
If the SDK's bundled `wasm-opt` rejects Rust's Wasm features, set
`EM_BINARYEN_ROOT` to a compatible Binaryen installation containing `bin/wasm-opt`
before linking. The saved benchmark binaries may differ from a fresh build.

Before timing, `bench/run.mjs` compares the variants' outputs on the three
fixtures in `bench/fixture.mjs`: `report-p4`, `rotated-p0`, and `flipped-p0`.
It stops if the checked outputs disagree. Timing uses page index 4 of
`report.pdf` by default; page indices start at zero.

Rounds alternate between variants, rotating the starting variant each round.
Use a fresh `--out` directory for exploratory runs: the runner deletes existing
JSON files for the selected mode in that directory. Without `--out`, it replaces
that mode's committed data in `bench/results/`.

After building, run `node --test bench/*.test.mjs` for the JavaScript tests.
`bench/run.test.mjs` runs a short benchmark in a temporary directory and needs
`em++` on `PATH` to check the recorded compiler version.

## Run the native benchmark

The native runner and build script are set up for ARM64 macOS. Native variant a
uses the repository runtime addon. Build it from `packages/engine/runtime`:

```bash
pnpm build:target darwin-arm64
```

Then run from `pdfium-binding-benchmarks`:

```bash
bash build/fetch.sh darwin-arm64
npm ci
bash build/build-native.sh darwin-arm64
node bench/run.mjs --native --rounds 7 --out /tmp/pdfium-binding-native-review
node bench/compare.mjs --native --out /tmp/pdfium-binding-native-review
```

`build/build-native.sh` builds b/c/d, but does not build a. If the runner reports
that `packages/engine/runtime/npm/darwin-arm64/lib/pdf-runtime.node` is missing,
use the runtime build command above; the runner's generic suggestion to run
`build-native.sh` cannot produce that file.

Native a uses C++ bindings and BigInt pointers. Native b/c use `napi-rs` and
numeric pointers; d uses C++ bindings and numeric pointers. These differences
prevent an a-versus-b comparison from isolating the cost of a Rust wrapper.
