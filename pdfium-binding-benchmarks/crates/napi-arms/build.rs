use std::path::PathBuf;

fn main() {
    napi_build::setup();
    let lib_dir = lib_dir();
    println!("cargo:rerun-if-env-changed=PDFIUM_LIB_DIR");
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=embedpdf");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    // The absolute rpath above dies the moment the checkout moves or is
    // renamed (it did: build/out/native/napi-arms.node kept pointing into the
    // old `pdfium-rust-spike/` path after 8a8cdeca3, so `run.mjs --native`
    // passed its existsSync check and failed in dlopen). build-native.sh
    // copies libembedpdf.dylib next to the .node, and the fetched copy sits at
    // build/libpdfium/<target>/lib two levels up from build/out/native, so
    // both relative rpaths keep resolving wherever the tree lands.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,@loader_path");
        if let Some(target) = lib_dir.parent().and_then(|p| p.file_name()) {
            println!(
                "cargo:rustc-link-arg=-Wl,-rpath,@loader_path/../../libpdfium/{}/lib",
                target.to_string_lossy()
            );
        }
    }
}

/// Where `libembedpdf` lives.
///
/// `PDFIUM_LIB_DIR` still wins when it is set, which is how
/// `build/build-native.sh` points this at a target other than the host. It used
/// to be the ONLY answer, so a bare `cargo test` or `cargo check` at the
/// workspace root died in this build script with
/// `PDFIUM_LIB_DIR is required: NotPresent` before it compiled a line, and the
/// three crates that need no native library at all went down with it.
///
/// The fallback is the directory `build/fetch.sh` writes for the host, which is
/// the same directory `build-native.sh` passes anyway on the one target this
/// spike has ever been run on.
fn lib_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("PDFIUM_LIB_DIR") {
        return PathBuf::from(dir);
    }

    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let target = host_target();
    let dir = root.join("build/libpdfium").join(&target).join("lib");
    if !dir.is_dir() {
        panic!(
            "no libembedpdf for {target} at {}.\n\
             Fix with: bash build/fetch.sh {target}\n\
             Or point PDFIUM_LIB_DIR at a directory holding libembedpdf.",
            dir.display(),
        );
    }
    dir
}

/// The `<platform>-<arch>` names `engine-runtime-build.json` pins artifacts
/// under, which are Node's spellings rather than Rust's.
fn host_target() -> String {
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let platform = match (os.as_str(), env.as_str()) {
        ("macos", _) => "darwin",
        ("linux", "musl") => "linuxmusl",
        ("linux", _) => "linux",
        ("windows", _) => "win32",
        _ => panic!("unsupported target_os `{os}`; set PDFIUM_LIB_DIR"),
    };
    let arch = match arch.as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => panic!("unsupported target_arch `{arch}`; set PDFIUM_LIB_DIR"),
    };
    format!("{platform}-{arch}")
}
