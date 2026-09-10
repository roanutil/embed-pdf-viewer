//! Browser and Node worker binding layer, built for `wasm32-unknown-emscripten`.
//!
//! Worker protocol decode and buffer handoff. No semantics: those are in
//! `epdf-engine`.
//!
//! Toolchain note, and it is an exit criterion rather than a footnote: `-O3`
//! makes `em++` run `wasm-opt`, emsdk 3.1.72's bundled Binaryen v119 rejects two
//! WASM features present in `rustc` 1.98.0's precompiled std, and Emscripten
//! takes Binaryen from its config rather than `PATH`. The build selects it
//! explicitly through `EM_BINARYEN_ROOT` and asserts the version it got.
