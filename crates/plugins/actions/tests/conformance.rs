//! `vectors/plugins/actions*.json`, generated from `packages/plugin/actions`.

use epdf_plugin_actions::harness::{Actions, functions};
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Actions)
}

#[test]
fn functions_match_the_typescript() {
    assert_suite("plugins/actions.json", &Registry::new(), &functions());
}

#[test]
fn dispatcher_matches_the_typescript() {
    assert_suite("plugins/actions-dispatcher.json", &registry(), &Vec::new());
}

#[test]
fn submit_matches_the_typescript() {
    assert_suite("plugins/actions-submit.json", &registry(), &Vec::new());
}

#[test]
fn triggers_match_the_typescript() {
    assert_suite("plugins/actions-triggers.json", &registry(), &Vec::new());
}

#[test]
fn lifecycle_matches_the_typescript() {
    assert_suite("plugins/actions-lifecycle.json", &registry(), &Vec::new());
}

#[test]
fn script_matches_the_typescript() {
    assert_suite("plugins/actions-script.json", &registry(), &Vec::new());
}

#[test]
fn hover_matches_the_typescript() {
    assert_suite("plugins/actions-hover.json", &registry(), &Vec::new());
}
