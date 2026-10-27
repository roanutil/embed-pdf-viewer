//! `vectors/plugins/outline.json`, generated from `packages/plugin/outline`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_history::harness::History;
use epdf_plugin_i18n::harness::I18n;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_link::harness::LinkAdapter;
use epdf_plugin_outline::harness::Outline;
use epdf_plugin_selection::harness::Selection;
use epdf_plugin_stage::harness::Stage;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new()
        .with(Interaction)
        .with(Actions)
        .with(Stage)
        .with(LinkAdapter)
        .with(Selection)
        .with(I18n)
        .with(History)
        .with(Outline)
}

#[test]
fn outline_matches_the_typescript() {
    assert_suite("plugins/outline.json", &registry(), &Vec::new());
}
