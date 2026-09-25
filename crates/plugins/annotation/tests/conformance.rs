//! `vectors/plugins/annotation-*.json`, generated from `packages/plugin/annotation`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::{self, Annotation};
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_selection::harness::Selection;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Selection).with(Actions).with(Annotation)
}

fn scenarios(file: &str) {
    assert_suite(file, &registry(), &Vec::new());
}

#[test]
fn hydration_matches_the_typescript() {
    scenarios("plugins/annotation-hydration.json");
}

#[test]
fn crud_matches_the_typescript() {
    scenarios("plugins/annotation-crud.json");
}

#[test]
fn repository_matches_the_typescript() {
    assert_suite(
        "plugins/annotation-repository.json",
        &Registry::new(),
        &harness::repository_functions(),
    );
}

#[test]
fn pure_modules_match_the_typescript() {
    assert_suite("plugins/annotation-pure.json", &Registry::new(), &harness::pure_functions());
}
