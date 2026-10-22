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
fn a_close_answers_the_tag_alone_and_a_second_one_answers_the_same() {
    // `WorkerHost.handleClose` answers `{ tag: 'close' }` whether or not a
    // session was there: closing nothing is the state the caller asked for.
    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );

    let json = call(&mut host, serde_json::json!({ "kind": "close", "docId": "d" }), Vec::new());
    assert_eq!(json, serde_json::json!({ "result": null }));
    assert_eq!(host.sessions().len(), 0);

    let json = call(&mut host, serde_json::json!({ "kind": "close", "docId": "d" }), Vec::new());
    assert_eq!(json, serde_json::json!({ "result": null }));
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
        assert_eq!(json, serde_json::json!({ "result": null }), "round {round} did not close");
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
fn a_layer_close_answers_the_tag_alone_whether_or_not_there_was_one() {
    // `WorkerHost.handleLayerClose`: idempotent, the caller may be closing a
    // layer this host never held, and the answer is `{ tag: 'close' }`.
    let mut host = Host::new();
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "layer.close", "docId": "d", "layerName": "nothing" }),
        Vec::new(),
    );
    assert_eq!(json, serde_json::json!({ "result": null }));

    call(
        &mut host,
        serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "layerName": "a",
            "baseKey": "base", "layer": { "kind": "fresh" } }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(host.sessions().len(), 1);
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "layer.close", "docId": "d", "layerName": "a" }),
        Vec::new(),
    );
    assert_eq!(json, serde_json::json!({ "result": null }));
    assert_eq!(host.sessions().len(), 0);
}

#[test]
fn a_page_edit_reaches_the_page_reads() {
    // Every page read checks the object number against the open document's
    // index first. A page mutation rebuilds the session's registry; the read
    // index has to follow it, or an inserted page is `NotFound` and a
    // deleted one is still found.
    let mut host = Host::new();
    call(&mut host, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }), vec![bytes(PLAIN)]);
    // Page structure is `document.apply` ops (119685ea4).
    let inserted = call(
        &mut host,
        serde_json::json!({
            "kind": "document.apply", "docId": "d",
            "change": { "ops": [{ "type": "pages.insertBlank",
                "spec": { "size": { "width": 612.0, "height": 792.0 } } }] }
        }),
        Vec::new(),
    );
    let new_page = inserted["result"]["result"]["items"][0]["insertedPages"][0].clone();
    assert!(new_page["objectNumber"].is_u64(), "no inserted page: {inserted}");
    let text = call(
        &mut host,
        serde_json::json!({ "kind": "pages.text", "docId": "d", "page": new_page }),
        Vec::new(),
    );
    assert!(text.get("error").is_none(), "the inserted page is unknown to reads: {text}");

    let deleted = call(
        &mut host,
        serde_json::json!({ "kind": "document.apply", "docId": "d",
            "change": { "ops": [{ "type": "pages.delete", "pages": [new_page] }] } }),
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

/// A layer over [`PLAIN`] under `doc` (no layer name), with `extra` fields
/// merged into the request.
fn open_layer(host: &mut Host, doc: &str, extra: serde_json::Value) -> serde_json::Value {
    let mut request = serde_json::json!({
        "kind": "open.layerMemBase", "docId": doc, "baseKey": format!("base-{doc}"),
        "layer": { "kind": "fresh" }, "password": null,
    });
    for (name, value) in extra.as_object().expect("an object") {
        request[name] = value.clone();
    }
    call(host, request, vec![bytes(PLAIN)])
}

#[test]
fn an_open_answers_its_document_and_a_layers_last_object_number() {
    // `{ tag: 'open', docId, security, protection, objectNumbers?,
    // lastObjectNumber? }` (`WorkerHost.handleOpen`). A layer knows its last
    // object number, and `open.fatMem` opens its bytes as one
    // (`openBytesAsLayer`), so its answer carries the same number.
    let mut host = Host::new();
    let layer = open_layer(&mut host, "layered", serde_json::json!({}));
    assert_eq!(layer["result"]["docId"], "layered", "{layer}");
    assert_eq!(layer["result"]["security"]["encryptionState"], "none");
    assert!(layer["result"].get("protection").is_some(), "{layer}");
    assert!(layer["result"]["lastObjectNumber"].as_u64().unwrap_or(0) > 0, "{layer}");
    assert!(layer["result"].get("objectNumbers").is_none(), "nothing was asked for: {layer}");
    // No layer name: the session is the document's own, as upstream keys it.
    let list =
        call(&mut host, serde_json::json!({ "kind": "pages.list", "docId": "layered" }), vec![]);
    assert!(list["result"]["pages"].is_array(), "{list}");

    let plain = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "plain" }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(plain["result"]["docId"], "plain", "{plain}");
    assert_eq!(plain["result"]["lastObjectNumber"], layer["result"]["lastObjectNumber"], "{plain}");
}

#[test]
fn an_open_hands_out_the_numbers_it_asked_for() {
    // `reserveObjectNumbers`: the result has them, and the last object
    // number the open reports is past them.
    let mut host = Host::new();
    let first = open_layer(&mut host, "a", serde_json::json!({}));
    let base_last = first["result"]["lastObjectNumber"].as_u64().expect("a number");
    let json = open_layer(&mut host, "b", serde_json::json!({ "reserveObjectNumbers": 3 }));
    assert_eq!(
        json["result"]["objectNumbers"],
        serde_json::json!({ "first": base_last + 1, "count": 3 }),
        "{json}",
    );
    assert_eq!(json["result"]["lastObjectNumber"], base_last + 3, "{json}");
}

#[test]
fn a_reserve_hands_out_the_next_numbers_each_once() {
    // A layer opened over a base, and `open.fatMem`'s layer over its bytes.
    let mut host = Host::new();
    let opened = open_layer(&mut host, "d", serde_json::json!({}));
    reserves_the_next_numbers_each_once(host, opened);
    let mut host = Host::new();
    let opened = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );
    reserves_the_next_numbers_each_once(host, opened);
}

/// The reserves of `a_reserve_hands_out_the_next_numbers_each_once`, on the
/// session `opened` answered for `d`.
fn reserves_the_next_numbers_each_once(mut host: Host, opened: serde_json::Value) {
    let last = opened["result"]["lastObjectNumber"].as_u64().expect("a number");
    let reserve = |host: &mut Host, count: serde_json::Value| {
        call(
            host,
            serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": count }),
            vec![],
        )
    };
    let one = reserve(&mut host, serde_json::json!(2));
    assert_eq!(one["result"]["range"], serde_json::json!({ "first": last + 1, "count": 2 }));
    let two = reserve(&mut host, serde_json::json!(5));
    assert_eq!(two["result"]["range"], serde_json::json!({ "first": last + 3, "count": 5 }));

    // `DocumentSession.reserveObjectNumbers`: a count that is not a whole
    // number of at least one is refused on `count`.
    for count in [
        serde_json::json!(0),
        serde_json::json!(1.5),
        serde_json::json!(-2),
        serde_json::json!("3"),
        serde_json::json!(null),
    ] {
        let refused = reserve(&mut host, count.clone());
        assert_eq!(refused["error"]["code"], "InvalidArg", "{count}: {refused}");
        assert_eq!(refused["error"]["details"]["field"], "count", "{count}: {refused}");
    }
    // Nothing was spent by the refusals.
    let three = reserve(&mut host, serde_json::json!(1));
    assert_eq!(three["result"]["range"]["first"], last + 8);
}

#[test]
fn a_reserve_past_the_issue_limit_is_layer_full() {
    // `OBJECT_NUMBER_ISSUE_LIMIT` (8,000,000): the numbers above it are the
    // engine's own, for appearance streams and fonts.
    let mut host = Host::new();
    let opened = open_layer(&mut host, "d", serde_json::json!({}));
    let last = opened["result"]["lastObjectNumber"].as_u64().expect("a number");
    let refused = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 8_000_000 }),
        vec![],
    );
    assert_eq!(refused["error"]["code"], "LayerFull", "{refused}");
    assert_eq!(refused["error"]["details"]["lastObjectNumber"], last, "{refused}");
    // A count past any document's numbers is the same refusal, not a
    // malformed request.
    let huge = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 1e12 }),
        vec![],
    );
    assert_eq!(huge["error"]["code"], "LayerFull", "{huge}");
    assert_eq!(huge["error"]["details"]["lastObjectNumber"], last, "{huge}");
    // Up to the limit is still handed out.
    let to_limit = 8_000_000 - last;
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": to_limit }),
        vec![],
    );
    assert_eq!(json["result"]["range"]["count"], to_limit, "{json}");
}

#[test]
fn a_session_the_caller_numbers_hands_out_nothing() {
    // `objectNumbers: 'caller'`: a server checked every number itself.
    let mut host = Host::new();
    let opened = open_layer(&mut host, "d", serde_json::json!({ "objectNumbers": "caller" }));
    assert!(opened.get("error").is_none(), "{opened}");
    let refused = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 1 }),
        vec![],
    );
    assert_eq!(refused["error"]["code"], "InvalidArg", "{refused}");
    // And an open that asks for numbers it may not hand out fails.
    let refused = open_layer(
        &mut host,
        "e",
        serde_json::json!({ "objectNumbers": "caller",
            "reserveObjectNumbers": 2 }),
    );
    assert_eq!(refused["error"]["code"], "InvalidArg", "{refused}");

    let malformed = open_layer(&mut host, "f", serde_json::json!({ "objectNumbers": "server" }));
    assert_eq!(malformed["error"]["code"], "WireFormat", "{malformed}");
}

#[test]
fn a_fat_memory_session_hands_out_numbers_past_its_file() {
    // `open.fatMem` opens its bytes as a fresh layer (`openBytesAsLayer`),
    // so the numbers it hands out start past the file's last one.
    let mut host = Host::new();
    let opened = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d" }),
        vec![bytes(PLAIN)],
    );
    let last = opened["result"]["lastObjectNumber"].as_u64().expect("a layer's number");
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 2 }),
        vec![],
    );
    assert_eq!(json["result"]["range"], serde_json::json!({ "first": last + 1, "count": 2 }));
}

#[test]
fn a_fat_memory_open_hands_out_the_numbers_it_asked_for() {
    let mut host = Host::new();
    let plain = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "a" }),
        vec![bytes(PLAIN)],
    );
    let file_last = plain["result"]["lastObjectNumber"].as_u64().expect("a number");
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "b", "reserveObjectNumbers": 2 }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(
        json["result"]["objectNumbers"],
        serde_json::json!({ "first": file_last + 1, "count": 2 }),
        "{json}",
    );
    assert_eq!(json["result"]["lastObjectNumber"], file_last + 2, "{json}");
}

#[test]
fn a_fat_memory_write_honours_its_floor_and_answers_no_artifact() {
    // `objectNumberFloor` raises a fat-memory session's last object number
    // as it does a layer's. The write answers no artifact
    // (`persistLayerArtifact = false`), where a layer's carries one.
    let mut host = Host::new();
    call(&mut host, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }), vec![bytes(PLAIN)]);
    open_layer(&mut host, "l", serde_json::json!({}));
    let write = |doc: &str| {
        serde_json::json!({ "kind": "metadata.update", "docId": doc, "opId": "op-1",
            "objectNumberFloor": 5000, "patch": { "title": "Floored" } })
        .to_string()
    };
    let written = host.call(&write("d"), vec![]);
    assert!(!written.json.contains("\"error\""), "{}", written.json);
    assert!(written.payloads.is_empty(), "a fat-memory write answered an artifact");
    let layered = host.call(&write("l"), vec![]);
    assert!(!layered.json.contains("\"error\""), "{}", layered.json);
    assert_eq!(layered.payloads.len(), 1, "a layer's write answers its artifact");

    let json = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 1 }),
        vec![],
    );
    let first = json["result"]["range"]["first"].as_u64().expect("a range");
    assert!(first >= 5000, "{json}");
}

#[test]
fn a_write_refused_for_a_parked_signing_spends_no_numbers() {
    // `WorkerHost.run` checks for a parked signing before it raises the
    // floor (`WorkerHost.ts:548-553`), so a refused write moves nothing.
    const SIGFIELD: &str = "packages/engine/main/test/fixtures/unsigned_sigfield.pdf";
    let mut host = Host::new();
    let opened = call(
        &mut host,
        serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "baseKey": "sig",
            "layer": { "kind": "fresh" }, "password": null }),
        vec![bytes(SIGFIELD)],
    );
    let last = opened["result"]["lastObjectNumber"].as_u64().expect("a layer");
    let listed =
        call(&mut host, serde_json::json!({ "kind": "signatures.list", "docId": "d" }), vec![]);
    let field = listed["result"]["snapshot"]["signatures"][0]["field"].clone();
    let prepared = call(
        &mut host,
        serde_json::json!({ "kind": "signatures.prepare", "docId": "d",
            "input": { "field": field, "signer": { "name": "Test Signer" } } }),
        vec![],
    );
    assert!(prepared.get("error").is_none(), "{prepared}");

    let refused = call(
        &mut host,
        serde_json::json!({ "kind": "metadata.update", "docId": "d", "opId": "op-1",
            "objectNumberFloor": 5000, "patch": { "title": "Refused" } }),
        vec![],
    );
    assert_eq!(refused["error"]["code"], "SigningPending", "{refused}");
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 1 }),
        vec![],
    );
    assert_eq!(json["result"]["range"]["first"], last + 1, "{json}");
}

#[test]
fn a_write_honours_the_floor_its_caller_set() {
    // `objectNumberFloor`: the layer's last object number is raised to just
    // below it before the write runs, so the numbers handed out after it
    // start at or above it.
    let mut host = Host::new();
    open_layer(&mut host, "d", serde_json::json!({}));
    let written = call(
        &mut host,
        serde_json::json!({ "kind": "metadata.update", "docId": "d", "opId": "op-1",
            "objectNumberFloor": 5000, "patch": { "title": "Floored" } }),
        vec![],
    );
    assert!(written.get("error").is_none(), "{written}");
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "objectNumbers.reserve", "docId": "d", "count": 1 }),
        vec![],
    );
    let first = json["result"]["range"]["first"].as_u64().expect("a range");
    assert!(first >= 5000, "{json}");
}

#[test]
fn the_open_keeps_what_it_was_asked_for_on_the_session() {
    // `signedDocumentPolicy` and `objectNumbers` live on the session, a
    // parked one included, so the signature engine (8.26) and the creates
    // can read them.
    use epdf_engine::features::signature::types::SignedDocumentPolicy;
    use epdf_engine::session::document::ObjectNumberAuthority;
    use epdf_engine::session::registry::SessionKey;

    let mut host = Host::new();
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d",
            "signedDocumentPolicy": "permit", "objectNumbers": "caller" }),
        vec![bytes(PLAIN)],
    );
    let session = host.sessions().session(&SessionKey::document("d")).expect("open");
    assert_eq!(session.signed_document_policy(), SignedDocumentPolicy::Permit);
    assert_eq!(session.object_number_authority(), ObjectNumberAuthority::Caller);

    let locked = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "locked",
            "signedDocumentPolicy": "permit" }),
        vec![bytes(ENCRYPTED)],
    );
    assert!(locked["result"].get("lastObjectNumber").is_none(), "{locked}");
    call(
        &mut host,
        serde_json::json!({ "kind": "document.checkPasswordPermissions", "docId": "locked",
            "password": OPEN_PASSWORD }),
        vec![],
    );
    let unlocked = host.sessions().session(&SessionKey::document("locked")).expect("unlocked");
    assert_eq!(unlocked.signed_document_policy(), SignedDocumentPolicy::Permit);

    let malformed = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "e",
            "signedDocumentPolicy": "ignore" }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(malformed["error"]["code"], "WireFormat", "{malformed}");
}

#[test]
fn a_layer_reopens_from_the_artifact_its_save_answered() {
    // `document.saveLayerBuffer` answers the layer artifact, and
    // `layer: { kind: 'artifact' }` opens a layer from it (its bytes the
    // payload after the base's).
    let mut host = Host::new();
    open_layer(&mut host, "d", serde_json::json!({}));
    call(
        &mut host,
        serde_json::json!({ "kind": "metadata.update", "docId": "d",
            "patch": { "title": "Layered" } }),
        vec![],
    );
    let response = host.call(
        &serde_json::json!({ "kind": "document.saveLayerBuffer", "docId": "d" }).to_string(),
        vec![],
    );
    let saved: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    let artifact = response.payloads.into_iter().next().expect("the artifact");
    assert_eq!(saved["result"]["size"], artifact.len(), "{saved}");

    let reopened = call(
        &mut host,
        serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "layerName": "again",
            "baseKey": "base-d", "layer": { "kind": "artifact" }, "password": null }),
        vec![bytes(PLAIN), artifact],
    );
    assert!(reopened.get("error").is_none(), "{reopened}");
    let read = call(
        &mut host,
        serde_json::json!({ "kind": "metadata.read", "docId": "d", "layerName": "again" }),
        vec![],
    );
    assert_eq!(read["result"]["title"], "Layered", "{read}");

    // A layer that names bytes it did not send is refused before it opens.
    let missing = call(
        &mut host,
        serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "layerName": "none",
            "baseKey": "base-d", "layer": { "kind": "raw-delta" }, "password": null }),
        vec![bytes(PLAIN)],
    );
    assert_eq!(missing["error"]["code"], "WireFormat", "{missing}");
}

#[test]
fn a_working_set_is_taken_and_never_answered() {
    // `pages.workingSet` (`WorkerHost.receive`): bookkeeping, taken at once,
    // and ignored for a document that is not open. This entry answers every
    // call, so it answers `null`, malformed input included.
    use epdf_engine::session::pages::Place;
    use epdf_engine::session::registry::SessionKey;

    let mut host = Host::new();
    call(&mut host, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }), vec![bytes(PLAIN)]);
    let list = call(&mut host, serde_json::json!({ "kind": "pages.list", "docId": "d" }), vec![]);
    let page = list["result"]["pages"][0]["ref"].clone();
    let number = page["objectNumber"].as_u64().expect("a page") as u32;
    let json = call(
        &mut host,
        serde_json::json!({ "kind": "pages.workingSet", "docId": "d", "view": "main",
            "pages": [{ "page": page, "role": "visible", "pixels": 1200 }] }),
        vec![],
    );
    assert_eq!(json, serde_json::json!({ "result": null }));
    let session = host.sessions().session(&SessionKey::document("d")).expect("open");
    let placed = session
        .page_pool()
        .expect("a pool")
        .place_of(epdf_engine::identity::PageObjectNumber::new(number).unwrap());
    assert_eq!(placed.place, Place::Visible);

    let ignored = call(
        &mut host,
        serde_json::json!({ "kind": "pages.workingSet", "docId": "closed", "view": "main",
            "pages": [] }),
        vec![],
    );
    assert_eq!(ignored, serde_json::json!({ "result": null }));
    let malformed = call(
        &mut host,
        serde_json::json!({ "kind": "pages.workingSet", "docId": "d", "view": "main",
            "pages": [{ "page": page, "role": "offscreen", "pixels": 0 }] }),
        vec![],
    );
    // Upstream never answers it, so input it cannot read changes nothing
    // and reports nothing.
    assert_eq!(malformed, serde_json::json!({ "result": null }));
    let still = host
        .sessions()
        .session(&SessionKey::document("d"))
        .expect("open")
        .page_pool()
        .expect("a pool")
        .place_of(epdf_engine::identity::PageObjectNumber::new(number).unwrap());
    assert_eq!(still.place, Place::Visible, "a malformed set replaced the last one");
}

#[test]
fn a_version_and_a_file_save_answer_as_upstream_does() {
    // `{ tag: 'document.version'; version }` and `{ tag:
    // 'document.saveFile'; path }`.
    let mut host = Host::new();
    call(&mut host, serde_json::json!({ "kind": "open.fatMem", "docId": "d" }), vec![bytes(PLAIN)]);
    let version =
        call(&mut host, serde_json::json!({ "kind": "document.version", "docId": "d" }), vec![]);
    assert_eq!(version["result"]["version"]["byteLength"], bytes(PLAIN).len(), "{version}");
    assert_eq!(version["result"]["version"]["sha256"].as_str().map(str::len), Some(64));

    let dir = std::env::temp_dir().join(format!("epdf-save-file-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("saved.pdf").to_string_lossy().into_owned();
    let saved = call(
        &mut host,
        serde_json::json!({ "kind": "document.saveFile", "docId": "d", "mode": "rewrite",
            "path": path }),
        vec![],
    );
    assert_eq!(saved, serde_json::json!({ "result": { "path": path } }));
    assert!(std::fs::read(&path).unwrap().starts_with(b"%PDF"));
    std::fs::remove_dir_all(&dir).ok();
}

/// `opening.test.ts`'s locked file: PDFium's `encrypted.pdf`, user password
/// `1234` (no copy, no print), owner password `5678`.
const LOCKED: &str = "packages/engine/main/test/fixtures/encrypted.pdf";

/// The three input kinds, as the open request each one sends: `bytes`
/// (`open.fatMem`), `layerBytes` (`open.layerMemBase`) and `layerFile`
/// (`open.layerFileBase`), with the payloads it carries.
fn locked_opens(password: Option<&str>) -> Vec<(&'static str, serde_json::Value, Vec<Vec<u8>>)> {
    let path = test_support::workspace_root().join(LOCKED);
    vec![
        (
            "bytes",
            serde_json::json!({ "kind": "open.fatMem", "docId": "d", "password": password }),
            vec![bytes(LOCKED)],
        ),
        (
            "layerBytes",
            serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "baseKey": "base",
                "layer": { "kind": "fresh" }, "password": password }),
            vec![bytes(LOCKED)],
        ),
        (
            "layerFile",
            serde_json::json!({ "kind": "open.layerFileBase", "docId": "d", "baseKey": "base",
                "basePath": path.to_string_lossy(), "layer": { "kind": "fresh" },
                "password": password }),
            Vec::new(),
        ),
    ]
}

fn unlock(host: &mut Host, password: &str) -> serde_json::Value {
    call(
        host,
        serde_json::json!({ "kind": "document.checkPasswordPermissions", "docId": "d",
            "password": password }),
        Vec::new(),
    )
}

fn pages(host: &mut Host) -> serde_json::Value {
    call(host, serde_json::json!({ "kind": "pages.list", "docId": "d" }), Vec::new())
}

#[test]
fn a_locked_file_opens_locked_without_a_password_and_unlocks_whatever_it_came_from() {
    // `WorkerHost.handleOpen`: every input kind loads the same way with or
    // without a password, so a locked file parks and unlocks the same way
    // whatever it came from. The locked answer is the probe alone: no
    // `protection`, no numbers.
    for (kind, request, payloads) in locked_opens(None) {
        let mut host = Host::new();
        let opened = call(&mut host, request, payloads);
        let answer = &opened["result"];
        assert_eq!(answer["security"]["encryptionRequiresPassword"], true, "{kind}: {opened}");
        assert!(answer.get("protection").is_none(), "{kind}: {opened}");
        assert!(answer.get("passwordRejected").is_none(), "{kind}: {opened}");
        assert!(answer.get("lastObjectNumber").is_none(), "{kind}: {opened}");

        let locked = pages(&mut host);
        assert_eq!(locked["error"]["code"], "DocPasswordRequired", "{kind}: {locked}");

        let unlocked = unlock(&mut host, "1234");
        assert!(unlocked.get("error").is_none(), "{kind}: {unlocked}");
        assert!(unlocked["result"]["security"]["pdfOpenedAs"].is_string(), "{kind}: {unlocked}");
        let listed = pages(&mut host);
        assert!(listed["result"]["pageCount"].as_u64() > Some(0), "{kind}: {listed}");
    }
}

#[test]
fn a_wrong_password_opens_locked_and_says_it_was_wrong() {
    // `passwordRejected` when a password was given and was wrong; an unlock
    // with another wrong one is `DocPasswordIncorrect` and leaves the
    // session parked for the right one.
    for (kind, request, payloads) in locked_opens(Some("tiger")) {
        let mut host = Host::new();
        let opened = call(&mut host, request, payloads);
        assert_eq!(opened["result"]["passwordRejected"], true, "{kind}: {opened}");
        assert!(opened["result"].get("protection").is_none(), "{kind}: {opened}");

        let wrong = unlock(&mut host, "lion");
        assert_eq!(wrong["error"]["code"], "DocPasswordIncorrect", "{kind}: {wrong}");
        let still = pages(&mut host);
        assert_eq!(still["error"]["code"], "DocPasswordRequired", "{kind}: {still}");

        let unlocked = unlock(&mut host, "1234");
        assert!(unlocked.get("error").is_none(), "{kind}: {unlocked}");
        assert!(pages(&mut host)["result"]["pageCount"].as_u64() > Some(0), "{kind}");
    }
}

#[test]
fn an_empty_password_is_no_password_to_reject() {
    // `...(req.password ? { passwordRejected: true } : {})`: an empty
    // password is falsy, so the open is locked but nothing was rejected.
    let mut host = Host::new();
    let opened = call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "docId": "d", "password": "" }),
        vec![bytes(LOCKED)],
    );
    assert_eq!(opened["result"]["security"]["encryptionRequiresPassword"], true, "{opened}");
    assert!(opened["result"].get("passwordRejected").is_none(), "{opened}");
}

#[test]
fn a_locked_layer_keeps_its_object_number_authority_for_the_unlock() {
    // The open's `objectNumbers` and `signedDocumentPolicy` are set before
    // the load, so a parked layer keeps them, and the unlock opens with
    // them.
    let mut host = Host::new();
    let opened = call(
        &mut host,
        serde_json::json!({ "kind": "open.layerMemBase", "docId": "d", "baseKey": "base",
            "layer": { "kind": "fresh" }, "objectNumbers": "caller" }),
        vec![bytes(LOCKED)],
    );
    assert!(opened.get("error").is_none(), "{opened}");
    unlock(&mut host, "1234");
    let session = host
        .sessions()
        .session(&epdf_engine::session::registry::SessionKey::document("d"))
        .expect("unlocked");
    assert_eq!(
        session.object_number_authority(),
        epdf_engine::session::document::ObjectNumberAuthority::Caller
    );
    assert_eq!(session.password(), Some("1234"), "the unlock's password is the session's");
}
