//! `vectors/plugins/shell/*.json`, generated from `packages/plugin/shell`.

use epdf_plugin_shell::harness::{Shell, functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn the_reducer_matches_the_typescript() {
    assert_suite("plugins/shell/reducer.json", &Registry::new(), &functions());
}

#[test]
fn the_plugin_matches_the_typescript() {
    assert_suite("plugins/shell/plugin.json", &Registry::new().with(Shell), &functions());
}
