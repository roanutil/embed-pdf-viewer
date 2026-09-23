//! Re-emits the rpath for this crate's own test binaries.
//!
//! The exports here build for `wasm32-unknown-emscripten`, where nothing is
//! loaded at run time, but their tests run on the host and link the dylib like
//! any other. Cargo does not propagate `cargo:rustc-link-arg` from a
//! dependency's build script to a dependent package's test targets, so without
//! this `cargo test -p epdf-wasm` links and then aborts with
//! `Library not loaded: @rpath/libembedpdf.dylib`. Same line, same reason, as
//! `crates/pdfium` and `crates/epdf-engine`.
fn main() {
    build_support::emit_rpath_only();
}
