# Swift geometry-only viewer benchmark

This is the Swift 6.4 counterpart to `rust-geometry-only`: model/state stay in
TypeScript while every geometry primitive crosses into Embedded Swift Wasm.
The shared root benchmark supplies the workloads and checks its display list
against the TypeScript baseline before timing.

```bash
bash scripts/build-wasm.sh
cd ..
node bench/run.mjs --rounds 7
node bench/compare.mjs
```

Install the official `swift-6.4.0-RELEASE_wasm-embedded` SDK using the command
in the root README first. Generated Wasm is ignored under `wasm/`.
