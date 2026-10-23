//! `vectors/plugins/render-*.json`, generated from `packages/plugin/render`
//! (and `engine-core`'s render-policy snaps).

use epdf_plugin_render::harness::{Render, functions};
use std::collections::BTreeSet;

use plugin_conformance::{Registry, assert_suite, load};

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

/// The policy suite's 31 refusals are part of its contract: a scale
/// viewport snapped to a width lattice without a page width, and an empty
/// width, appearance or tile-scale lattice, each refused as the engine
/// refuses it. The replay compares every refusal; this pins how many there
/// are and why, so a regenerated vector that drops one is noticed.
#[test]
fn the_policy_snaps_refuse_31_cases() {
    let suite = load("plugins/render-policy.json");
    let refusals: Vec<(String, String)> = suite["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter_map(|case| {
            let threw = case["outcome"].get("threw")?;
            let message = threw["$error"]["message"].as_str().unwrap_or_default();
            Some((case["name"].as_str().unwrap_or_default().to_string(), message.to_string()))
        })
        .collect();
    assert_eq!(refusals.len(), 31, "{refusals:?}");
    let reasons: BTreeSet<&str> = refusals.iter().map(|(_, message)| message.as_str()).collect();
    assert_eq!(
        reasons,
        BTreeSet::from([
            "render policy has an empty appearance-scale lattice",
            "render policy has an empty tile-scale lattice",
            "render policy has an empty width ladder",
            "snapping a scale viewport to a width lattice requires opts.pageWidth",
        ])
    );
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
