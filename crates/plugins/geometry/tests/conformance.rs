//! `vectors/core/geometry-*.json`, generated from `packages/core/geometry`
//! and `engine-core`'s page-space and quad helpers.

use std::collections::BTreeSet;

use epdf_geometry::harness::functions;
use plugin_conformance::{Registry, assert_suite, load};

const FILES: [&str; 4] = [
    "core/geometry-basics.json",
    "core/geometry-affine.json",
    "core/geometry-page.json",
    "core/geometry-text-quad.json",
];

fn replay(file: &str) {
    assert_suite(file, &Registry::new(), &functions());
}

#[test]
fn basics_match_the_typescript() {
    replay(FILES[0]);
}

#[test]
fn affine_matches_the_typescript() {
    replay(FILES[1]);
}

#[test]
fn page_matches_the_typescript() {
    replay(FILES[2]);
}

#[test]
fn text_quad_matches_the_typescript() {
    replay(FILES[3]);
}

/// The table dispatches exactly the functions the vectors call: a retired
/// helper has no entry left to answer for it.
#[test]
fn the_table_names_only_what_the_vectors_call() {
    let called: BTreeSet<String> = FILES
        .iter()
        .flat_map(|file| {
            load(file)["cases"]
                .as_array()
                .expect("cases")
                .iter()
                .map(|case| case["fn"].as_str().expect("fn").to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    let table: BTreeSet<String> = functions().iter().map(|(name, _)| name.to_string()).collect();
    assert_eq!(functions().len(), table.len(), "duplicate table entry");
    assert_eq!(table, called);
}
