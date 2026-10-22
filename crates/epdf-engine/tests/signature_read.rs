//! `signatures.list`, `signatures.contents`, `signatures.revisionBytes` and
//! `document.version` over the signing fixtures.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{sig_call, sig_call_bytes, sig_list, sig_open_fixture};

const FIX: &str = "packages/engine/main/test/fixtures";

#[test]
fn two_approvals_list_in_field_order_with_whole_revision_coverage() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["chainValid"], json!(true));
    let signatures = snapshot["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 2);
    for (i, s) in signatures.iter().enumerate() {
        assert_eq!(s["index"], json!(i));
        assert_eq!(s["signed"], json!(true));
        assert_eq!(s["coverage"], json!("whole-revision"));
        assert_eq!(s["field"]["kind"], json!("objectNumber"));
        assert_eq!(s["revisionIndex"], json!(i + 1));
    }
    let revisions = snapshot["revisions"].as_array().unwrap();
    assert_eq!(revisions.len(), 3);
    assert_eq!(revisions[0]["signatureIndex"], Value::Null);
    assert_eq!(revisions[1]["signatureIndex"], json!(0));
    assert_eq!(snapshot["protection"]["enforced"], Value::Null);
    // `deriveProtection`'s approval baseline, policy version 4.
    assert_eq!(snapshot["protection"]["judged"], json!("annotate"));
    assert_eq!(snapshot["protection"]["policyVersion"], json!(4));
}

#[test]
fn a_certification_is_named_in_protection() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_certified.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["signatures"][0]["catalogCertification"], json!(true));
    assert!(snapshot["protection"]["certification"].is_object(), "{snapshot}");
}

#[test]
fn an_unsigned_field_lists_unsigned() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let s = &snapshot["signatures"][0];
    assert_eq!(s["signed"], json!(false));
    assert_eq!(s["coverage"], Value::Null);
    assert_eq!(s["byteRange"], Value::Null);
    assert!(s["widget"]["objectNumber"].as_u64().unwrap() > 0, "{s}");
    assert_eq!(s["widget"]["page"]["kind"], json!("objectNumber"));
    assert_eq!(snapshot["protection"]["judged"], Value::Null);
}

#[test]
fn contents_and_revision_bytes_are_payloads() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let field = snapshot["signatures"][0]["field"].clone();
    let cms = sig_call_bytes(
        &mut host,
        json!({ "kind": "signatures.contents", "docId": doc, "ref": field }),
    );
    assert_eq!(cms[0], 0x30, "a DER SEQUENCE");
    let revision = sig_call_bytes(
        &mut host,
        json!({ "kind": "signatures.revisionBytes", "docId": doc, "revisionIndex": 0 }),
    );
    assert!(revision.starts_with(b"%PDF-"));
    let version =
        sig_call(&mut host, json!({ "kind": "document.version", "docId": doc }))["version"].clone();
    assert_eq!(version["sha256"].as_str().unwrap().len(), 64);
    assert!(version["byteLength"].as_u64().unwrap() > revision.len() as u64);
}

#[test]
fn the_contents_of_an_unsigned_field_are_not_found() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let field = snapshot["signatures"][0]["field"].clone();
    let response = host.call(
        &json!({ "kind": "signatures.contents", "jobId": 1, "docId": doc, "ref": field })
            .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("NotFound"), "{envelope}");
}
