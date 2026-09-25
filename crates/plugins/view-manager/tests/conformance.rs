//! `vectors/plugins/view-manager/*.json`, generated from `packages/plugin/view-manager`.

use epdf_plugin_view_manager::harness::{ViewManager, functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn the_reducer_matches_the_typescript() {
    assert_suite("plugins/view-manager/reducer.json", &Registry::new(), &functions());
}

#[test]
fn the_plugin_matches_the_typescript() {
    assert_suite(
        "plugins/view-manager/plugin.json",
        &Registry::new().with(ViewManager),
        &functions(),
    );
}
