//! `vectors/core/ui-*.json`, generated from `packages/core/ui`.

use epdf_ui_core::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn schema_matches_the_typescript() {
    replay("core/ui-schema.json");
}

#[test]
fn solver_matches_the_typescript() {
    replay("core/ui-solver.json");
}

#[test]
fn projection_matches_the_typescript() {
    replay("core/ui-projection.json");
}

#[test]
fn transforms_match_the_typescript() {
    replay("core/ui-transforms.json");
}

#[test]
fn shortcuts_match_the_typescript() {
    replay("core/ui-shortcuts.json");
}

#[test]
fn helpers_match_the_typescript() {
    replay("core/ui-helpers.json");
}

/// The layout is plain data with JavaScript's member order, so its cases
/// match the TypeScript's object member order without any entry in
/// `member-order-allow.json`: every case of the two layout files, and the
/// layout cases of `ui-schema.json`, compared with no allowance.
#[test]
fn layout_cases_keep_member_order_with_no_allowance() {
    let table = functions();
    for file in ["core/ui-transforms.json", "core/ui-helpers.json", "core/ui-schema.json"] {
        let suite = plugin_conformance::load(file);
        for case in suite["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().unwrap_or("?");
            let fn_name = case["fn"].as_str().unwrap_or("?");
            if file.ends_with("ui-schema.json") && !fn_name.contains("Layout") {
                continue;
            }
            let (_, f) = table.iter().find(|(n, _)| *n == fn_name).expect("a function");
            let args = case["args"].as_array().cloned().unwrap_or_default();
            let returned = f(&args).unwrap_or_else(|e| panic!("{file} {name}: {}", e.message));
            let actual = serde_json::json!({ "returned": plugin_conformance::scrub(&returned) });
            assert!(plugin_conformance::canonical::member_order_checked());
            if let Some(diff) = plugin_conformance::canonical::diff(&case["outcome"], &actual) {
                panic!("{file} {name}: {diff}");
            }
        }
    }
}
