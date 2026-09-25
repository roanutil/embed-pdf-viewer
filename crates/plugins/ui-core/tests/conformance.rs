//! `vectors/core/ui-*.json`, generated from `packages/core/ui`.

use epdf_ui_core::harness::functions;
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn schema_matches_the_typescript() {
    replay("core/ui-schema.json");
}

#[test]
fn solver_matches_the_typescript() {
    replay("core/ui-solver.json");
}

#[test]
fn projection_matches_the_typescript() {
    replay("core/ui-projection.json");
}

#[test]
fn transforms_match_the_typescript() {
    replay("core/ui-transforms.json");
}

#[test]
fn shortcuts_match_the_typescript() {
    replay("core/ui-shortcuts.json");
}

#[test]
fn helpers_match_the_typescript() {
    replay("core/ui-helpers.json");
}
