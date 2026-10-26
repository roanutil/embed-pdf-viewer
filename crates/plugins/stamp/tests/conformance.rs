//! `vectors/plugins/stamp-*.json`, generated from `packages/plugin/stamp`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::Form;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_stamp::harness::{self, Stamp};
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Annotation).with(Form).with(Stamp)
}

#[test]
fn library_import_matches_the_typescript() {
    assert_suite("plugins/stamp-library.json", &registry(), &Vec::new());
}

#[test]
fn assets_match_the_typescript() {
    assert_suite("plugins/stamp-assets.json", &registry(), &Vec::new());
}

#[test]
fn placement_matches_the_typescript() {
    assert_suite("plugins/stamp-placement.json", &registry(), &Vec::new());
}

#[test]
fn the_current_surface_matches_the_typescript() {
    assert_suite("plugins/stamp-api.json", &registry(), &Vec::new());
}

#[test]
fn transitions_match_the_typescript() {
    assert_suite("plugins/stamp-functions.json", &Registry::new(), &harness::functions());
}
