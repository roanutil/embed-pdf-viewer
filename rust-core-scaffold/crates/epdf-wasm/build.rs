//! Re-emits the rpath half of pdfium-sys/build.rs's link directives.
//!
//! Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
//! script to a dependent crate's own test/bin targets: only
//! `rustc-link-lib`/`rustc-link-search` are transitive. Without this, `cargo
//! test -p epdf-wasm` links against libembedpdf fine but the resulting test
//! binary cannot find the dylib at runtime on a host build (native tests are
//! the only ones that run this build script's non-emscripten branch;
//! build/link-wasm.sh links wasm32-unknown-emscripten separately and this
//! script emits nothing there). See build_support::emit_rpath_only for the
//! shared logic and pdfium/build.rs, epdf-node/build.rs for the same pattern.
fn main() {
    build_support::emit_rpath_only();
}
