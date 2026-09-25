//! `vectors/plugins/render-*.json`, generated from `packages/plugin/render`
//! (and `engine-core`'s render-policy snaps).

use epdf_plugin_render::harness::{Render, functions};
use plugin_conformance::{Registry, assert_suite};

fn replay(file: &str) {
    assert_suite(file, &Registry::new().with(Render), &functions());
}

#[test]
fn tiles_match_the_typescript() {
    replay("plugins/render-tiles.json");
}

#[test]
fn strategy_matches_the_typescript() {
    replay("plugins/render-strategy.json");
}

#[test]
fn policy_snaps_match_the_typescript() {
    replay("plugins/render-policy.json");
}

#[test]
fn ledger_matches_the_typescript() {
    replay("plugins/render-ledger.json");
}

#[test]
fn capability_matches_the_typescript() {
    replay("plugins/render-capability.json");
}

#[test]
fn tile_plane_matches_the_typescript() {
    replay("plugins/render-tile-plane.json");
}
