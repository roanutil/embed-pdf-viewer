//! `document.setFontSettings` and the `fonts.*` wire, through the host.
//! Mirrors `WorkerHost.handleDocumentSetFontSettings`, the `handleFonts*`
//! handlers and `FontRegistrar`.

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
                "weight": 0, "italic": -1 }),
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

fn roboto() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("packages/engine/main/test/fixtures/Roboto-Regular.ttf"),
    )
    .expect("read Roboto")
}

fn register(host: &mut Host, key: &str, italic: i32, data: Vec<u8>) -> serde_json::Value {
    host_call(
        host,
        json!({ "kind": "fonts.register", "fontKey": key, "familyName": "",
            "weight": 0, "italic": italic }),
        vec![data],
    )
}

#[test]
fn registering_answers_the_key_and_the_identity_the_runtime_resolved() {
    // `{ tag: 'fonts.register'; fontKey; identity: FontIdentityInfo }`: no
    // native id, which is per-thread and means nothing to the caller.
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let answer = register(&mut host, "roboto", -1, roboto());
        assert_eq!(
            answer["result"],
            json!({ "fontKey": "roboto", "identity": {
                "familyName": "Roboto", "weight": 400, "italic": false,
                "embeddingPermission": "installable", "editingAuthorized": true,
                "instanced": false } }),
            "{answer}"
        );
    });
}

#[test]
fn italic_is_the_c_apis_number_rather_than_a_bool() {
    // `-1` infers, `0` and `1` say, as `EPDFFont_RegisterMemFont64` reads it.
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let said = register(&mut host, "slanted", 1, roboto());
        assert_eq!(said["result"]["identity"]["italic"], json!(true), "{said}");
        let inferred = register(&mut host, "upright", -1, roboto());
        assert_eq!(inferred["result"]["identity"]["italic"], json!(false), "{inferred}");

        let refused = host_call(
            &mut host,
            json!({ "kind": "fonts.register", "fontKey": "bool", "familyName": "",
                "weight": 0, "italic": true }),
            vec![roboto()],
        );
        assert_eq!(refused["error"]["code"], json!("WireFormat"), "{refused}");
    });
}

#[test]
fn a_given_family_and_weight_are_the_identity() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let answer = host_call(
            &mut host,
            json!({ "kind": "fonts.register", "fontKey": "brand", "familyName": "Brand Sans",
                "weight": 700, "italic": 0 }),
            vec![roboto()],
        );
        let identity = &answer["result"]["identity"];
        assert_eq!(identity["familyName"], json!("Brand Sans"), "{answer}");
        assert_eq!(identity["weight"], json!(700), "{answer}");
        assert_eq!(identity["italic"], json!(false), "{answer}");
    });
}

#[test]
fn empty_and_unloadable_font_data_are_refused_as_the_registrar_words_it() {
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        let empty = register(&mut host, "empty", -1, Vec::new());
        assert_eq!(empty["error"]["code"], json!("InvalidArg"), "{empty}");
        assert_eq!(empty["error"]["message"], json!("empty font data: empty"), "{empty}");

        let garbage = register(&mut host, "garbage", -1, b"this is not a font".to_vec());
        assert_eq!(garbage["error"]["code"], json!("InvalidArg"), "{garbage}");
        assert_eq!(
            garbage["error"]["message"],
            json!("font registration failed (unloadable or no glyphs): garbage"),
            "{garbage}"
        );
    });
}

#[test]
fn registering_a_key_twice_replaces_what_it_names() {
    // `FontRegistrar.register` has no duplicate check: the second call
    // registers again and the key answers the newer identity.
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        register(&mut host, "roboto", 0, roboto());
        let again = register(&mut host, "roboto", 1, roboto());
        assert_eq!(again["result"]["identity"]["italic"], json!(true), "{again}");
    });
}

#[test]
fn fallback_and_clear_answer_only_their_tag() {
    // `{ tag: 'fonts.addFallback' }`, `{ tag: 'fonts.clearFallbacks' }` and
    // `{ tag: 'fonts.clear' }` carry nothing else.
    on_a_fresh_thread(|| {
        let mut host = Host::new();
        register(&mut host, "roboto", -1, roboto());
        for kind in ["fonts.addFallback", "fonts.clearFallbacks", "fonts.clear"] {
            let answer =
                host_call(&mut host, json!({ "kind": kind, "fontKey": "roboto" }), Vec::new());
            assert!(answer.get("error").is_none(), "{kind}: {answer}");
            assert!(answer["result"].is_null(), "{kind}: {answer}");
        }

        let unknown = host_call(
            &mut host,
            json!({ "kind": "fonts.addFallback", "fontKey": "roboto" }),
            Vec::new(),
        );
        assert_eq!(unknown["error"]["code"], json!("InvalidArg"), "{unknown}");
        assert_eq!(
            unknown["error"]["message"],
            json!("font not registered on this thread: roboto"),
            "cleared, so the key is gone: {unknown}"
        );
    });
}
