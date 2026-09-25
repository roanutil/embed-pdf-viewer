//! `vectors/plugins/selection*.json`, generated from `packages/plugin/selection`.

use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_selection::harness::{Selection, geometry_functions, handle_functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn selection_matches_the_typescript() {
    let registry = Registry::new().with(Interaction).with(Selection);
    assert_suite("plugins/selection.json", &registry, &Vec::new());
}

#[test]
fn selection_geometry_matches_the_typescript() {
    assert_suite("plugins/selection-geometry.json", &Registry::new(), &geometry_functions());
}

#[test]
fn selection_handles_match_the_typescript() {
    assert_suite("plugins/selection-handles.json", &Registry::new(), &handle_functions());
}
