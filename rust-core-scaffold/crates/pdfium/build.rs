//! Re-emits the rpath half of pdfium-sys/build.rs's link directives.
//!
//! Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
//! script to a dependent crate's own test/bin targets: only
//! `rustc-link-lib`/`rustc-link-search` are transitive. pdfium-sys's own
//! `-Wl,-rpath,...` therefore only takes effect when building pdfium-sys's
//! own targets, so without this, `cargo test -p pdfium` links against
//! libembedpdf fine but the resulting test binary cannot find the dylib at
//! runtime. This mirrors, rather than depends on, pdfium-sys/build.rs's
//! `lib_root`/`fork_target`/rpath logic, because crates/pdfium-sys is not
//! modified by this task.
use std::env;
use std::path::PathBuf;

fn main() {
    // wasm32-unknown-emscripten links libembedpdf.a directly into the em++
    // command line (see pdfium-sys/build.rs); nothing to do here.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }

    println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_LIB_DIR");
    println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_TARGET");

    let lib_root = match env::var("EPDF_SCAFFOLD_LIB_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => {
            PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..").join("build/libpdfium")
        }
    };
    let lib = lib_root.join(fork_target()).join("lib");

    if lib.join("libembedpdf.dylib").exists() || lib.join("libembedpdf.so").exists() {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
}

/// Copied from pdfium-sys/build.rs's `fork_target`, which this cannot import
/// because pdfium-sys does not expose it as a library function.
fn fork_target() -> String {
    if let Ok(target) = env::var("EPDF_SCAFFOLD_TARGET") {
        return target;
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    // aarch64-apple-ios-sim reports target_abi = "sim" with an EMPTY target_env,
    // so the simulator cannot be detected from target_env.
    let target_abi = env::var("CARGO_CFG_TARGET_ABI").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

    let platform = match os.as_str() {
        "macos" => "darwin",
        "ios" if target_abi == "sim" => "ios-sim",
        "ios" => "ios",
        "android" => "android",
        "linux" if target_env == "musl" => "linuxmusl",
        "linux" => "linux",
        "windows" => "win32",
        _ => panic!("unsupported target_os `{os}`; set EPDF_SCAFFOLD_TARGET"),
    };
    let arch = match arch.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => panic!("unsupported target_arch `{arch}`; set EPDF_SCAFFOLD_TARGET"),
    };
    format!("{platform}-{arch}")
}
