//! `vectors/plugins/page-edit.json`, generated from `packages/plugin/page-edit`.

use epdf_plugin_page_edit::harness::PageEdit;
use plugin_conformance::{Registry, assert_suite};

#[test]
fn page_edit_matches_the_typescript() {
    assert_suite("plugins/page-edit.json", &Registry::new().with(PageEdit), &Vec::new());
}
