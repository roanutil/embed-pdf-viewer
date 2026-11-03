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

#[test]
fn a_script_can_request_a_field_cursor() {
    let effect: epdf_acrojs::ScriptUiEffect = serde_json::from_value(serde_json::json!({
        "kind": "focusField", "field": "email"
    }))
    .expect("the field focus effect");
    assert_eq!(effect.kind(), "focusField");
    assert_eq!(
        serde_json::to_value(effect).unwrap(),
        serde_json::json!({ "kind": "focusField", "field": "email" })
    );
}
