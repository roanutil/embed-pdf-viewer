//! `signatures.prepare`, `complete` and `abort`, and the visual fill, over
//! the signing fixtures. The CMS comes from `epdf-signature-core`'s own
//! builder with the TEST-ONLY key, so the engine is tested against the CMS
//! the plugin hands it.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{
    call_legacy, sig_call, sig_call_bytes_with, sig_call_with_payload, sig_open_fixture,
    sig_open_fixture_with,
};

const FIX: &str = "packages/engine/main/test/fixtures";

fn key_file(name: &str) -> String {
    std::fs::read_to_string(
        test_support::workspace_root().join(format!("{FIX}/signature-compat/keys/{name}")),
    )
    .unwrap()
}

fn test_cms(digest: &[u8]) -> Vec<u8> {
    epdf_signature_core::harness::test_cms_over_digest(
        digest,
        &key_file("TEST-ONLY-private-key.pem"),
        &key_file("TEST-ONLY-signer.pem"),
    )
}

fn envelope(host: &mut Host, request: Value, payloads: Vec<Vec<u8>>) -> Value {
    let mut request = request;
    request["jobId"] = json!(7);
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).unwrap()
}

fn unsigned_field(host: &mut Host, doc: &str) -> Value {
    sig_call(host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"]
        .clone()
}

#[test]
fn prepare_then_complete_seals_the_field() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let before =
        sig_call(&mut host, json!({ "kind": "document.version", "docId": doc }))["version"].clone();
    let (prepared, digest) = sig_call_bytes_with(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc,
                "input": { "field": field, "attribution": { "name": "Test Signer" } } }),
    );
    assert_eq!(digest.len(), 32);
    assert_eq!(prepared["algorithm"], json!("sha256"));
    assert_eq!(prepared["contentsSize"], json!(8192));
    assert_eq!(prepared["subFilter"], json!("ETSI.CAdES.detached"));
    assert_eq!(prepared["expectedVersion"]["baseSha256"], before["sha256"]);
    let cms = test_cms(&digest);
    let input = json!({ "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] });
    let completed = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc, "input": input }),
        cms.clone(),
    );
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");
    assert_eq!(completed["event"]["type"], json!("signature.completed"));
    assert_eq!(completed["event"]["signingId"], prepared["signingId"]);
    assert_ne!(completed["result"]["version"]["sha256"], before["sha256"]);
    let snapshot = sig_call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["signatures"][0]["signed"], json!(true));
    assert_eq!(snapshot["signatures"][0]["coverage"], json!("whole-revision"));
    assert_eq!(snapshot["signatures"][0]["signer"]["name"], json!("Test Signer"));
    // The installed bytes carry the CMS that was handed in.
    let contents = common::sig_call_bytes(
        &mut host,
        json!({ "kind": "signatures.contents", "docId": doc, "ref": field }),
    );
    assert_eq!(contents, cms);
    // The new signature seals the last revision: nothing follows it.
    let analysis = sig_call(
        &mut host,
        json!({ "kind": "signatures.analyze", "docId": doc,
                "input": { "since": { "signatureIndex": 0 }, "until": "persisted" } }),
    );
    assert_eq!(analysis["verdict"], json!("unchanged"), "{analysis}");
    // Replaying the same CMS is idempotent.
    let replay = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc, "input": input }),
        cms,
    );
    assert_eq!(replay["result"]["status"], json!("already-completed"));
    // Removed upstream, renamed `signatures.cancel`; owner 8.26.
    let again = call_legacy(
        &mut host,
        json!({ "kind": "signatures.abort", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(again["status"], json!("already-completed"));
}

#[test]
fn abort_discards_a_prepared_signing() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let (prepared, _) = sig_call_bytes_with(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    // Removed upstream, renamed `signatures.cancel`; owner 8.26.
    let aborted = call_legacy(
        &mut host,
        json!({ "kind": "signatures.abort", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(aborted["status"], json!("aborted"));
    let again = call_legacy(
        &mut host,
        json!({ "kind": "signatures.abort", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(again["status"], json!("unknown"));
}

#[test]
fn a_pending_signing_refuses_mutations_and_a_second_prepare() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    sig_call_bytes_with(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    let write = envelope(
        &mut host,
        json!({ "kind": "metadata.update", "docId": doc, "patch": { "title": "x" } }),
        Vec::new(),
    );
    assert_eq!(write["error"]["code"], json!("SigningPending"), "{write}");
    let second = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    assert_eq!(second["error"]["code"], json!("SigningPending"), "{second}");
}

#[test]
fn a_visual_fill_draws_into_an_unsigned_field() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let artwork =
        std::fs::read(test_support::workspace_root().join(format!("{FIX}/signature_artwork.pdf")))
            .unwrap();
    let filled = sig_call_with_payload(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": field, "pageIndex": 0 }),
        artwork,
    );
    assert_eq!(filled["event"]["type"], json!("form.fieldUpdated"), "{filled}");
    assert_eq!(filled["result"]["field"]["family"], json!("signature"));
    assert!(!filled["result"]["meta"]["affectedPages"].as_array().unwrap().is_empty(), "{filled}");
}

#[test]
fn completing_after_the_document_moved_is_a_version_mismatch() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let (prepared, digest) = sig_call_bytes_with(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    let mut stale = prepared["expectedVersion"].clone();
    stale["editsVersion"] = json!(stale["editsVersion"].as_u64().unwrap() + 1);
    let response = envelope(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc,
                "input": { "signingId": prepared["signingId"], "expectedVersion": stale } }),
        vec![test_cms(&digest)],
    );
    assert_eq!(response["error"]["code"], json!("SigningVersionMismatch"), "{response}");
}

#[test]
fn a_cms_that_is_not_der_is_refused() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let (prepared, _) = sig_call_bytes_with(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    let response = envelope(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc,
                "input": { "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] } }),
        vec![vec![1, 2, 3]],
    );
    assert_eq!(response["error"]["code"], json!("SignatureRefused"), "{response}");
}

#[test]
fn signing_a_signed_field_is_refused() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let response = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    assert_eq!(response["error"]["code"], json!("SignatureRefused"), "{response}");
}

#[test]
fn without_doc_sign_a_prepare_is_forbidden() {
    let mut host = Host::new();
    let doc = sig_open_fixture_with(
        &mut host,
        &format!("{FIX}/unsigned_sigfield.pdf"),
        json!({ "scope": ["doc.open", "doc.render", "doc.forms.read"] }),
    );
    let field = unsigned_field(&mut host, &doc);
    let response = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    assert_eq!(response["error"]["code"], json!("Forbidden"), "{response}");
    let certify = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field, "certify": { "permission": 2 } } }),
        Vec::new(),
    );
    assert_eq!(certify["error"]["code"], json!("Forbidden"), "{certify}");
}

// ── document-derived protection (final review) ───────────────────────────────

#[test]
fn a_certification_narrows_the_session_authority() {
    use epdf_engine::auth::scope::DocCapability;
    let mut session = epdf_engine::session::document::DocumentSession::new();
    session.open(test_support::fixture(&format!("{FIX}/signed_certified.pdf")), None).unwrap();
    // P=2: form fill and signing only, whatever the scope grants.
    let authority = session.authority();
    assert!(!authority.allows(DocCapability::DocPagesModify));
    assert!(!authority.allows(DocCapability::DocAnnotateModify));
    assert!(authority.allows(DocCapability::DocFormsFill));
    assert!(authority.allows(DocCapability::DocSign));
}

#[test]
fn a_field_locked_by_a_signature_refuses_a_write() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_fieldmdp.pdf"));
    let response = envelope(
        &mut host,
        json!({ "kind": "forms.setValue", "docId": doc,
                "ref": { "kind": "objectNumber", "objectNumber": 4 },
                "value": { "type": "text", "value": "changed" } }),
        Vec::new(),
    );
    assert_eq!(response["error"]["code"], json!("ProtectedDocument"), "{response}");
}

#[test]
fn signature_reads_are_gated_as_the_typescript_gates_them() {
    let mut host = Host::new();
    let doc = sig_open_fixture_with(
        &mut host,
        &format!("{FIX}/signed_two_approvals.pdf"),
        json!({ "scope": ["doc.open", "doc.render"] }),
    );
    let list = envelope(&mut host, json!({ "kind": "signatures.list", "docId": doc }), Vec::new());
    assert_eq!(list["error"]["code"], json!("Forbidden"), "{list}");
    let revision = envelope(
        &mut host,
        json!({ "kind": "signatures.revisionBytes", "docId": doc, "revisionIndex": 0 }),
        Vec::new(),
    );
    assert_eq!(revision["error"]["code"], json!("Forbidden"), "{revision}");
    let analyze = envelope(
        &mut host,
        json!({ "kind": "signatures.analyze", "docId": doc, "input": { "since": { "signatureIndex": 0 } } }),
        Vec::new(),
    );
    assert_eq!(analyze["error"]["code"], json!("Forbidden"), "{analyze}");
    // Form read grants the list, not the revision bytes (`doc.download`).
    let reader = sig_open_fixture_with(
        &mut host,
        &format!("{FIX}/signed_certified.pdf"),
        json!({ "scope": ["doc.open", "doc.forms.read"] }),
    );
    sig_call(&mut host, json!({ "kind": "signatures.list", "docId": reader }));
    let bytes = envelope(
        &mut host,
        json!({ "kind": "signatures.revisionBytes", "docId": reader, "revisionIndex": 0 }),
        Vec::new(),
    );
    assert_eq!(bytes["error"]["code"], json!("Forbidden"), "{bytes}");
}
