//! `vectors/plugins/metadata.json`, generated from `packages/plugin/metadata`.

use epdf_plugin_metadata::harness::Metadata;
use plugin_conformance::{Registry, assert_suite};

#[test]
fn metadata_matches_the_typescript() {
    assert_suite("plugins/metadata.json", &Registry::new().with(Metadata), &Vec::new());
}
