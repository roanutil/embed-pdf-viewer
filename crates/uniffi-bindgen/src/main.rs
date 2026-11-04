//! Binding generator for `epdf-uniffi`, pinned to the same uniffi version.
//!
//! `uniffi-bindgen generate --library` reads the metadata uniffi's proc macros
//! bake into the built library, and that reader has to be the same version
//! that wrote it. A `cargo install uniffi-bindgen` from the registry would
//! drift the moment the workspace pin moved, so the generator is a workspace
//! binary sharing the pin in `crates/epdf-uniffi/Cargo.toml`.
mod kotlin;

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    uniffi::uniffi_bindgen_main();
    if let Some(directory) = kotlin::output_directory(&arguments) {
        match kotlin::fix_external_error_buffers(&directory) {
            Ok(0) => {}
            Ok(changed) => {
                eprintln!("Adapted generated Kotlin ABI declarations in {changed} generated files")
            }
            Err(error) => {
                eprintln!("Cannot adapt Kotlin callback error buffers: {error}");
                std::process::exit(1);
            }
        }
    }
}
