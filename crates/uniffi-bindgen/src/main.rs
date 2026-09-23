//! Binding generator for `epdf-uniffi`, pinned to the same uniffi version.
//!
//! `uniffi-bindgen generate --library` reads the metadata uniffi's proc macros
//! bake into the built library, and that reader has to be the same version
//! that wrote it. A `cargo install uniffi-bindgen` from the registry would
//! drift the moment the workspace pin moved, so the generator is a workspace
//! binary sharing the pin in `crates/epdf-uniffi/Cargo.toml`.
fn main() {
    uniffi::uniffi_bindgen_main()
}
