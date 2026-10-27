//! `vectors/plugins/history.json`, generated from `packages/plugin/history`.

use epdf_plugin_history::harness::History;
use plugin_conformance::{Registry, assert_suite};

#[test]
fn history_matches_the_typescript() {
    assert_suite("plugins/history.json", &Registry::new().with(History), &Vec::new());
}
