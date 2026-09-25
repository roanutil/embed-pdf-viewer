//! `vectors/plugins/search*.json`, generated from `packages/plugin/search`.

use epdf_plugin_search::harness::{Search, functions};
use epdf_plugin_stage::harness::Stage;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Search).with(Stage)
}

#[test]
fn reducer_matches_the_typescript() {
    assert_suite("plugins/search-reducer.json", &Registry::new(), &functions());
}

#[test]
fn session_matches_the_typescript() {
    assert_suite("plugins/search.json", &registry(), &Vec::new());
}

#[test]
fn stage_integration_matches_the_typescript() {
    assert_suite("plugins/search-stage.json", &registry(), &Vec::new());
}

#[test]
fn find_all_matches_the_typescript() {
    assert_suite("plugins/search-find-all.json", &registry(), &Vec::new());
}
