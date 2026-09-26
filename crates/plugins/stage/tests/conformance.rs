//! `vectors/plugins/stage*.json`, generated from `packages/plugin/stage`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_stage::harness::{Stage, functions};
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Stage).with(Interaction).with(Actions)
}

#[test]
fn destinations_match_the_typescript() {
    assert_suite("plugins/stage-destination.json", &Registry::new(), &functions());
}

#[test]
fn pure_modules_match_the_typescript() {
    assert_suite("plugins/stage-pure.json", &Registry::new(), &functions());
}

#[test]
fn capability_matches_the_typescript() {
    assert_suite("plugins/stage-capability.json", &registry(), &Vec::new());
}

#[test]
fn motion_matches_the_typescript() {
    assert_suite("plugins/stage-motion.json", &registry(), &Vec::new());
}

#[test]
fn feeds_match_the_typescript() {
    assert_suite("plugins/stage-feeds.json", &registry(), &Vec::new());
}

#[test]
fn change_hooks_match_the_typescript() {
    assert_suite("plugins/stage-events.json", &registry(), &Vec::new());
}
