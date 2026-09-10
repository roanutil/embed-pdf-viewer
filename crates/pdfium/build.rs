//! Re-emits the rpath half of pdfium-sys/build.rs's link directives.
//!
//! Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
//! script to a dependent crate's own test and bin targets; only
//! `rustc-link-lib` and `rustc-link-search` are transitive. pdfium-sys's own
//! `-Wl,-rpath,...` therefore only takes effect for pdfium-sys's own targets,
//! so without this, `cargo test -p pdfium` links against libembedpdf fine but
//! the test binary cannot find the dylib at run time.
fn main() {
    build_support::emit_rpath_only();
}
