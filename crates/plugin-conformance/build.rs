//! Re-emits the rpath for this crate's own test binaries.
//!
//! The fake engine hydrates text layouts through `epdf-engine`, which links
//! libembedpdf. Cargo does not propagate `cargo:rustc-link-arg` to a
//! dependent package's test targets, so without this line the tests link and
//! then fail at load with `Library not loaded: @rpath/libembedpdf.dylib`.
fn main() {
    build_support::emit_rpath_only();
}
