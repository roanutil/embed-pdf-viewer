//! The scope an `open.fatMem` carries, and the signing capabilities.

use epdf_engine::auth::scope::{DocCapability, decode_pdf_bits, expand_raw_scope};
use epdf_engine::host::Host;
use serde_json::json;

mod common;
use common::sig_open_fixture_with;

#[test]
fn doc_sign_expands_from_form_fill_bits() {
    let bits = decode_pdf_bits(Some(1 << 8)); // bit 9: fill in forms
    let caps = expand_raw_scope(&["pdf.permissions".into()], bits).unwrap();
    assert!(caps.contains(&DocCapability::DocSign));
    assert!(!caps.contains(&DocCapability::DocSignCertify));
    assert_eq!(DocCapability::parse("doc.sign.certify"), Some(DocCapability::DocSignCertify));
}

#[test]
fn an_open_with_a_narrow_scope_refuses_a_metadata_write() {
    let mut host = Host::new();
    let doc = sig_open_fixture_with(
        &mut host,
        "pdfium-multiplatform-prototype/fixtures/report.pdf",
        json!({ "scope": ["doc.open", "doc.render"] }),
    );
    let response = host.call(
        &json!({ "kind": "metadata.update", "jobId": 9, "docId": doc, "patch": { "title": "x" } })
            .to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("Forbidden"), "{envelope}");
}

#[test]
fn an_invalid_scope_fails_the_open() {
    let mut host = Host::new();
    let bytes = std::fs::read(
        test_support::workspace_root().join("pdfium-multiplatform-prototype/fixtures/report.pdf"),
    )
    .unwrap();
    let response = host.call(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d", "scope": ["doc.nope"] })
            .to_string(),
        vec![bytes],
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("InvalidArg"), "{envelope}");
}

#[test]
fn a_parked_open_unlocks_through_a_password_check_under_its_scope() {
    let mut host = Host::new();
    let doc = sig_open_fixture_with(
        &mut host,
        "pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf",
        json!({ "scope": ["doc.open", "doc.render"] }),
    );
    let check = |host: &mut Host, password: &str| -> serde_json::Value {
        let response = host.call(
            &json!({ "kind": "document.checkPasswordPermissions", "jobId": 2, "docId": doc,
                     "password": password })
            .to_string(),
            Vec::new(),
        );
        serde_json::from_str(&response.json).unwrap()
    };
    let wrong = check(&mut host, "nope");
    assert_eq!(wrong["error"]["code"], json!("DocPasswordIncorrect"), "{wrong}");
    let right = check(&mut host, "hôtel");
    assert!(right.get("error").is_none(), "{right}");
    let pages = common::sig_call(&mut host, json!({ "kind": "pages.list", "docId": doc }));
    assert!(!pages.is_null());
    let response = host.call(
        &json!({ "kind": "metadata.update", "jobId": 3, "docId": doc, "patch": { "title": "x" } })
            .to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("Forbidden"), "{envelope}");
}
