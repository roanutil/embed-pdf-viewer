//! `vectors/plugins/i18n/*.json`, generated from `packages/plugin/i18n`.

use epdf_plugin_i18n::harness::{I18n, functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn core_functions_match_the_typescript() {
    assert_suite("plugins/i18n/core.json", &Registry::new(), &functions());
}

#[test]
fn plural_categories_match_intl_plural_rules() {
    assert_suite("plugins/i18n/plurals.json", &Registry::new(), &functions());
}

#[test]
fn the_plugin_matches_the_typescript() {
    assert_suite("plugins/i18n/plugin.json", &Registry::new().with(I18n), &functions());
}
