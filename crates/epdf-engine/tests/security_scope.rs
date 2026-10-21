//! The scope an `open.fatMem` carries, and the signing capabilities.

use epdf_engine::auth::scope::{
    CollabAction, CollabTarget, DocCapability, IdentityClaims, check_collab, decode_pdf_bits,
    expand_raw_scope, materialize_pdf_permissions, parse_all, parse_scope,
};
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
    // `{ security, protection }`, as `WorkerHost` answers it.
    assert_eq!(right["result"]["security"]["pdfOpenedAs"], json!("user"), "{right}");
    assert!(right["result"].get("protection").is_some(), "{right}");
    assert!(right["result"].get("encryptionState").is_none(), "{right}");
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

// --- `pdf.permissions`, as `materializePdfPermissions` reads the bits
// (upstream `test/auth/scope/resolver.test.ts`, "bit-derived").

const MODIFY: i64 = 1 << 3;
const COPY: i64 = 1 << 4;
const ANNOTATE_FILL: i64 = 1 << 5;
const FILL_FORMS: i64 = 1 << 8;
const ASSEMBLE: i64 = 1 << 10;

fn permissions(raw: i64) -> std::collections::BTreeSet<DocCapability> {
    expand_raw_scope(&["pdf.permissions".into()], decode_pdf_bits(Some(raw))).unwrap()
}

#[test]
fn finding_and_selecting_text_need_no_bit_and_taking_it_out_needs_bit_5() {
    // 5443a5649 and a10ed2659: Acrobat finds and selects text in a file that
    // forbids copying it.
    let none = permissions(0);
    assert!(none.contains(&DocCapability::DocTextSelect));
    assert!(none.contains(&DocCapability::DocTextSearch));
    assert!(!none.contains(&DocCapability::DocTextCopy));
    assert!(!none.contains(&DocCapability::DocContentCopy));
    let copy = permissions(COPY);
    assert!(copy.contains(&DocCapability::DocTextCopy));
    assert!(copy.contains(&DocCapability::DocContentCopy));

    // `acrobat-permissions.test.ts`: the USCIS I-140 as published, P = -1052.
    let i140 = permissions(-1052);
    assert!(i140.contains(&DocCapability::DocTextSearch));
    assert!(i140.contains(&DocCapability::DocTextSelect));
    assert!(!i140.contains(&DocCapability::DocTextCopy));
}

#[test]
fn bit_4_or_bit_11_assembles_and_changes_the_outline() {
    // 5443a5649 (assembling) and 67ccb76dc (the outline).
    for raw in [MODIFY, ASSEMBLE] {
        let caps = permissions(raw);
        assert!(caps.contains(&DocCapability::DocPagesAssemble), "{raw:#x}");
        assert!(caps.contains(&DocCapability::DocOutlineModify), "{raw:#x}");
    }
    let annotate = permissions(ANNOTATE_FILL);
    assert!(!annotate.contains(&DocCapability::DocPagesAssemble));
    assert!(!annotate.contains(&DocCapability::DocOutlineModify));
}

#[test]
fn bit_4_6_or_9_fills_in_and_signs_and_only_4_with_6_changes_fields() {
    for raw in [MODIFY, ANNOTATE_FILL, FILL_FORMS] {
        let caps = permissions(raw);
        assert!(caps.contains(&DocCapability::DocFormsFill), "{raw:#x}");
        assert!(caps.contains(&DocCapability::DocSign), "{raw:#x}");
        assert!(!caps.contains(&DocCapability::DocFormsModify), "{raw:#x}");
    }
    assert!(permissions(MODIFY | ANNOTATE_FILL).contains(&DocCapability::DocFormsModify));
}

#[test]
fn every_bit_set_expands_to_exactly_the_materialized_set() {
    // Every bit set: the expansion adds nothing the materialized set lacks,
    // since every implication's source is already in it.
    let bits = decode_pdf_bits(Some(0xffff_ffff));
    let materialized = materialize_pdf_permissions(bits);
    assert_eq!(expand_raw_scope(&["pdf.permissions".into()], bits).unwrap(), materialized);
    for grant_minted in [
        DocCapability::DocFormsSubmit,
        DocCapability::DocFormsImport,
        DocCapability::DocFormsScript,
        DocCapability::DocAnnotateImport,
        DocCapability::DocDownload,
        DocCapability::DocDownloadFlattened,
        DocCapability::DocSignCertify,
    ] {
        assert!(!materialized.contains(&grant_minted), "{grant_minted} is never a bit's");
    }
}

#[test]
fn the_closed_set_takes_upstreams_grant_minted_capabilities() {
    // `parser.ts` `KNOWN_CAPABILITIES`: a token carrying one of these opens.
    for name in
        ["doc.forms.import", "doc.forms.script", "doc.annotate.import", "doc.outline.modify"]
    {
        let parsed = parse_scope(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let caps = expand_raw_scope(&[name.into()], decode_pdf_bits(None)).unwrap();
        assert_eq!(caps.len(), 1, "{name} implies nothing: {parsed:?}");
    }
}

#[test]
fn a_group_filter_matches_the_records_group_whoever_calls() {
    // 4b2cdb395: `filterMatches` dropped the membership test. The scope
    // names the group; which groups the caller is in plays no part.
    let parsed = parse_all(&["annotations:update:group=4".into()]).unwrap();
    let bits = decode_pdf_bits(None);
    let outsider = IdentityClaims {
        user_id: Some("alice".into()),
        group_id: Some("9".into()),
        groups: vec!["9".into()],
        display_name: None,
    };
    let in_four = CollabTarget { user_id: Some("bob".into()), group_id: Some("4".into()) };
    assert!(check_collab(CollabAction::Update, &in_four, &parsed, &outsider, bits));
    assert!(check_collab(
        CollabAction::Update,
        &in_four,
        &parsed,
        &IdentityClaims::default(),
        bits
    ));

    let in_five = CollabTarget { user_id: Some("bob".into()), group_id: Some("5".into()) };
    assert!(!check_collab(CollabAction::Update, &in_five, &parsed, &outsider, bits));
    let in_none = CollabTarget { user_id: Some("bob".into()), group_id: None };
    assert!(!check_collab(CollabAction::Update, &in_none, &parsed, &outsider, bits));
}

#[test]
fn a_file_probe_answers_its_security_under_a_key() {
    // `{ tag: 'document.probeSecurityFile', security }`
    // (`WorkerHost.handleDocumentProbeSecurityFile`).
    let mut host = Host::new();
    let path = test_support::workspace_root()
        .join("pdfium-multiplatform-prototype/fixtures/encrypted-hello.pdf");
    let response = host.call(
        &json!({ "kind": "document.probeSecurityFile", "jobId": 1,
                 "path": path.to_string_lossy() })
        .to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["result"]["security"]["encryptionState"], json!("encrypted"), "{envelope}");
    assert_eq!(envelope["result"]["security"]["encryptionRequiresPassword"], json!(true));
    assert!(envelope["result"].get("encryptionState").is_none(), "{envelope}");
}
