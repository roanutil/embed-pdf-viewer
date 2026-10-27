//! `vectors/plugins/attachment.json`, generated from
//! `packages/plugin/attachment`.

use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_attachment::harness::Attachment;
use epdf_plugin_history::harness::History;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_stage::harness::Stage;
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Annotation).with(Stage).with(History).with(Attachment)
}

#[test]
fn attachment_matches_the_typescript() {
    assert_suite("plugins/attachment.json", &registry(), &Vec::new());
}
