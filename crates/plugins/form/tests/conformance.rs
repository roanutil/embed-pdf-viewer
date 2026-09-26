//! `vectors/plugins/form-*.json`, generated from `packages/plugin/form`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::{self, Form};
use epdf_plugin_interaction::harness::Interaction;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Annotation).with(Form)
}

fn scenarios(file: &str) {
    assert_suite(file, &registry(), &Vec::new());
}

#[test]
fn pure_modules_match_the_typescript() {
    assert_suite("plugins/form-core.json", &Registry::new(), &harness::functions());
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
