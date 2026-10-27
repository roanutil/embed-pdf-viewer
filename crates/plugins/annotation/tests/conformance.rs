//! `vectors/plugins/annotation-*.json`, generated from `packages/plugin/annotation`.

use epdf_plugin_annotation::harness::{Annotation, pure_functions, repository_functions};
use plugin_conformance::{Registry, assert_suite, load};

fn registry() -> Registry {
    Registry::new()
        .with(epdf_plugin_interaction::harness::Interaction)
        .with(epdf_plugin_selection::harness::Selection)
        .with(epdf_plugin_actions::harness::Actions)
        .with(epdf_plugin_render::harness::Render)
        .with(Annotation)
}

fn replay(file: &str) {
    assert_suite(file, &registry(), &Vec::new());
}

#[test]
fn api_matches_the_typescript() {
    replay("plugins/annotation-api.json");
}

#[test]
fn comments_match_the_typescript() {
    replay("plugins/annotation-comments.json");
}

#[test]
fn crud_matches_the_typescript() {
    replay("plugins/annotation-crud.json");
}

#[test]
fn grouping_matches_the_typescript() {
    replay("plugins/annotation-grouping.json");
}

#[test]
fn hover_matches_the_typescript() {
    replay("plugins/annotation-hover.json");
}

#[test]
fn hydration_matches_the_typescript() {
    replay("plugins/annotation-hydration.json");
}

#[test]
fn markup_matches_the_typescript() {
    replay("plugins/annotation-markup.json");
}

#[test]
fn reconciliation_matches_the_typescript() {
    replay("plugins/annotation-reconciliation.json");
}

#[test]
fn tools_match_the_typescript() {
    replay("plugins/annotation-tools.json");
}

#[test]
fn pure_functions_match_the_typescript() {
    assert_suite("plugins/annotation-pure.json", &Registry::new(), &pure_functions());
}

#[test]
fn repository_functions_match_the_typescript() {
    assert_suite("plugins/annotation-repository.json", &Registry::new(), &repository_functions());
}

/// The repository suite's 95 field-changing patch pairs and 12 missing-icon
/// negatives are part of its contract: a regenerated vector that drops one
/// is noticed.
#[test]
fn the_repository_suite_holds_95_patch_pairs_and_12_missing_icon_negatives() {
    let suite = load("plugins/annotation-repository.json");
    let cases = suite["cases"].as_array().expect("cases");
    let pairs = cases.iter().filter(|c| c["fn"] == "annotationPatchBetween").count();
    assert_eq!(pairs, 95);
    let missing_icon = cases
        .iter()
        .filter(|c| c["name"].as_str().is_some_and(|n| n.contains("text-no-icon")))
        .count();
    assert_eq!(missing_icon, 12);
}
