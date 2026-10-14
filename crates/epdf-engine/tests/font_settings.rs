//! `document.setFontSettings` and `fonts.authorizeEditing`, through the
//! host. Mirrors `WorkerHost.handleDocumentSetFontSettings` and
//! `FontRegistrar.authorizeEditing`.

use epdf_engine::host::Host;
use epdf_engine::session::registry::SessionKey;
use serde_json::json;

mod common;
use common::{call, call_err, open_fixture};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

fn settings(host: &Host) -> (i32, bool) {
    let session = host.sessions().session(&SessionKey::document("d")).expect("the session");
    let document = session.document().expect("open").document();
    (document.font_embedding_policy(), document.typographic_features())
}

#[test]
fn font_settings_change_only_the_members_given() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FIXTURE);
    assert_eq!(settings(&host), (0, false), "the fork's defaults");

    call(
        &mut host,
        json!({ "kind": "document.setFontSettings", "docId": doc,
        "embeddingPolicy": "full" }),
    );
    assert_eq!(settings(&host), (2, false));

    call(
        &mut host,
        json!({ "kind": "document.setFontSettings", "docId": doc,
        "typographicFeatures": true }),
    );
    assert_eq!(settings(&host), (2, true), "an absent policy leaves the policy alone");

    call(
        &mut host,
        json!({ "kind": "document.setFontSettings", "docId": doc,
        "embeddingPolicy": "subset", "typographicFeatures": false }),
    );
    assert_eq!(settings(&host), (1, false));
}

#[test]
fn font_settings_are_session_state_rather_than_a_mutation() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FIXTURE);
    let answer = call(
        &mut host,
        json!({ "kind": "document.setFontSettings", "docId": doc,
        "embeddingPolicy": "default" }),
    );
    assert!(answer.is_null(), "no event, no mutation sequence: {answer}");
}

#[test]
fn an_unknown_embedding_policy_is_refused() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FIXTURE);
    let error = call_err(
        &mut host,
        json!({ "kind": "document.setFontSettings", "docId": doc,
        "embeddingPolicy": "partial" }),
    );
    assert_eq!(error["code"], json!("InvalidArg"), "{error}");
    assert_eq!(settings(&host), (0, false));
}

fn host_call(
    host: &mut Host,
    request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> serde_json::Value {
    let mut request = request;
    request["jobId"] = json!(9);
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).unwrap()
}

/// Each test on a thread of its own: PDFium's font registry is thread-global.
fn on_a_fresh_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::spawn(body).join().expect("the test thread panicked");
}

#[test]
fn authorizing_a_registered_font_answers_its_identity_and_licence() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let roboto = std::fs::read(
            test_support::workspace_root()
                .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
        )
        .expect("read Roboto");
        let registered = host_call(
            &mut host,
            json!({ "kind": "fonts.register", "fontKey": "roboto", "familyName": "",
                "weight": 0, "italic": false }),
            vec![roboto],
        );
        assert!(registered.get("error").is_none(), "{registered}");

        let answer = host_call(
            &mut host,
            json!({ "kind": "fonts.authorizeEditing", "fontKey": "roboto" }),
            Vec::new(),
        );
        let identity = &answer["result"]["identity"];
        assert_eq!(identity["familyName"], json!("Roboto"), "{answer}");
        assert_eq!(identity["weight"], json!(400));
        assert_eq!(identity["italic"], json!(false));
        assert_eq!(identity["embeddingPermission"], json!("installable"));
        assert_eq!(identity["editingAuthorized"], json!(true));
        assert_eq!(identity["instanced"], json!(false));
    });
}

#[test]
fn authorizing_an_unregistered_key_is_refused() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let answer = host_call(
            &mut host,
            json!({ "kind": "fonts.authorizeEditing", "fontKey": "nobody" }),
            Vec::new(),
        );
        assert_eq!(answer["error"]["code"], json!("InvalidArg"), "{answer}");
        assert!(answer["error"]["message"].as_str().unwrap().contains("nobody"));
    });
}
