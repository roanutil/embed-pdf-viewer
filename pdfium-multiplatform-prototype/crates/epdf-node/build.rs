fn main() {
    napi_build::setup();
    // Cargo does not propagate cargo:rustc-link-arg from a dependency's build
    // script, so the addon needs its own rpath to find libembedpdf at runtime.
    build_support::emit_rpath_only();
}
