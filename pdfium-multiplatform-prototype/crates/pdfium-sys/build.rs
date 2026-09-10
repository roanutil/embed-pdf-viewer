use std::env;
use std::path::PathBuf;

fn main() {
    let lib_root = build_support::lib_root();
    let target = build_support::fork_target();
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

    build_support::emit_link_directives(&dir);
}
