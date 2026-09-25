//! Node-API linker flags, and the rpath to the fork.
//!
//! `napi_build::setup()` emits what a Node addon needs to link: on macOS that
//! is `-undefined dynamic_lookup`, because the Node symbols an addon calls are
//! resolved by the process that loads it rather than at link time.
//!
//! The rpath is separate and equally necessary. Cargo does not propagate
//! `cargo:rustc-link-arg` from a dependency's build script to a dependent
//! package's own targets, so without this line the addon links and then fails
//! at `dlopen` with `libembedpdf.dylib` not found, which reads as a broken
//! install rather than a missing flag. Same reason as `crates/pdfium`,
//! `crates/epdf-engine` and `crates/epdf-uniffi`.
fn main() {
    napi_build::setup();
    build_support::emit_rpath_only();
}
