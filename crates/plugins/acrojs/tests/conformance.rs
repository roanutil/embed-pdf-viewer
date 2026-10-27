//! `vectors/core/acrojs*.json`, generated from `packages/core/acrojs`.

use epdf_acrojs::harness::{ScriptHostHarness, functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn acrojs_functions_match_the_typescript() {
    assert_suite("core/acrojs.json", &Registry::new(), &functions());
}

#[test]
fn acrojs_host_matches_the_typescript() {
    assert_suite("core/acrojs-host.json", &Registry::new().with(ScriptHostHarness), &Vec::new());
}

#[test]
fn prelude_is_the_generated_file() {
    // The vector pins the text; this pins that the crate ships that file.
    let on_disk =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/prelude.js")).unwrap();
    assert_eq!(epdf_acrojs::PRELUDE_SOURCE, on_disk);
    assert!(epdf_acrojs::PRELUDE_SOURCE.starts_with("(function installAcroJs(globalObject)"));
}
