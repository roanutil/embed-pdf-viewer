fn main() {
    napi_build::setup();
    let lib_dir = std::env::var("PDFIUM_LIB_DIR").expect("PDFIUM_LIB_DIR is required");
    println!("cargo:rustc-link-search=native={lib_dir}");
    println!("cargo:rustc-link-lib=dylib=embedpdf");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{lib_dir}");
}
