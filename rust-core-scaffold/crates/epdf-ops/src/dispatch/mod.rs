//! One trait-shaped API, two implementations chosen by target.
//!
//! On every native target the fork builds with embedpdf_thread_local_globals=true
//! (see runtime-src/scripts/embedpdf-runtime/build-target.sh:70-77), so PDFium's
//! globals are per-thread and a handle cannot cross threads. `threaded` owns one
//! OS thread and every op is a channel round-trip to it.
//!
//! On wasm32-unknown-emscripten there are no threads and each wasm instance has
//! its own linear memory, which is why that target leaves the flag off. `inline`
//! calls straight through.

#[cfg(not(target_os = "emscripten"))]
mod threaded;
#[cfg(not(target_os = "emscripten"))]
pub use threaded::Worker;

#[cfg(target_os = "emscripten")]
mod inline;
#[cfg(target_os = "emscripten")]
pub use inline::Worker;
