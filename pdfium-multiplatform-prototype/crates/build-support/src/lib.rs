//! Shared build-script logic for crates that build against the fork's
//! prebuilt libembedpdf artifacts: resolving the fork's per-target library
//! directory and emitting the link directives each of the fork's artifact
//! shapes (Unix dylib, Windows import library, static archive) needs.
//!
//! This is a `[build-dependencies]`-only crate: it is never a runtime
//! dependency of anything, only compiled into the build-script binaries of
//! crates that link libembedpdf (pdfium-sys) or depend on one that does
//! (pdfium).
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// The shape of the prebuilt libembedpdf artifact at a target: Unix dylib,
/// Windows import library, or static archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactShape {
    UnixDylib,
    WindowsImportLib,
    StaticArchive,
}

static EMIT_LIB_DIR_ONCE: Once = Once::new();
static EMIT_TARGET_ONCE: Once = Once::new();

/// Resolves the fork's per-target library root: `build/libpdfium` relative to
/// the scaffold root, or `EPDF_SCAFFOLD_LIB_DIR` when that's set.
///
/// Cargo sets `CARGO_MANIFEST_DIR` in a build script's process environment to
/// the manifest directory of the package *whose build script is running*,
/// not of whatever crate that build script happens to link in. So even
/// though this function's code lives in crates/build-support, reading
/// `CARGO_MANIFEST_DIR` while executing inside crates/pdfium-sys's build.rs
/// process yields crates/pdfium-sys, and `../..` from there is still the
/// scaffold root — confirmed by `cargo test -p pdfium-sys -p pdfium` after
/// wiring both build scripts through this crate.
pub fn lib_root() -> PathBuf {
    EMIT_LIB_DIR_ONCE.call_once(|| {
        println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_LIB_DIR");
    });

    if let Ok(dir) = env::var("EPDF_SCAFFOLD_LIB_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .join("build/libpdfium")
}

/// The fork's target spelling, which is Node's rather than Rust's. Copied from
/// pdfium-binding-benchmarks/crates/napi-arms/build.rs:56-78 and extended with the two
/// mobile OSes.
pub fn fork_target() -> String {
    EMIT_TARGET_ONCE.call_once(|| {
        println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_TARGET");
    });

    if let Ok(target) = env::var("EPDF_SCAFFOLD_TARGET") {
        return target;
    }
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if os == "emscripten" {
        return "wasm32-eh".to_string();
    }

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

/// Classifies a prebuilt libembedpdf artifact by its filename pattern in the
/// provided library directory. Inspects artifact filenames exactly once in the file.
pub fn classify_artifact(lib: &Path) -> ArtifactShape {
    // The fork ships a Unix dylib where build-target.sh sets
    // pdf_is_complete_lib=false (darwin, linux-gnu): package-target.sh:29-36
    // stages lib/libembedpdf.{dylib,so}. It ships a Windows import library +
    // DLL for win32 (also pdf_is_complete_lib=false, but neither
    // libembedpdf.dylib nor libembedpdf.so exists there):
    // package-target.sh:22-28 stages lib/embedpdf.dll.lib and
    // bin/embedpdf.dll, never a libembedpdf.* file. Everything else is a
    // static archive, where build-target.sh sets pdf_is_complete_lib=true
    // (wasm32, linuxmusl). iOS and Android are both static: an Android
    // shared_library links empty, because Chromium's Android config exports
    // nothing and --gc-sections then drops all of PDFium, so the fork builds
    // android-* with pdf_is_complete_lib=true like every other static target.
    if lib.join("libembedpdf.dylib").exists() || lib.join("libembedpdf.so").exists() {
        ArtifactShape::UnixDylib
    } else if lib.join("embedpdf.dll.lib").exists() {
        ArtifactShape::WindowsImportLib
    } else {
        ArtifactShape::StaticArchive
    }
}

/// Emits the full link setup for a crate that links libembedpdf directly:
/// search path, the library itself, the C++ runtime for a static link, and
/// macOS frameworks. `dir` is `lib_root().join(fork_target())`.
///
/// wasm32-unknown-emscripten links nothing here: build/link-wasm.sh puts
/// libembedpdf.a on the em++ command line alongside the Rust staticlib, exactly
/// as pdfium-binding-benchmarks/build/link-wasm.sh does.
pub fn emit_link_directives(dir: &Path) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }

    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let shape = classify_artifact(&lib);

    match shape {
        ArtifactShape::UnixDylib => {
            println!("cargo:rustc-link-lib=dylib=embedpdf");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        }
        ArtifactShape::WindowsImportLib => {
            // MSVC's link.exe resolves a `/DEFAULTLIB:NAME.lib` from whatever name
            // rustc-link-lib passes, so "embedpdf.dll" (not "embedpdf") is what
            // resolves to the fork's embedpdf.dll.lib import library. No rpath:
            // PE has no equivalent, and locating embedpdf.dll at runtime (PATH,
            // or copying it next to the binary) is a separate problem this build
            // script does not solve.
            println!("cargo:rustc-link-lib=dylib=embedpdf.dll");
        }
        ArtifactShape::StaticArchive => {
            println!("cargo:rustc-link-lib=static=embedpdf");
            // build-target.sh:47-55 sets is_clang=false and use_custom_libcxx=false
            // for linuxmusl-x64 and linuxmusl-arm64, so those artifacts are built
            // with GCC against libstdc++, not LLVM's libc++. Apple targets (macOS,
            // iOS) use libc++.
            let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
            let cxx_runtime = match (target_os.as_str(), target_env.as_str()) {
                ("macos", _) | ("ios", _) => "c++",
                ("linux", "musl") => "stdc++",
                // The Android NDK's sysroot provides libc++ as `c++` (either
                // libc++_shared.so or, for a static link, the NDK's libc++.a),
                // the same link name as the two Apple platforms above.
                ("android", _) => "c++",
                _ => panic!(
                    "no known C++ runtime for static embedpdf link on \
                     target_os=`{target_os}` target_env=`{target_env}`"
                ),
            };
            println!("cargo:rustc-link-lib=dylib={cxx_runtime}");
        }
    }

    if target_os == "macos" {
        for framework in ["AppKit", "CoreFoundation", "CoreGraphics", "Foundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else if target_os == "ios" {
        // No AppKit on iOS; CQuartz2D (fx_apple_impl.o) and CPDF_Type1Font's
        // glyph lookup still need CoreGraphics and CoreFoundation on this
        // platform too, same as macOS.
        for framework in ["CoreFoundation", "CoreGraphics", "Foundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}

/// Re-emits just the rpath half of [`emit_link_directives`], for a crate that
/// depends on one linking libembedpdf directly rather than linking it itself.
///
/// Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
/// script to a dependent package's own bin/test targets; only
/// `rustc-link-lib`/`rustc-link-search` are transitive. So without this, a
/// dependent crate's test binary links against libembedpdf fine (that part
/// *is* transitive) but cannot find it at runtime. Windows has no rpath
/// equivalent, so there is nothing to re-emit there.
pub fn emit_rpath_only() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }

    let lib = lib_root().join(fork_target()).join("lib");
    let shape = classify_artifact(&lib);

    if shape == ArtifactShape::UnixDylib {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
}
