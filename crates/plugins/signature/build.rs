//! Re-emits the rpath for this crate's own test binaries.
//!
//! The conformance harness (`harness` feature) depends on `plugin-conformance`,
//! whose fake engine links libembedpdf through `epdf-engine`. Cargo does not
//! propagate `cargo:rustc-link-arg` to a dependent package's test targets, so
//! without this line the tests link and then fail at load with
//! `Library not loaded: @rpath/libembedpdf.dylib`. Without the harness the
//! crate is pure and links nothing, so it needs no rpath.
fn main() {
    if std::env::var_os("CARGO_FEATURE_HARNESS").is_some() {
        build_support::emit_rpath_only();
    }
}
