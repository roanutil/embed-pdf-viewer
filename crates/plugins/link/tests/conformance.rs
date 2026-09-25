//! `vectors/plugins/link*.json`, generated from `packages/plugin/link`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_link::harness::{self, Link};
use epdf_plugin_stage::harness::Stage;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Stage).with(Annotation).with(Link)
}

#[test]
fn scenarios_match_the_typescript() {
    assert_suite("plugins/link.json", &registry(), &Vec::new());
}

#[test]
fn functions_match_the_typescript() {
    assert_suite("plugins/link-functions.json", &Registry::new(), &harness::functions());
}
