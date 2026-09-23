//! Shared `build.rs` logic for the crates that link the fork's prebuilt
//! libembedpdf artifacts: resolving the per-target library directory and
//! emitting the link directives each artifact shape (Unix dylib, Windows
//! import library, static archive) needs.
//!
//! This is a `[build-dependencies]`-only crate. It is never a runtime
//! dependency of anything, only compiled into the build-script binaries of
//! `pdfium-sys` (which links libembedpdf) and `pdfium` (which depends on one
//! that does).
//!
//! Group G0 (multiplatform-port-order.md).

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// The shape of the prebuilt libembedpdf artifact at a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactShape {
    UnixDylib,
    WindowsImportLib,
    StaticArchive,
}

static EMIT_LIB_DIR_ONCE: Once = Once::new();
static EMIT_TARGET_ONCE: Once = Once::new();

/// Resolves the per-target library root.
///
/// The workspace does not fetch its own copy of the fork artifacts. It reads
/// the ones the shipping runtime package already fetches, so there is one
/// download location, one pin file, and one fetch script in the repo:
///
/// ```text
/// packages/engine/runtime/build/libpdfium/<fork target>/{include,lib}
/// bash packages/engine/runtime/scripts/fetch-libpdfium.sh <fork target>
/// ```
///
/// `PDF_RUNTIME_LIB_DIR` overrides it, and is the same variable that fetch
/// script honours (`scripts/fetch-libpdfium.sh:7`), so pointing one at a
/// different directory points both.
///
/// Cargo sets `CARGO_MANIFEST_DIR` in a build script's process environment to
/// the manifest directory of the package *whose build script is running*, not
/// of whatever crate that build script links in. So even though this code
/// lives in `crates/build-support`, reading `CARGO_MANIFEST_DIR` while
/// executing inside `crates/pdfium-sys`'s build.rs yields
/// `crates/pdfium-sys`, and `../..` from there is the workspace root.
pub fn lib_root() -> PathBuf {
    EMIT_LIB_DIR_ONCE.call_once(|| {
        println!("cargo:rerun-if-env-changed=PDF_RUNTIME_LIB_DIR");
    });

    if let Ok(dir) = env::var("PDF_RUNTIME_LIB_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .join("packages/engine/runtime/build/libpdfium")
}

/// The fork's target spelling, which is Node's rather than Rust's.
pub fn fork_target() -> String {
    EMIT_TARGET_ONCE.call_once(|| {
        println!("cargo:rerun-if-env-changed=EMBEDPDF_FORK_TARGET");
    });

    if let Ok(target) = env::var("EMBEDPDF_FORK_TARGET") {
        return target;
    }
    fork_target_from(
        &env::var("CARGO_CFG_TARGET_OS").unwrap_or_default(),
        &env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default(),
        &env::var("CARGO_CFG_TARGET_ABI").unwrap_or_default(),
        &env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default(),
    )
    .unwrap_or_else(|reason| panic!("{reason}; set EMBEDPDF_FORK_TARGET"))
}

/// The pure half of [`fork_target`], so the mapping is testable without a
/// build-script environment.
///
/// `aarch64-apple-ios-sim` reports `target_abi = "sim"` with an EMPTY
/// `target_env`, so the simulator cannot be told from the device by
/// `target_env` alone.
pub fn fork_target_from(os: &str, env: &str, abi: &str, arch: &str) -> Result<String, String> {
    if os == "emscripten" {
        return Ok("wasm32-eh".to_string());
    }
    let platform = match os {
        "macos" => "darwin",
        "ios" if abi == "sim" => "ios-sim",
        "ios" => "ios",
        "android" => "android",
        "linux" if env == "musl" => "linuxmusl",
        "linux" => "linux",
        "windows" => "win32",
        _ => return Err(format!("unsupported target_os `{os}`")),
    };
    let arch = match arch {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        _ => return Err(format!("unsupported target_arch `{arch}`")),
    };
    Ok(format!("{platform}-{arch}"))
}

/// Classifies a prebuilt libembedpdf artifact by the filenames in `lib`.
///
/// The fork ships a Unix dylib where `build-target.sh` sets
/// `pdf_is_complete_lib=false` (darwin, linux-gnu): `package-target.sh:29-36`
/// stages `lib/libembedpdf.{dylib,so}`. It ships a Windows import library plus
/// DLL for win32 (also `pdf_is_complete_lib=false`, but neither
/// `libembedpdf.dylib` nor `libembedpdf.so` exists there):
/// `package-target.sh:22-28` stages `lib/embedpdf.dll.lib` and
/// `bin/embedpdf.dll`, never a `libembedpdf.*`. Everything else is a static
/// archive, where `build-target.sh` sets `pdf_is_complete_lib=true` (wasm32,
/// linuxmusl). iOS and Android are both static: an Android `shared_library`
/// links empty, because Chromium's Android config exports nothing and
/// `--gc-sections` then drops all of PDFium, so the fork builds `android-*`
/// with `pdf_is_complete_lib=true` like every other static target.
pub fn classify_artifact(lib: &Path) -> ArtifactShape {
    if lib.join("libembedpdf.dylib").exists() || lib.join("libembedpdf.so").exists() {
        ArtifactShape::UnixDylib
    } else if lib.join("embedpdf.dll.lib").exists() {
        ArtifactShape::WindowsImportLib
    } else {
        ArtifactShape::StaticArchive
    }
}

/// Extra `clang` arguments `bindgen` needs to find a target's system headers.
///
/// The fork headers `#include <stdint.h>`, so `bindgen` needs a sysroot for
/// any target whose libc is not the host's. macOS and iOS share the Xcode SDK
/// that `libclang` finds on its own, so they need nothing. Android and
/// Emscripten do not, and without this `bindgen` fails with a wall of
/// `unknown type name 'uint32_t'`.
///
/// The prototype worked around this with 21 hand-written declarations. That is
/// not carried over: a sysroot flag is the fix, and hand-maintained
/// declarations drift from the headers they duplicate.
///
/// A sysroot alone is not enough. `libclang` defaults to the HOST triple, and
/// a host-triple clang looks for a host layout inside whatever sysroot it is
/// given: pointed at the emsdk sysroot it reports
/// `fatal error: 'time.h' file not found` even though
/// `cache/sysroot/include/time.h` is right there. It then returns what it
/// parsed before the fatal error, which for the fork headers is 147 constants
/// and not one function, and `bindgen` reports success. So the triple is
/// passed too, and [`assert_bindings_are_complete`] refuses the quiet version
/// of this failure.
pub fn sysroot_clang_args(target_os: &str) -> Vec<String> {
    let target = |args: Vec<String>| -> Vec<String> {
        let mut out = vec![format!("--target={}", env::var("TARGET").unwrap_or_default())];
        out.extend(args);
        out
    };
    match target_os {
        "android" => {
            println!("cargo:rerun-if-env-changed=ANDROID_NDK_HOME");
            println!("cargo:rerun-if-env-changed=ANDROID_NDK_ROOT");
            println!("cargo:rerun-if-env-changed=NDK_HOME");
            match android_sysroot() {
                Some(sysroot) => target(vec![format!("--sysroot={}", sysroot.display())]),
                None => panic!(
                    "building for Android needs the NDK sysroot: set ANDROID_NDK_HOME (or \
                     ANDROID_NDK_ROOT / NDK_HOME) to an NDK root, the one \
                     multiplatform-plan-claude-remix.md pins being 29.0.14206865"
                ),
            }
        }
        "emscripten" => {
            println!("cargo:rerun-if-env-changed=EMSDK");
            match std::env::var("EMSDK") {
                Ok(emsdk) => {
                    let sysroot = PathBuf::from(emsdk).join("upstream/emscripten/cache/sysroot");
                    target(vec![
                        format!("--sysroot={}", sysroot.display()),
                        // Without this, `bindgen` emits every type and not one
                        // function. `Function::parse` drops any cursor whose
                        // visibility is not `CXVisibility_Default`
                        // (`bindgen-0.72.1/ir/function.rs:735`), clang's implicit
                        // default on `wasm32-*` is hidden, and PDFium's
                        // `FPDF_EXPORT` expands to nothing unless
                        // `FPDF_IMPLEMENTATION` is defined
                        // (`fpdfview.h:216-220`), so the declarations carry no
                        // attribute to override it. Every other target this
                        // workspace builds for defaults to default visibility,
                        // which is why only wasm needs saying.
                        //
                        // This changes what bindgen reads, not what anything
                        // links: the flag reaches libclang, never the archive.
                        // `-DFPDF_IMPLEMENTATION` would also work and is worse,
                        // because that macro means "you are building PDFium".
                        "-fvisibility=default".to_string(),
                    ])
                }
                Err(_) => panic!(
                    "building for wasm32-unknown-emscripten needs the emsdk sysroot: set \
                     EMSDK to an activated emsdk root (the pin is 3.1.72)"
                ),
            }
        }
        _ => Vec::new(),
    }
}

/// Resolves an NDK root from the environment.
///
/// The three variables are what `cargo-ndk` and Gradle set between them. The
/// NDK ships only `darwin-x86_64` prebuilts on macOS, including on Apple
/// silicon, where they run under Rosetta.
fn android_sysroot() -> Option<PathBuf> {
    let root = ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME"]
        .iter()
        .find_map(|key| env::var(key).ok())
        .map(PathBuf::from)?;
    let host_tag = if cfg!(target_os = "macos") {
        "darwin-x86_64"
    } else if cfg!(target_os = "windows") {
        "windows-x86_64"
    } else {
        "linux-x86_64"
    };
    let sysroot = root.join("toolchains/llvm/prebuilt").join(host_tag).join("sysroot");
    sysroot.is_dir().then_some(sysroot)
}

/// Emits the full link setup for a crate that links libembedpdf directly:
/// search path, the library, the C++ runtime for a static link, and the Apple
/// frameworks. `dir` is `lib_root().join(fork_target())`.
///
/// `wasm32-unknown-emscripten` links nothing here: the wasm link step puts
/// `libembedpdf.a` on the `em++` command line alongside the Rust staticlib.
pub fn emit_link_directives(dir: &Path) {
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "emscripten" {
        return;
    }

    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());

    match classify_artifact(&lib) {
        ArtifactShape::UnixDylib => {
            println!("cargo:rustc-link-lib=dylib=embedpdf");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        }
        ArtifactShape::WindowsImportLib => {
            // MSVC's link.exe resolves `/DEFAULTLIB:NAME.lib` from whatever
            // name rustc-link-lib passes, so "embedpdf.dll" (not "embedpdf")
            // is what resolves to the fork's embedpdf.dll.lib. No rpath: PE
            // has no equivalent, and locating embedpdf.dll at run time is a
            // separate problem this build script does not solve.
            println!("cargo:rustc-link-lib=dylib=embedpdf.dll");
        }
        ArtifactShape::StaticArchive => {
            println!("cargo:rustc-link-lib=static=embedpdf");
            // build-target.sh:47-55 sets is_clang=false and
            // use_custom_libcxx=false for linuxmusl-x64 and linuxmusl-arm64,
            // so those artifacts are built with GCC against libstdc++, not
            // LLVM's libc++. The Android NDK sysroot provides libc++ under the
            // same link name as the Apple platforms.
            let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
            let cxx = cxx_runtime(&target_os, &target_env).unwrap_or_else(|| {
                panic!(
                    "no known C++ runtime for a static embedpdf link on \
                     target_os=`{target_os}` target_env=`{target_env}`"
                )
            });
            println!("cargo:rustc-link-lib=dylib={cxx}");
        }
    }

    for framework in apple_frameworks(&target_os) {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}

/// The C++ runtime a static libembedpdf needs on each platform.
pub fn cxx_runtime(target_os: &str, target_env: &str) -> Option<&'static str> {
    match (target_os, target_env) {
        ("macos", _) | ("ios", _) | ("android", _) => Some("c++"),
        ("linux", "musl") => Some("stdc++"),
        _ => None,
    }
}

/// Apple frameworks the fork's rendering paths need. iOS has no AppKit, but
/// CQuartz2D (`fx_apple_impl.o`) and `CPDF_Type1Font`'s glyph lookup still
/// need CoreGraphics and CoreFoundation there.
pub fn apple_frameworks(target_os: &str) -> &'static [&'static str] {
    match target_os {
        "macos" => &["AppKit", "CoreFoundation", "CoreGraphics", "Foundation"],
        "ios" => &["CoreFoundation", "CoreGraphics", "Foundation"],
        _ => &[],
    }
}

/// Re-emits just the rpath half of [`emit_link_directives`], for a crate that
/// depends on one linking libembedpdf rather than linking it itself.
///
/// Cargo does not propagate `cargo:rustc-link-arg` from a dependency's build
/// script to a dependent package's own bin and test targets; only
/// `rustc-link-lib` and `rustc-link-search` are transitive. Without this, a
/// dependent's test binary links against libembedpdf fine (that part is
/// transitive) but cannot find it at run time. Windows has no rpath
/// equivalent, so there is nothing to re-emit there.
pub fn emit_rpath_only() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }
    let lib = lib_root().join(fork_target()).join("lib");
    if classify_artifact(&lib) == ArtifactShape::UnixDylib {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    }
}

/// Refuses a `bindgen` run that produced types but no functions.
///
/// `bindgen` returns `Ok` after a fatal clang error, handing back whatever it
/// parsed first. For the fork headers that is 147 constants, 147 types and
/// zero functions, and the failure only surfaces hundreds of lines later as
/// `cannot find function ... in crate pdfium_sys`, pointing at the caller
/// rather than at the build script that produced an empty binding.
///
/// The fork exports 764 functions on the host at the pin this was written
/// against, so any plausible floor separates "parsed the headers" from "gave
/// up on the first `#include`".
pub fn assert_bindings_are_complete(bindings: &str, target: &str) {
    const FLOOR: usize = 600;
    let functions = bindings.matches("pub fn ").count();
    assert!(
        functions >= FLOOR,
        "bindgen produced {functions} functions for {target}, expected at least {FLOOR}.\n\
         That means clang stopped at a fatal error and bindgen returned what it had.\n\
         Reproduce the real diagnostic with:\n  \
         clang -fsyntax-only --target={target} <the same -I and --sysroot flags> \
         crates/pdfium-sys/wrapper.h"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rust_targets_to_the_forks_target_spelling() {
        let cases = [
            (("macos", "", "", "aarch64"), "darwin-arm64"),
            (("macos", "", "", "x86_64"), "darwin-x64"),
            (("ios", "", "", "aarch64"), "ios-arm64"),
            (("ios", "", "sim", "aarch64"), "ios-sim-arm64"),
            (("android", "", "", "aarch64"), "android-arm64"),
            (("android", "", "", "x86_64"), "android-x64"),
            (("linux", "gnu", "", "x86_64"), "linux-x64"),
            (("linux", "musl", "", "aarch64"), "linuxmusl-arm64"),
            (("windows", "msvc", "", "x86_64"), "win32-x64"),
            // Emscripten answers before arch is consulted: the fork ships one
            // wasm artifact and it is the exception-handling build.
            (("emscripten", "", "", "wasm32"), "wasm32-eh"),
        ];
        for ((os, env, abi, arch), expected) in cases {
            assert_eq!(fork_target_from(os, env, abi, arch).as_deref(), Ok(expected));
        }
    }

    #[test]
    fn refuses_targets_with_no_fork_artifact() {
        assert!(fork_target_from("freebsd", "", "", "x86_64").is_err());
        assert!(fork_target_from("linux", "gnu", "", "riscv64").is_err());
    }

    #[test]
    fn static_links_pick_the_cxx_runtime_the_artifact_was_built_against() {
        assert_eq!(cxx_runtime("macos", ""), Some("c++"));
        assert_eq!(cxx_runtime("ios", ""), Some("c++"));
        assert_eq!(cxx_runtime("android", ""), Some("c++"));
        assert_eq!(cxx_runtime("linux", "musl"), Some("stdc++"));
        assert_eq!(cxx_runtime("linux", "gnu"), None);
    }

    #[test]
    fn only_apple_targets_link_frameworks() {
        assert!(apple_frameworks("macos").contains(&"AppKit"));
        assert!(!apple_frameworks("ios").contains(&"AppKit"));
        assert!(apple_frameworks("ios").contains(&"CoreGraphics"));
        assert!(apple_frameworks("linux").is_empty());
    }

    #[test]
    fn the_wasm_parse_asks_for_default_visibility() {
        // The regression this guards is silent: without the flag `bindgen`
        // emits every type, no functions, and no diagnostic, and the failure
        // surfaces hundreds of lines later as "cannot find function" in a
        // crate that did nothing wrong. See the comment at the flag.
        unsafe {
            env::set_var("TARGET", "wasm32-unknown-emscripten");
            env::set_var("EMSDK", "/nonexistent-emsdk-root");
        }
        let args = sysroot_clang_args("emscripten");
        unsafe {
            env::remove_var("TARGET");
            env::remove_var("EMSDK");
        }
        assert!(args.iter().any(|a| a == "-fvisibility=default"), "{args:?}");
        assert!(
            args.iter().any(|a| a == "--target=wasm32-unknown-emscripten"),
            "a sysroot without a triple makes clang look for a host layout inside it: {args:?}"
        );
    }
}
