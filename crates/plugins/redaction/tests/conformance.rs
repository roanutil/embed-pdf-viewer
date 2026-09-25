//! `vectors/plugins/redaction*.json`, generated from `packages/plugin/redaction`.

use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_redaction::harness::{self, Redaction};
use epdf_plugin_selection::harness::Selection;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Selection).with(Annotation).with(Redaction)
}

#[test]
fn scenarios_match_the_typescript() {
    assert_suite("plugins/redaction.json", &registry(), &Vec::new());
}

#[test]
fn functions_match_the_typescript() {
    assert_suite("plugins/redaction-functions.json", &Registry::new(), &harness::functions());
}
