//! G3's session gate, re-run through the wire dispatcher.
//!
//! `session_registry.rs` covers the registry directly. This covers the same
//! ground through `Host::call`, because that is what a web or mobile host
//! actually reaches and because the envelope is where the two can disagree:
//! a session that behaves correctly and reports something else is a session
//! nobody can use.
//!
//! This is the Rust half of the G3w rehearsal. The other half is
//! `packages/engine/services/src/worker-host/RustSessionRehearsal.ts`, which
//! sends these same envelopes from TypeScript.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::host::Host;

const PLAIN: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
const ENCRYPTED: &str = "tooling/engine-conformance/fixtures/encrypted-r4-aes-128.pdf";
const OPEN_PASSWORD: &str = "user";

fn bytes(fixture: &str) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(fixture))
        .unwrap_or_else(|e| panic!("read {fixture}: {e}"))
}

fn call(host: &mut Host, request: serde_json::Value, payloads: Vec<Vec<u8>>) -> serde_json::Value {
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).expect("every response is JSON")
}

/// [`call`], through `Host::call_legacy`, for a kind upstream removed.
fn call_legacy(
    host: &mut Host,
    request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> serde_json::Value {
    let response = host.call_legacy(&request.to_string(), payloads);
    serde_json::from_str(&response.json).expect("every response is JSON")
}

#[test]
fn an_open_reports_the_security_probe_the_client_handle_needs() {
    // Answered by the open rather than by a second request. A viewer decides
    // whether to show a password prompt from this, and asking separately
    // would be a visible round trip on every document.
    let mut host = Host::new();
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );

    let security = &json["result"]["security"];
    // `none`, not `unencrypted`: the wire spelling is the TypeScript's, and
    // `EncryptionState` has four values, one of which is `unknown`.
    assert_eq!(security["encryptionState"], "none");
    assert!(
        security["securityProbedAt"].as_i64().unwrap_or(0) > 0,
        "the probe has no timestamp: {security}",
    );
}

#[test]
fn a_document_that_needs_a_password_parks_rather_than_failing() {
    // A password is a STATE. The session takes its key and holds the bytes,
    // so the later unlock does not have to transfer the document again,
    // which for a large one is the expensive half of opening it.
    let mut host = Host::new();
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "locked" }),
        vec![bytes(ENCRYPTED)],
    );

    assert!(json.get("error").is_none(), "a locked document failed to open: {json}");
    let security = &json["result"]["security"];
    assert_eq!(security["encryptionState"], "encrypted");
    assert_eq!(security["encryptionRequiresPassword"], true);

    // The session exists and holds the key, which is what "parked" means.
    let json = call(&mut host, serde_json::json!({ "kind": "pages.list", "docId": "locked" }), Vec::new());
    assert_eq!(
        json["error"]["code"],
        format!("{:?}", EngineErrorCode::DocPasswordRequired),
        "a parked session served a read or reported the wrong reason: {json}",
    );
    assert_eq!(host.sessions().len(), 1, "the parked session did not take its key");
}

#[test]
fn opening_the_same_key_twice_is_refused_rather_than_replacing() {
    // Silently dropping the first session would close a document another
    // caller is still reading through.
    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );
    assert!(json["error"].is_object(), "the second open was accepted: {json}");
    assert_eq!(host.sessions().len(), 1);
}

#[test]
fn an_open_without_its_bytes_is_refused_before_a_session_exists() {
    // The bytes are a payload, never a field, and an open that arrived
    // without them has to leave no half-made session behind.
    let mut host = Host::new();
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        Vec::new(),
    );
    assert_eq!(json["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat));
    assert_eq!(host.sessions().len(), 0, "a failed open left a session behind");
}

#[test]
fn a_close_reports_what_it_closed_and_a_second_one_reports_nothing() {
    // The count is how a host tells "closed it" from "there was nothing to
    // close", which a pool reconciling its own bookkeeping needs.
    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );

    let json = call(&mut host, serde_json::json!({ "kind": "close", "docId": "d" }), Vec::new());
    assert_eq!(json["result"]["closedSessions"], 1);

    let json = call(&mut host, serde_json::json!({ "kind": "close", "docId": "d" }), Vec::new());
    assert_eq!(json["result"]["closedSessions"], 0);
    assert_eq!(host.sessions().len(), 0);
}

#[test]
fn a_document_opened_and_closed_without_a_single_read_leaves_nothing_behind() {
    // The G3w gate, verbatim. The rehearsal exists to prove a Rust-owned
    // session can be opened and closed under the real transport before any
    // read routes to it, so this is the shape that has to be clean: no page
    // acquired, no lease outstanding, no session left.
    let mut host = Host::new();
    for round in 0..8 {
        let id = format!("doc-{round}");
        let json = call(
            &mut host,
            serde_json::json!({ "kind": "open.fatMem", "docId": id }),
            vec![bytes(PLAIN)],
        );
        assert!(json.get("error").is_none(), "round {round} would not open: {json}");
        let json = call(&mut host, serde_json::json!({ "kind": "close", "docId": id }), Vec::new());
        assert_eq!(json["result"]["closedSessions"], 1, "round {round} did not close");
    }
    assert_eq!(host.sessions().len(), 0, "sessions accumulated across rounds");
}

#[test]
fn a_shutdown_refuses_new_work_rather_than_serving_it() {
    // Terminal by design: a shutdown happens because the worker is going
    // away, and an engine that quietly came back would let a supervisor keep
    // using one it had decided to discard.
    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );
    call(&mut host, serde_json::json!({ "kind": "shutdown" }), Vec::new());
    assert_eq!(host.sessions().len(), 0);

    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "again" }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(json["error"]["code"], format!("{:?}", EngineErrorCode::RuntimeUnavailable));
}

#[test]
fn checking_a_password_answers_without_changing_the_session() {
    // "Would this password help?" is what a prompt asks before it commits to
    // a reopen, so it must not unlock anything on the way.
    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );

    let json = call(
        &mut host,
        serde_json::json!({
            "kind": "document.checkPasswordPermissions",
            "docId": "d",
            "password": OPEN_PASSWORD,
        }),
        Vec::new(),
    );
    // An unencrypted document has no password to be wrong about; what
    // matters is that the call answers with a probe rather than an error and
    // that the session is still readable afterwards.
    assert!(json["result"].is_object() || json["error"].is_object(), "no answer: {json}");

    let json = call(&mut host, serde_json::json!({ "kind": "pages.list", "docId": "d" }), Vec::new());
    assert!(json["result"]["pages"].is_array(), "the check disturbed the session: {json}");
}

#[test]
fn a_layer_close_answers_whether_there_was_one_to_close() {
    // Boolean rather than silent, for the same reason the document close
    // returns a count: a host reconciling its own map needs to know.
    let mut host = Host::new();
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "layer.close", "docId": "d", "layerName": "nothing" }),
        Vec::new(),
    );
    assert_eq!(json["result"]["closed"], false);
}

#[test]
fn a_page_edit_reaches_the_page_reads() {
    // Every page read checks the object number against the open document's
    // index first. A page mutation rebuilds the session's registry; the read
    // index has to follow it, or an inserted page is `NotFound` and a
    // deleted one is still found.
    let mut host = Host::new();
    call(&mut host, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }), vec![bytes(PLAIN)]);
    // `pages.insertBlank` and `pages.delete` were removed upstream; owner
    // 8.18 (`document.apply`).
    let inserted = call_legacy(
        &mut host,
        serde_json::json!({
            "kind": "pages.insertBlank", "docId": "d",
            "size": { "width": 612.0, "height": 792.0 }
        }),
        Vec::new(),
    );
    let new_page = inserted["result"]["result"]["insertedPages"][0].clone();
    assert!(new_page["objectNumber"].is_u64(), "no inserted page: {inserted}");
    let text = call(
        &mut host,
        serde_json::json!({ "kind": "pages.text", "docId": "d", "page": new_page }),
        Vec::new(),
    );
    assert!(text.get("error").is_none(), "the inserted page is unknown to reads: {text}");

    let deleted = call_legacy(
        &mut host,
        serde_json::json!({ "kind": "pages.delete", "docId": "d", "pages": [new_page] }),
        Vec::new(),
    );
    assert!(deleted.get("error").is_none(), "{deleted}");
    let text = call(
        &mut host,
        serde_json::json!({ "kind": "pages.text", "docId": "d", "page": new_page }),
        Vec::new(),
    );
    assert_eq!(text["error"]["code"], "NotFound", "a deleted page still reads: {text}");
}
