//! Re-emits the rpath for this crate's own test binaries.
//!
//! The fake engine hydrates text layouts through `epdf-engine`, which links
//! libembedpdf. Cargo does not propagate `cargo:rustc-link-arg` to a
//! dependent package's test targets, so without this line the tests link and
//! then fail at load with `Library not loaded: @rpath/libembedpdf.dylib`.
//! On wasm32 the engine is not a dependency (see Cargo.toml), so there is
//! nothing to load.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }
    build_support::emit_rpath_only();
}
