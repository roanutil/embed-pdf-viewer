//! `vectors/plugins/annotation-*.json`, generated from `packages/plugin/annotation`.

use epdf_plugin_annotation::harness;
use plugin_conformance::{Registry, assert_suite};

#[test]
fn repository_matches_the_typescript() {
    assert_suite(
        "plugins/annotation-repository.json",
        &Registry::new(),
        &harness::repository_functions(),
    );
}

#[test]
fn pure_modules_match_the_typescript() {
    assert_suite("plugins/annotation-pure.json", &Registry::new(), &harness::pure_functions());
}
