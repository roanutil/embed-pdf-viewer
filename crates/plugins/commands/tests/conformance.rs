//! `vectors/plugins/commands/*.json`, generated from `packages/plugin/commands`.

use epdf_plugin_commands::harness::{Commands, functions};
use epdf_plugin_i18n::harness::I18n;
use epdf_plugin_shell::harness::Shell;
use plugin_conformance::{Registry, assert_suite};

#[test]
fn the_pure_functions_match_the_typescript() {
    assert_suite("plugins/commands/functions.json", &Registry::new(), &functions());
}

#[test]
fn the_plugin_matches_the_typescript() {
    let registry = Registry::new().with(Commands).with(I18n).with(Shell);
    assert_suite("plugins/commands/plugin.json", &registry, &functions());
}
