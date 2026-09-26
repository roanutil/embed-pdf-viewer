//! `vectors/core/annotation-*.json`, generated from `packages/core/annotation`.

use epdf_annotation_core::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn geometry_basics_match_the_typescript() {
    replay("core/annotation-geometry-basics.json");
}

#[test]
fn geometry_shapes_match_the_typescript() {
    replay("core/annotation-geometry-shapes.json");
}

#[test]
fn geometry_hit_matches_the_typescript() {
    replay("core/annotation-geometry-hit.json");
}

#[test]
fn geometry_text_matches_the_typescript() {
    replay("core/annotation-geometry-text.json");
}

#[test]
fn endings_match_the_typescript() {
    replay("core/annotation-endings.json");
}

#[test]
fn cloudy_matches_the_typescript() {
    replay("core/annotation-cloudy.json");
}

#[test]
fn kinds_match_the_typescript() {
    replay("core/annotation-kinds.json");
}

#[test]
fn flags_match_the_typescript() {
    replay("core/annotation-flags.json");
}

#[test]
fn groups_match_the_typescript() {
    replay("core/annotation-groups.json");
}

#[test]
fn props_match_the_typescript() {
    replay("core/annotation-props.json");
}

#[test]
fn anchor_matches_the_typescript() {
    replay("core/annotation-anchor.json");
}

#[test]
fn snap_matches_the_typescript() {
    replay("core/annotation-snap.json");
}

#[test]
fn hit_matches_the_typescript() {
    replay("core/annotation-hit.json");
}

#[test]
fn scene_matches_the_typescript() {
    replay("core/annotation-scene.json");
}

#[test]
fn placement_matches_the_typescript() {
    replay("core/annotation-placement.json");
}

#[test]
fn view_matches_the_typescript() {
    replay("core/annotation-view.json");
}

#[test]
fn update_matches_the_typescript() {
    replay("core/annotation-update.json");
}

#[test]
fn measurement_matches_the_typescript() {
    replay("core/annotation-measurement.json");
}

#[test]
fn richtext_matches_the_typescript() {
    replay("core/annotation-richtext.json");
}

#[test]
fn measurement_model_matches_the_typescript() {
    replay("core/annotation-measurement-model.json");
}
