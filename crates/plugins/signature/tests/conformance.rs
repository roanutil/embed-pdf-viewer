//! `vectors/plugins/signature*.json`, generated from
//! `packages/plugin/signature`.

use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::Form;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_signature::harness::{self, Signature};
use epdf_plugin_stamp::harness::Stamp;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Annotation).with(Form).with(Stamp).with(Signature)
}

#[test]
fn the_plugin_matches_the_typescript() {
    assert_suite("plugins/signature.json", &registry(), &Vec::new());
}

#[test]
fn the_functions_match_the_typescript() {
    assert_suite("plugins/signature-functions.json", &Registry::new(), &harness::functions());
}
