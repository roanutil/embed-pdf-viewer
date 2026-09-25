//! `vectors/core/stage-*.json`, generated from `packages/core/stage`.

use epdf_stage_core::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn camera_matches_the_typescript() {
    replay("core/stage-camera.json");
}

#[test]
fn layout_matches_the_typescript() {
    replay("core/stage-layout.json");
}

#[test]
fn anchor_matches_the_typescript() {
    replay("core/stage-anchor.json");
}
