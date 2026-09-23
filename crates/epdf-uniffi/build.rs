//! Re-emits the rpath for this crate's own test binaries, and runs uniffi's
//! build-time scaffolding hook.
//!
//! Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
//! script to a dependent package's test and bin targets, so without the rpath
//! line `cargo test -p epdf-uniffi` links and then aborts at load. Same reason
//! as `crates/pdfium` and `crates/epdf-engine`.
fn main() {
    build_support::emit_rpath_only();
}
