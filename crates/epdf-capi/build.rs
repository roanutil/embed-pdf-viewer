fn main() {
    // Test binaries use the source runtime; only the shipped cdylib uses relative lookup.
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target.as_str() {
        "linux" => println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,$ORIGIN"),
        "macos" => {
            println!("cargo:rustc-cdylib-link-arg=-Wl,-rpath,@loader_path");
            println!("cargo:rustc-cdylib-link-arg=-Wl,-install_name,@rpath/libepdf_capi.dylib");
        }
        _ => {}
    }
    let lib = build_support::lib_root().join(build_support::fork_target()).join("lib");
    if build_support::classify_artifact(&lib) == build_support::ArtifactShape::UnixDylib {
        println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{}", lib.display());
    }
}
