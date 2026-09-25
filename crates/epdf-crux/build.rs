//! Re-emits the rpath for this crate's own test and bin targets, for the
//! reason given in `crates/epdf-uniffi/build.rs`.
fn main() {
    build_support::emit_rpath_only();
}
