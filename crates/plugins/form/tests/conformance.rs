//! `vectors/plugins/form-*.json`, generated from `packages/plugin/form`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::{self, Form};
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_render::harness::Render;
use plugin_conformance::{Registry, assert_suite, load};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Render).with(Annotation).with(Form)
}

fn scenarios(file: &str) {
    assert_suite(file, &registry(), &Vec::new());
}

#[test]
fn core_functions_match_the_typescript() {
    assert_suite("plugins/form-core.json", &Registry::new(), &harness::functions());
}

#[test]
fn core_lifecycle_matches_the_typescript() {
    scenarios("plugins/form-core-lifecycle.json");
}

#[test]
fn fill_matches_the_typescript() {
    scenarios("plugins/form-fill.json");
}

#[test]
fn design_matches_the_typescript() {
    scenarios("plugins/form-design.json");
}

#[test]
fn actions_integration_matches_the_typescript() {
    scenarios("plugins/form-actions.json");
}

#[test]
fn scripting_matches_the_typescript() {
    scenarios("plugins/form-scripting.json");
}

/// The six files hold 221 cases, every one replayed: a regenerated vector
/// that drops one is noticed.
#[test]
fn every_form_case_replays() {
    let counts: Vec<(&str, usize)> = [
        "plugins/form-actions.json",
        "plugins/form-core-lifecycle.json",
        "plugins/form-core.json",
        "plugins/form-design.json",
        "plugins/form-fill.json",
        "plugins/form-scripting.json",
    ]
    .into_iter()
    .map(|file| (file, load(file)["cases"].as_array().map_or(0, Vec::len)))
    .collect();
    assert_eq!(
        counts,
        vec![
            ("plugins/form-actions.json", 13),
            ("plugins/form-core-lifecycle.json", 3),
            ("plugins/form-core.json", 99),
            ("plugins/form-design.json", 29),
            ("plugins/form-fill.json", 31),
            ("plugins/form-scripting.json", 46),
        ]
    );
    assert_eq!(counts.iter().map(|(_, n)| n).sum::<usize>(), 221);
}
