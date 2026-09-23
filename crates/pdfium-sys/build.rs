use std::env;
use std::path::PathBuf;

fn main() {
    let dir = build_support::lib_root().join(build_support::fork_target());
    let include = dir.join("include");

    if !include.is_dir() {
        panic!(
            "no libembedpdf headers at {}.\n\
             Fix with: bash packages/engine/runtime/scripts/fetch-libpdfium.sh {}",
            include.display(),
            build_support::fork_target(),
        );
    }

    println!("cargo:rerun-if-changed=wrapper.h");
    // wrapper.h #includes 30 headers under here, populated by the runtime
    // package's fetch script. A directory path is enough: Cargo walks it.
    println!("cargo:rerun-if-changed={}", include.display());
    // lib_root() and fork_target() read these directly. Without tracking them,
    // flipping one alone (no wrapper.h touch) would not regenerate once any
    // rerun-if-* directive has been emitted.
    println!("cargo:rerun-if-env-changed=PDF_RUNTIME_LIB_DIR");
    println!("cargo:rerun-if-env-changed=EMBEDPDF_FORK_TARGET");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let bindings = bindgen::Builder::default()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include.display()))
        .clang_args(build_support::sysroot_clang_args(&target_os))
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

    build_support::assert_bindings_are_complete(
        &bindings.to_string(),
        &env::var("TARGET").unwrap_or_default(),
    );

    bindings
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("write bindings.rs");

    build_support::emit_link_directives(&dir);
}
