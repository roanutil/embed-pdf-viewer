//! Re-emits the rpath for this crate's own test binaries.
//!
//! The conformance harness (`harness` feature) depends on `plugin-conformance`,
//! whose fake engine links libembedpdf through `epdf-engine`. Cargo does not
//! propagate `cargo:rustc-link-arg` to a dependent package's test targets, so
//! without the rpath the tests link and then fail at load with
//! `Library not loaded: @rpath/libembedpdf.dylib`. On wasm32 the engine is not
//! in the graph, so there is nothing to load.
//!
//! This is `build_support::emit_rpath_only` with the library root resolved
//! from this crate's depth: `build_support::lib_root` joins
//! `CARGO_MANIFEST_DIR/../..`, which is the repository root for
//! `crates/<name>` but not for `crates/plugins/<name>`.

use std::path::PathBuf;

use build_support::{ArtifactShape, classify_artifact, fork_target};

fn main() {
    println!("cargo:rerun-if-env-changed=PDF_RUNTIME_LIB_DIR");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32")
        || std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten")
    {
        return;
    }
    let root = std::env::var_os("PDF_RUNTIME_LIB_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("packages/engine/runtime/build/libpdfium")
    });
    let lib = root.join(fork_target()).join("lib");
    if classify_artifact(&lib) == ArtifactShape::UnixDylib {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
}
