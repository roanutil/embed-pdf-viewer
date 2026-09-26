//! `vectors/plugins/measurement*.json`, generated from
//! `packages/plugin/measurement`.

use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_measurement::harness::{self, MeasurementAdapter};
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Annotation).with(MeasurementAdapter)
}

#[test]
fn scenarios_match_the_typescript() {
    assert_suite("plugins/measurement.json", &registry(), &Vec::new());
}

#[test]
fn functions_match_the_typescript() {
    assert_suite("plugins/measurement-functions.json", &Registry::new(), &harness::functions());
}
