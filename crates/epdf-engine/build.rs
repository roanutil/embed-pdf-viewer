//! Re-emits the rpath for this crate's own test binaries.
//!
//! `pdfium-sys` emits `-Wl,-rpath,...` for the crate that links libembedpdf,
//! but Cargo does not propagate `cargo:rustc-link-arg` to a dependent
//! package's bin and test targets: only `rustc-link-lib` and
//! `rustc-link-search` are transitive. Every crate that depends on `pdfium`
//! and builds its own binaries needs this line, or its tests link and then
//! fail at load with `Library not loaded: @rpath/libembedpdf.dylib`.
fn main() {
    build_support::emit_rpath_only();
}
