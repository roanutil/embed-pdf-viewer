//! Re-emits the rpath half of pdfium-sys/build.rs's link directives.
//!
//! Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
//! script to a dependent crate's own test/bin targets: only
//! `rustc-link-lib`/`rustc-link-search` are transitive. pdfium-sys's own
//! `-Wl,-rpath,...` therefore only takes effect when building pdfium-sys's
//! own targets, so without this, `cargo test -p pdfium` links against
//! libembedpdf fine but the resulting test binary cannot find the dylib at
//! runtime. See build_support::emit_rpath_only for the shared logic.
fn main() {
    build_support::emit_rpath_only();
}
