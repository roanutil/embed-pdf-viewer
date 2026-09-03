use std::env;
use std::path::PathBuf;

fn main() {
    let lib_root = lib_root();
    let target = fork_target();
    let dir = lib_root.join(&target);
    let include = dir.join("include");

    if !include.is_dir() {
        panic!(
            "no libembedpdf headers for {target} at {}.\n\
             Fix with: bash build/fetch-libpdfium.sh {target}",
            include.display()
        );
    }

    println!("cargo:rerun-if-changed=wrapper.h");
    // wrapper.h #includes 30 headers that live under here, populated by
    // build/fetch-libpdfium.sh. A directory path is enough: Cargo walks it.
    println!("cargo:rerun-if-changed={}", include.display());
    println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_LIB_DIR");
    // fork_target() reads this directly; without tracking it, flipping it
    // alone (no wrapper.h touch) would not trigger regeneration once any
    // rerun-if-* directive is emitted.
    println!("cargo:rerun-if-env-changed=EPDF_SCAFFOLD_TARGET");

    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include.display()))
        // The same filter packages/engine/runtime/build/generate-functions.mjs
        // applies: /^(?:FPDF|EPDF|FORM|PDFiumExt_)/.
        .allowlist_function("^(FPDF|EPDF|FORM|PDFiumExt_).*")
        .allowlist_type("^(FPDF|EPDF|FORM|FS_|FX_|PDFiumExt_|IPDF_|fpdf_).*")
        .allowlist_var("^(FPDF|EPDF|FORM|FPDFBitmap_|FPDF_ANNOT|FWL_).*")
        .derive_debug(true)
        .derive_default(true)
        .derive_copy(true)
        .layout_tests(true)
        .generate()
        .expect("bindgen failed over the fork headers");

    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("write bindings.rs");

    emit_link_directives(&dir);
}

/// wasm32-unknown-emscripten links nothing here: build/link-wasm.sh puts
/// libembedpdf.a on the em++ command line alongside the Rust staticlib, exactly
/// as pdfium-rust-spike/build/link-wasm.sh does.
fn emit_link_directives(dir: &PathBuf) {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("emscripten") {
        return;
    }

    let lib = dir.join("lib");
    println!("cargo:rustc-link-search=native={}", lib.display());

    // The fork ships a dylib where build-target.sh sets pdf_is_complete_lib=false
    // (darwin, linux-gnu, win32) and a static archive where it sets true (wasm32,
    // linuxmusl). iOS will be static; Android will be a .so.
    if lib.join("libembedpdf.dylib").exists() || lib.join("libembedpdf.so").exists() {
        println!("cargo:rustc-link-lib=dylib=embedpdf");
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
    } else {
        println!("cargo:rustc-link-lib=static=embedpdf");
        // build-target.sh:47-55 sets is_clang=false and use_custom_libcxx=false
        // for linuxmusl-x64 and linuxmusl-arm64, so those artifacts are built
        // with GCC against libstdc++, not LLVM's libc++. Apple targets (macOS,
        // iOS) use libc++.
        let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
        let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        let cxx_runtime = match (target_os.as_str(), target_env.as_str()) {
            ("macos", _) | ("ios", _) => "c++",
            ("linux", "musl") => "stdc++",
            _ => panic!(
                "no known C++ runtime for static embedpdf link on \
                 target_os=`{target_os}` target_env=`{target_env}`"
            ),
        };
        println!("cargo:rustc-link-lib=dylib={cxx_runtime}");
    }

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        for framework in ["AppKit", "CoreFoundation", "CoreGraphics", "Foundation"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}

fn lib_root() -> PathBuf {
    if let Ok(dir) = env::var("EPDF_SCAFFOLD_LIB_DIR") {
        return PathBuf::from(dir);
    }
    PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../..")
        .join("build/libpdfium")
}

/// The fork's target spelling, which is Node's rather than Rust's. Copied from
/// pdfium-rust-spike/crates/napi-arms/build.rs:56-78 and extended with the two
/// mobile OSes.
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

    if os == "emscripten" {
        return "wasm32".to_string();
    }

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
