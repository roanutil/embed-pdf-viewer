//! `vectors/plugins/interaction*.json`, generated from `packages/plugin/interaction`.

use epdf_plugin_interaction::harness::{Interaction, functions};
use plugin_conformance::{Registry, assert_suite};

#[test]
fn interaction_matches_the_typescript() {
    assert_suite("plugins/interaction.json", &Registry::new().with(Interaction), &Vec::new());
}

#[test]
fn interaction_functions_match_the_typescript() {
    assert_suite("plugins/interaction-functions.json", &Registry::new(), &functions());
}
