//! `vectors/core/geometry-*.json`, generated from `packages/core/geometry`.

use epdf_geometry::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn basics_match_the_typescript() {
    replay("core/geometry-basics.json");
}

#[test]
fn affine_matches_the_typescript() {
    replay("core/geometry-affine.json");
}

#[test]
fn page_matches_the_typescript() {
    replay("core/geometry-page.json");
}

#[test]
fn text_quad_matches_the_typescript() {
    replay("core/geometry-text-quad.json");
}
