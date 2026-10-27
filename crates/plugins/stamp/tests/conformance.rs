//! `vectors/plugins/stamp-*.json`, generated from `packages/plugin/stamp`.

use epdf_plugin_actions::harness::Actions;
use epdf_plugin_annotation::harness::Annotation;
use epdf_plugin_form::harness::Form;
use epdf_plugin_interaction::harness::Interaction;
use epdf_plugin_stamp::harness::{self, Stamp};
use plugin_conformance::{Registry, assert_suite};

fn registry() -> Registry {
    Registry::new().with(Interaction).with(Actions).with(Annotation).with(Form).with(Stamp)
}

#[test]
fn library_import_matches_the_typescript() {
    assert_suite("plugins/stamp-library.json", &registry(), &Vec::new());
}

#[test]
fn assets_match_the_typescript() {
    assert_suite("plugins/stamp-assets.json", &registry(), &Vec::new());
}

#[test]
fn placement_matches_the_typescript() {
    assert_suite("plugins/stamp-placement.json", &registry(), &Vec::new());
}

#[test]
fn the_current_surface_matches_the_typescript() {
    assert_suite("plugins/stamp-api.json", &registry(), &Vec::new());
}

#[test]
fn transitions_match_the_typescript() {
    assert_suite("plugins/stamp-functions.json", &Registry::new(), &harness::functions());
}

/// The settings verbs no vector calls, through the adapter: a listener
/// hears an update with the members it changed, `getSettings` reads it
/// back, and the case's host (the `[stamp]` console and `Math.random`) is
/// put back once the case's kernel is gone, as `restoreStampHost` runs on
/// `onCaseEnd`.
#[test]
fn the_settings_verbs_replay_and_the_host_is_restored() {
    use serde_json::json;
    let call = |method: &str, args: serde_json::Value| json!({ "op": "call", "target": { "token": "stamp" }, "method": method, "args": args });
    let case = json!({
        "name": "settings",
        "plugins": [{ "factory": "stamp-current", "config": { "assetEngine": "instance" } }],
        "steps": [
            { "op": "call", "target": "kernel", "method": "start" },
            call("onSettingsChanged", json!([{ "$fn": "changed" }])),
            call("updateSettings", json!([{ "previewWidth": 96, "dynamic": false }])),
            call("getSettings", json!([])),
            call("resetSettings", json!([])),
        ],
    });
    let before = epdf_plugin_stamp::console::set_sink(None);
    assert!(before.is_none());
    let run = plugin_conformance::scenario::run_case(&case, &registry());
    let callbacks = &run.trace[2]["callbacks"];
    assert_eq!(callbacks[0]["fn"], "changed");
    assert_eq!(callbacks[0]["args"][0]["changed"], json!(["previewWidth", "dynamic"]));
    let read = &run.trace[3]["outcome"]["returned"];
    assert_eq!((read["previewWidth"].as_f64(), &read["dynamic"]), (Some(96.0), &json!(false)));
    assert_eq!(
        run.trace[4]["callbacks"][0]["args"][0]["settings"]["previewWidth"].as_f64(),
        Some(256.0)
    );
    assert!(
        epdf_plugin_stamp::console::set_sink(None).is_none(),
        "the case's console sink is gone"
    );
    assert!(epdf_plugin_stamp::console::set_random(None).is_none(), "the case's random is gone");
}
