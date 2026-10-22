//! `signatures.prepare`, `complete` and `cancel`, the visual fill
//! (`forms.setSignatureAppearance`, a verb and a change op), and what
//! `signedDocumentPolicy` enforces, over the signing fixtures. The CMS comes
//! from `epdf-signature-core`'s own builder with the TEST-ONLY key, so the
//! engine is tested against the CMS the plugin hands it.
//!
//! Ports `packages/engine/main/test/signature-visual-fill.test.ts`, the
//! signing cases of `runSignatureConformance.ts` and the two signing cases
//! of `runFieldGroupsConformance.ts`.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{
    page_ref, sig_call, sig_call_bytes, sig_call_with_payload, sig_list, sig_open_fixture,
    sig_open_fixture_with, sig_prepare,
};

const FIX: &str = "packages/engine/main/test/fixtures";
/// A DER `SEQUENCE`: what a completion takes as a CMS, without judging it.
const PLACEHOLDER_CMS: [u8; 8] = [0x30, 0x06, 0x02, 0x01, 0x01, 0x02, 0x01, 0x02];

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

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(format!("{FIX}/{name}"))).unwrap()
}

fn envelope(host: &mut Host, request: Value, payloads: Vec<Vec<u8>>) -> Value {
    let mut request = request;
    request["jobId"] = json!(7);
    let response = host.call(&request.to_string(), payloads);
    serde_json::from_str(&response.json).unwrap()
}

fn unsigned_field(host: &mut Host, doc: &str) -> Value {
    sig_list(host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"]
        .clone()
}

fn fqn(name: &str) -> Value {
    json!({ "kind": "fqn", "name": name })
}

/// Prepare and complete a signing of `field` with the placeholder CMS.
fn sign(host: &mut Host, doc: &str, input: Value) -> Value {
    let (prepared, _) =
        sig_prepare(host, json!({ "kind": "signatures.prepare", "docId": doc, "input": input }));
    sig_call_with_payload(
        host,
        json!({ "kind": "signatures.complete", "docId": doc,
            "input": { "signingId": prepared["signingId"],
                "expectedVersion": prepared["expectedVersion"] } }),
        PLACEHOLDER_CMS.to_vec(),
    )
}

#[test]
fn prepare_then_complete_seals_the_field() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let before =
        sig_call(&mut host, json!({ "kind": "document.version", "docId": doc }))["version"].clone();
    let (prepared, digest) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc,
                "input": { "field": field, "signer": { "name": "Test Signer",
                    "reason": "approved", "signedAt": "2026-10-08T10:42:00+01:00" } } }),
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
        json!({ "kind": "signatures.complete", "docId": doc, "opId": "sign-1", "input": input }),
        cms.clone(),
    );
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");
    // A signing is final: its own write, never undoable.
    assert_eq!(completed["result"]["meta"]["opId"], json!("sign-1"), "{completed}");
    assert_eq!(completed["result"]["meta"]["undoable"], json!(false));
    assert_eq!(completed["result"]["meta"]["cacheDelta"], Value::Null);
    let pages = completed["result"]["meta"]["affectedPages"].as_array().unwrap();
    assert!(!pages.is_empty() && pages.iter().all(|p| p["kind"] == "objectNumber"), "{completed}");
    assert_eq!(completed["event"]["type"], json!("signature.completed"));
    assert_eq!(completed["event"]["signingId"], prepared["signingId"]);
    assert_ne!(completed["result"]["version"]["sha256"], before["sha256"]);
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let signature = &snapshot["signatures"][0];
    assert_eq!(signature["signed"], json!(true));
    assert_eq!(signature["coverage"], json!("whole-revision"));
    // The signer's facts, `/M` read back as an `IsoDateTime`.
    assert_eq!(
        signature["signer"],
        json!({ "name": "Test Signer", "reason": "approved", "location": null,
            "contactInfo": null, "signedAt": "2026-10-08T10:42:00+01:00" })
    );
    // The installed bytes carry the CMS that was handed in.
    let contents = sig_call_bytes(
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
    assert_eq!(analysis["analysis"]["verdict"], json!("unchanged"), "{analysis}");
    // Replaying the same CMS is idempotent.
    let replay = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc, "input": input }),
        cms,
    );
    assert_eq!(replay["result"]["status"], json!("already-completed"));
    let again = sig_call(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(again, json!({ "result": { "status": "already-completed" } }));
}

#[test]
fn a_signed_fat_memory_session_stays_a_layer_that_persists_no_artifact() {
    // `install` opens the sealed bytes as a layer, and a session that came
    // from bytes keeps `persistLayerArtifact = false`
    // (`SignatureMutator.verifySealed`): a write after the signing answers
    // no artifact, and the session still hands out object numbers.
    use epdf_engine::session::document::OpenKind;
    use epdf_engine::session::registry::SessionKey;

    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let (prepared, digest) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc,
                "input": { "field": field, "signer": { "name": "Test Signer" } } }),
    );
    let input = json!({ "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] });
    let completed = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc, "input": input }),
        test_cms(&digest),
    );
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");

    let session = host.sessions().session(&SessionKey::document(&doc)).expect("the session");
    assert_eq!(session.kind(), Some(OpenKind::Layer));
    assert!(!session.persists_layer_artifact());
    let written = host.call(
        &json!({ "kind": "metadata.update", "jobId": 8, "docId": doc, "opId": "after-sign",
            "patch": { "title": "Signed" } })
        .to_string(),
        Vec::new(),
    );
    assert!(!written.json.contains("\"error\""), "{}", written.json);
    assert!(written.payloads.is_empty(), "a write after the signing answered an artifact");
    let reserved = envelope(
        &mut host,
        json!({ "kind": "objectNumbers.reserve", "docId": doc, "count": 1 }),
        Vec::new(),
    );
    assert!(reserved["result"]["range"]["first"].as_u64().is_some(), "{reserved}");
}

#[test]
fn cancel_discards_a_prepared_signing_and_lifts_the_fence() {
    // `runSignatureConformance` "sign: cancel discards the candidate and
    // lifts the fence".
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let v0 =
        sig_call(&mut host, json!({ "kind": "document.version", "docId": doc }))["version"].clone();
    let (prepared, _) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    let cancelled = sig_call(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(cancelled["result"]["status"], json!("cancelled"), "{cancelled}");
    let listed = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(listed["signatures"][0]["signed"], json!(false));
    let v1 =
        sig_call(&mut host, json!({ "kind": "document.version", "docId": doc }))["version"].clone();
    assert_eq!(v1["sha256"], v0["sha256"]);
    // The fence is gone: a write runs.
    sig_call(
        &mut host,
        json!({ "kind": "metadata.update", "docId": doc, "patch": { "title": "free again" } }),
    );
    let gone = envelope(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc,
            "input": { "signingId": prepared["signingId"],
                "expectedVersion": prepared["expectedVersion"] } }),
        vec![PLACEHOLDER_CMS.to_vec()],
    );
    assert_eq!(gone["error"]["code"], json!("NotFound"), "{gone}");
    let again = sig_call(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": doc, "signingId": prepared["signingId"] }),
    );
    assert_eq!(again["result"]["status"], json!("unknown"));
}

#[test]
fn a_pending_signing_refuses_mutations_and_a_second_prepare() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
    );
    let write = envelope(
        &mut host,
        json!({ "kind": "metadata.update", "docId": doc, "patch": { "title": "x" } }),
        Vec::new(),
    );
    assert_eq!(write["error"]["code"], json!("SigningPending"), "{write}");
    assert!(
        write["error"]["message"].as_str().unwrap_or("").contains("complete or cancel it"),
        "{write}"
    );
    let second = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    assert_eq!(second["error"]["code"], json!("SigningPending"), "{second}");
}

#[test]
fn a_signature_field_is_authored_filled_visually_and_refuses_a_visual_fill_once_signed() {
    // `signature-visual-fill.test.ts`.
    let mut host = Host::new();
    let (doc, pon) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let artwork = fixture("signature_artwork.pdf");
    let created = sig_call(
        &mut host,
        json!({ "kind": "forms.createField", "docId": doc, "draft": { "family": "signature",
            "name": "sig2", "widgets": [{ "page": page_ref(pon),
                "rect": { "x": 50, "y": 50, "width": 200, "height": 70 } }] } }),
    );
    assert_eq!(created["result"]["field"]["family"], json!("signature"), "{created}");
    let listed = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let names: Vec<(Value, Value)> = listed["signatures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["fieldName"].clone(), s["signed"].clone()))
        .collect();
    assert_eq!(names, [(json!("sig"), json!(false)), (json!("sig2"), json!(false))]);

    // The mark is drawn into the widget; the field stays unsigned.
    let filled = sig_call_with_payload(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "opId": "fill-1",
            "ref": fqn("sig2") }),
        artwork.clone(),
    );
    assert_eq!(filled["result"]["field"]["name"], json!("sig2"), "{filled}");
    assert_eq!(filled["event"]["type"], json!("forms.updated"), "{filled}");
    let meta = &filled["result"]["meta"];
    assert_eq!(meta["affectedPages"], json!([page_ref(pon)]), "{filled}");
    assert_eq!(meta["opId"], json!("fill-1"));
    assert_eq!(meta["undoable"], json!(true));
    // Its widget is drawn again, and answered as a row on its page.
    assert_eq!(meta["redrawnWidgets"].as_array().map(Vec::len), Some(1), "{filled}");
    assert_eq!(filled["result"]["widgets"].as_array().map(Vec::len), Some(1), "{filled}");
    let listed = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(listed["signatures"][1]["signed"], json!(false));

    let not_a_signature = envelope(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": fqn("group.total") }),
        vec![artwork.clone()],
    );
    assert_eq!(not_a_signature["error"]["code"], json!("InvalidArg"), "{not_a_signature}");
    // A signature appearance takes a one-page PDF.
    let two_pages = envelope(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": fqn("sig2") }),
        vec![fixture("two_page_sigfield.pdf")],
    );
    assert_eq!(two_pages["error"]["code"], json!("InvalidArg"), "{two_pages}");
    assert!(
        two_pages["error"]["message"].as_str().unwrap_or("").contains("has 2 pages"),
        "{two_pages}"
    );

    // Sign the other field, with its artwork; the facts land in it.
    let prepared = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc,
            "input": { "field": fqn("sig"), "appearance": { "pdf": { "payload": 0 } },
                "signer": { "name": "Bob Singor", "reason": "approved",
                    "location": "Amsterdam" } } }),
        vec![artwork.clone()],
    )["result"]["result"]
        .take();
    let completed = sig_call_with_payload(
        &mut host,
        json!({ "kind": "signatures.complete", "docId": doc,
            "input": { "signingId": prepared["signingId"],
                "expectedVersion": prepared["expectedVersion"] } }),
        PLACEHOLDER_CMS.to_vec(),
    );
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");
    assert_eq!(completed["result"]["signature"]["signer"]["location"], json!("Amsterdam"));
    // A signed field's appearance is sealed with the signature.
    let sealed = envelope(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": fqn("sig") }),
        vec![artwork.clone()],
    );
    assert_eq!(sealed["error"]["code"], json!("ProtectedDocument"), "{sealed}");
    // The unsigned field can still be drawn after the document was
    // versioned.
    let again = sig_call_with_payload(
        &mut host,
        json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": fqn("sig2") }),
        artwork,
    );
    assert_eq!(again["result"]["field"]["family"], json!("signature"), "{again}");
}

#[test]
fn a_prepare_draws_its_artwork_from_the_payload_its_appearance_names() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let request = |pdf: Value| {
        json!({ "kind": "signatures.prepare", "docId": doc,
            "input": { "field": fqn("sig"), "appearance": { "pdf": pdf } } })
    };
    // No payload where the appearance points.
    let missing = envelope(
        &mut host,
        request(json!({ "payload": 1 })),
        vec![fixture("signature_artwork.pdf")],
    );
    assert_eq!(missing["error"]["code"], json!("WireFormat"), "{missing}");
    // A PDF of more than one page.
    let two_pages = envelope(
        &mut host,
        request(json!({ "payload": 0 })),
        vec![fixture("two_page_sigfield.pdf")],
    );
    assert_eq!(two_pages["error"]["code"], json!("InvalidArg"), "{two_pages}");
    // Nothing was parked by either: a one-page artwork prepares.
    let prepared = envelope(
        &mut host,
        request(json!({ "payload": 0 })),
        vec![fixture("signature_artwork.pdf")],
    );
    assert!(prepared["result"]["result"]["signingId"].is_string(), "{prepared}");
}

#[test]
fn a_visual_fill_is_a_change_op_its_undo_takes_back_and_only_a_signer_may_undo() {
    // `forms.setSignatureAppearance` inside `document.apply`, its PDF a
    // payload; its reverse puts the widget's appearance back, and an undo
    // is checked as signing (`authorizeRevertedFills`).
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let artwork = fixture("signature_artwork.pdf");
    let bits = json!({ "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
        "bit10": true, "bit11": true, "bit12": true });
    let authority = |scope: Value| {
        json!({ "identity": { "userId": "ann" }, "grants": { "scope": scope, "pdfBits": bits },
            "protection": null })
    };
    let read_widget = |host: &mut Host| {
        let form = sig_call(host, json!({ "kind": "forms.list", "docId": doc }));
        form["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "sig")
            .cloned()
            .expect("the signature field")
    };
    let before = read_widget(&mut host);
    let applied = sig_call_with_payload(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "look-1",
            "authority": authority(json!(["*"])),
            "change": { "ops": [{ "type": "forms.setSignatureAppearance", "field": fqn("sig"),
                "appearance": { "pdf": { "payload": 0 } } }] } }),
        artwork,
    );
    let item = &applied["result"]["items"][0];
    assert_eq!(item["type"], json!("forms.setSignatureAppearance"), "{applied}");
    assert_eq!(item["field"]["name"], json!("sig"));
    assert_eq!(applied["events"][0]["type"], json!("forms.updated"), "{applied}");

    // A fill-only caller may not undo a signature's look: it is signing.
    let refused = envelope(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc,
            "authority": authority(json!(["doc.open", "doc.forms.read", "doc.forms.fill"])),
            "change": { "undoOf": "look-1" } }),
        Vec::new(),
    );
    assert_eq!(refused["error"]["code"], json!("Forbidden"), "{refused}");
    assert_eq!(refused["error"]["details"]["required"], json!("doc.sign"), "{refused}");

    let undone = sig_call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc,
            "authority": authority(json!(["*"])), "change": { "undoOf": "look-1" } }),
    );
    assert_eq!(
        undone["result"]["items"][0]["type"],
        json!("forms.setSignatureAppearance"),
        "{undone}"
    );
    assert_eq!(read_widget(&mut host), before);
}

#[test]
fn completing_after_the_document_moved_is_a_version_mismatch() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = unsigned_field(&mut host, &doc);
    let (prepared, digest) = sig_prepare(
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
    let (prepared, _) = sig_prepare(
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

// ── field groups (`runFieldGroupsConformance`) ─────────────────────────────

/// `pdfOf`: a PDF of `objects`, numbered from 1.
fn pdf_of(objects: &[String]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// `FIELD_GROUPS_PDF`: a buyer's and a seller's fields, each field's group
/// in its `/EMBD_Metadata /GroupID`.
fn field_groups_pdf() -> Vec<u8> {
    let group = |name: &str| format!("/EMBD_Metadata << /SchemaVersion 1 /GroupID ({name}) >>");
    let widget = |ft: &str, name: &str, rect: &str, extra: &str| {
        format!(
            "<< /Type /Annot /Subtype /Widget /FT /{ft} /T ({name}) /Rect [{rect}] /F 4 /P 3 0 R \
             {extra} >>"
        )
    };
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] \
         /CO [7 0 R] /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv << /Type /Font /Subtype /Type1 \
         /BaseFont /Helvetica >> >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 10 0 R /Resources << >> \
         /Annots [4 0 R 5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >>"
            .to_owned(),
        widget("Tx", "buyer_name", "20 340 180 360", &group("buyer")),
        widget("Tx", "buyer_price", "20 300 180 320", &group("buyer")),
        widget("Tx", "seller_name", "200 340 380 360", &group("seller")),
        widget("Tx", "total", "20 260 180 280", "/AA << /C << /S /JavaScript /JS (event.value = 1;) >> >>"),
        widget("Sig", "buyer_sig", "20 100 180 150", &group("buyer")),
        widget("Sig", "seller_sig", "200 100 380 150", &group("seller")),
        "<< /Length 0 >>\nstream\n\nendstream".to_owned(),
    ])
}

/// A host with `FIELD_GROUPS_PDF` open as `"groups"` under `scope`.
fn open_groups(scope: Value, identity: Value) -> Host {
    let mut host = Host::new();
    let response = host.call(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "groups", "scope": scope,
            "identity": identity })
        .to_string(),
        vec![field_groups_pdf()],
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    host
}

#[test]
fn each_signer_signs_only_their_own_signature_field() {
    let mut host = open_groups(
        json!(["doc.open", "doc.render", "fields:fill:group=buyer", "fields:sign:group=buyer"]),
        json!({ "userId": "alice", "displayName": "Alice" }),
    );
    let refused = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": "groups",
            "input": { "field": fqn("seller_sig") } }),
        Vec::new(),
    );
    assert_eq!(refused["error"]["code"], json!("Forbidden"), "{refused}");
    assert_eq!(
        refused["error"]["details"]["required"],
        json!("fields:sign:group=seller"),
        "{refused}"
    );
    let (prepared, _) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": "groups",
            "input": { "field": fqn("buyer_sig") } }),
    );
    let cancelled = sig_call(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": "groups",
            "signingId": prepared["signingId"] }),
    );
    assert_eq!(cancelled["result"]["status"], json!("cancelled"));
}

#[test]
fn signing_a_grouped_field_locks_the_rest_of_its_group_for_everyone() {
    // The lock follows the field's group, whoever signs: here a caller who
    // may do everything, whom the lock then refuses too.
    let mut host = open_groups(json!(["*"]), json!({}));
    sig_call(
        &mut host,
        json!({ "kind": "forms.setValue", "docId": "groups", "ref": fqn("buyer_name"),
            "value": { "value": "Alice" } }),
    );
    let completed = sign(&mut host, "groups", json!({ "field": fqn("buyer_sig") }));
    assert_eq!(completed["result"]["status"], json!("completed"), "{completed}");
    let lock = json!({ "action": "include", "fields": ["buyer_name", "buyer_price"] });
    assert_eq!(completed["result"]["signature"]["fieldMdp"], lock, "{completed}");
    let locked = envelope(
        &mut host,
        json!({ "kind": "forms.setValue", "docId": "groups", "ref": fqn("buyer_name"),
            "value": { "value": "x" } }),
        Vec::new(),
    );
    assert_eq!(locked["error"]["code"], json!("ProtectedDocument"), "{locked}");
    // The other group's fields, and its signature field, stay open.
    sig_call(
        &mut host,
        json!({ "kind": "forms.setValue", "docId": "groups", "ref": fqn("seller_name"),
            "value": { "value": "Bob" } }),
    );
    let (next, _) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": "groups",
            "input": { "field": fqn("seller_sig") } }),
    );
    sig_call(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": "groups", "signingId": next["signingId"] }),
    );
}

#[test]
fn a_request_authority_decides_who_signs() {
    // `req.authority` names who signs: a seller's authority on a session
    // that may do everything is refused the buyer's field.
    let mut host = open_groups(json!(["*"]), json!({}));
    let seller = json!({ "identity": { "userId": "bob" }, "grants": {
        "scope": ["doc.open", "fields:*:group=seller"],
        "pdfBits": { "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
            "bit10": true, "bit11": true, "bit12": true } }, "protection": null });
    let refused = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": "groups", "authority": seller,
            "input": { "field": fqn("buyer_sig") } }),
        Vec::new(),
    );
    assert_eq!(refused["error"]["code"], json!("Forbidden"), "{refused}");
    let (prepared, _) = sig_prepare(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": "groups", "authority": seller,
            "input": { "field": fqn("seller_sig") } }),
    );
    // Its completion is checked again, under the authority it carries.
    let buyer = json!({ "identity": { "userId": "alice" }, "grants": {
        "scope": ["doc.open", "fields:*:group=buyer"],
        "pdfBits": { "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
            "bit10": true, "bit11": true, "bit12": true } }, "protection": null });
    let complete = |authority: &Value| {
        json!({ "kind": "signatures.complete", "docId": "groups", "authority": authority,
            "input": { "signingId": prepared["signingId"],
                "expectedVersion": prepared["expectedVersion"] } })
    };
    let wrong = envelope(&mut host, complete(&buyer), vec![PLACEHOLDER_CMS.to_vec()]);
    assert_eq!(wrong["error"]["code"], json!("Forbidden"), "{wrong}");
    let done = envelope(&mut host, complete(&seller), vec![PLACEHOLDER_CMS.to_vec()]);
    assert_eq!(done["result"]["result"]["status"], json!("completed"), "{done}");
}

// ── signedDocumentPolicy ─────────────────────────────────────────────────────

#[test]
fn a_rewrite_save_of_a_signed_document_is_refused_unless_the_session_permits_it() {
    // `WorkerHost.assertRewriteKeepsSignatures`: a rewrite would void every
    // signature; an incremental save keeps them.
    let save = |host: &mut Host, doc: &str, mode: &str| {
        envelope(host, json!({ "kind": "document.saveBuffer", "docId": doc, "mode": mode }), vec![])
    };
    let mut host = Host::new();
    let (signed, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let refused = save(&mut host, &signed, "rewrite");
    assert_eq!(refused["error"]["code"], json!("ProtectedDocument"), "{refused}");
    assert!(save(&mut host, &signed, "incremental").get("error").is_none());
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("rewrite-{}.pdf", std::process::id()));
    let to_file = envelope(
        &mut host,
        json!({ "kind": "document.saveFile", "docId": signed, "mode": "rewrite",
            "path": path.to_string_lossy() }),
        vec![],
    );
    assert_eq!(to_file["error"]["code"], json!("ProtectedDocument"), "{to_file}");
    assert!(!path.exists(), "a refused save wrote its file");

    let permitted = sig_open_fixture_with(
        &mut host,
        &format!("{FIX}/signed_certified.pdf"),
        json!({ "signedDocumentPolicy": "permit" }),
    );
    assert!(save(&mut host, &permitted, "rewrite").get("error").is_none());
    let (unsigned, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    assert!(save(&mut host, &unsigned, "rewrite").get("error").is_none());
}

#[test]
fn under_permit_a_signature_s_field_lock_holds_nothing() {
    // `runSignatureConformance`: the locked field refuses a write under
    // `protect`, and takes it under `permit`.
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_fieldmdp.pdf"));
    let write = |host: &mut Host, doc: &str| {
        envelope(
            host,
            json!({ "kind": "forms.setValue", "docId": doc, "ref": fqn("Text Box"),
                "value": { "value": "changed" } }),
            Vec::new(),
        )
    };
    assert_eq!(write(&mut host, &doc)["error"]["code"], json!("ProtectedDocument"));
    let mut host = Host::new();
    let permitted = sig_open_fixture_with(
        &mut host,
        &format!("{FIX}/signed_fieldmdp.pdf"),
        json!({ "signedDocumentPolicy": "permit" }),
    );
    let answer = write(&mut host, &permitted);
    assert_eq!(answer["result"]["result"]["field"]["name"], json!("Text Box"), "{answer}");
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
    assert!(!authority.allows(DocCapability::DocOutlineModify));
    assert!(!authority.allows(DocCapability::DocFormsScript));
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
    sig_list(&mut host, json!({ "kind": "signatures.list", "docId": reader }));
    let bytes = envelope(
        &mut host,
        json!({ "kind": "signatures.revisionBytes", "docId": reader, "revisionIndex": 0 }),
        Vec::new(),
    );
    assert_eq!(bytes["error"]["code"], json!("Forbidden"), "{bytes}");
}
