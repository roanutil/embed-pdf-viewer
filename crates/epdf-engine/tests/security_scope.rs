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
    let response = host.call_filled(
        &json!({ "kind": "metadata.update", "jobId": 9, "docId": doc, "patch": { "title": "x" } })
            .to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("Forbidden"), "{envelope}");
}

/// A kind that takes an `authority` is checked against the one the request
/// carries. A client filling it from a narrow session sends that session's
/// scope (`SessionAuthority::to_wire`), and the write is refused as the
/// session would refuse it; the same request from a full session is not.
#[test]
fn a_filled_authority_carries_a_narrow_sessions_scope() {
    const FORMS: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";
    let set_value = |doc: &str| {
        json!({ "kind": "forms.setValue", "docId": doc,
            "ref": { "kind": "objectNumber", "objectNumber": 4 },
            "value": { "value": "x" } })
    };
    let mut host = Host::new();
    let narrow =
        sig_open_fixture_with(&mut host, FORMS, json!({ "scope": ["doc.open", "doc.forms.read"] }));
    let mut request = set_value(&narrow);
    host.fill_envelope(&mut request);
    assert_eq!(
        request["authority"]["grants"]["scope"],
        json!(["doc.open", "doc.forms.read"]),
        "{request}"
    );
    let refused: serde_json::Value =
        serde_json::from_str(&host.call(&request.to_string(), Vec::new()).json).unwrap();
    assert_eq!(refused["error"]["code"], json!("Forbidden"), "{refused}");

    let mut host = Host::new();
    let full = sig_open_fixture_with(&mut host, FORMS, json!({ "scope": ["*"] }));
    let response = host.call_filled(&set_value(&full).to_string(), Vec::new());
    let written: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert!(written.get("error").is_none(), "{written}");
}

#[test]
fn an_invalid_scope_fails_the_open() {
    let mut host = Host::new();
    let bytes = std::fs::read(
        test_support::workspace_root().join("pdfium-multiplatform-prototype/fixtures/report.pdf"),
    )
    .unwrap();
    let response = host.call_filled(
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
        let response = host.call_filled(
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
    let response = host.call_filled(
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
    let response = host.call_filled(
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

// --- Field scopes (upstream `test/auth/scope/fields.test.ts`, 4b2cdb395).

mod fields {
    use epdf_engine::auth::scope::{
        CollabEntity, DocCapability, FieldAction, FieldActionPattern, FieldFilter, FieldGroupRight,
        FieldWriteAction, IdentityClaims, ParsedScope, check_any_field_action, check_capability,
        check_field_action, check_set_group, decode_pdf_bits, expand_raw_scope, field_rights_of,
        parse_all, parse_scope,
    };
    use epdf_engine::session::authority::SessionAuthority;

    use FieldWriteAction::{Fill, Sign};

    fn scope(entries: &[&str]) -> Vec<ParsedScope> {
        parse_all(&entries.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }

    fn none() -> epdf_engine::auth::scope::PdfBits {
        decode_pdf_bits(Some(0))
    }

    #[test]
    fn parse_with_fill_sign_set_group_and_wildcard_by_all_or_group() {
        for (raw, action) in [
            ("fill", FieldActionPattern::One(FieldAction::Fill)),
            ("sign", FieldActionPattern::One(FieldAction::Sign)),
            ("set-group", FieldActionPattern::One(FieldAction::SetGroup)),
            ("*", FieldActionPattern::Any),
        ] {
            assert_eq!(
                parse_scope(&format!("fields:{raw}:all")).unwrap(),
                ParsedScope::FieldCollab { action, filter: FieldFilter::All },
            );
            assert_eq!(
                parse_scope(&format!("fields:{raw}:group=buyer")).unwrap(),
                ParsedScope::FieldCollab { action, filter: FieldFilter::Group("buyer".into()) },
            );
        }
    }

    #[test]
    fn refuse_filters_about_who_made_a_field_and_actions_fields_have_not() {
        for raw in [
            "fields:fill:self",
            "fields:sign:createdBy=alice",
            "fields:create:all",
            "fields:update:group=buyer",
            "fields:fill:group=",
        ] {
            assert!(parse_scope(raw).is_err(), "{raw} parsed");
        }
    }

    #[test]
    fn let_the_holder_see_the_form_groups_limit_writes_never_reads() {
        let granted = expand_raw_scope(&["fields:fill:group=buyer".into()], none()).unwrap();
        assert!(granted.contains(&DocCapability::DocFormsRead));
        assert!(!granted.contains(&DocCapability::DocFormsFill));
    }

    #[test]
    fn a_group_scope_reaches_its_group_only() {
        let buyer = scope(&["doc.open", "fields:fill:group=buyer", "fields:sign:group=buyer"]);
        assert!(check_field_action(Fill, Some("buyer"), &buyer, none()));
        assert!(check_field_action(Sign, Some("buyer"), &buyer, none()));
        assert!(!check_field_action(Fill, Some("seller"), &buyer, none()));
        // A field in no group is filled only by an unrestricted filler.
        assert!(!check_field_action(Fill, None, &buyer, none()));
    }

    #[test]
    fn narrows_per_action_a_fill_scope_says_nothing_about_signing() {
        let parsed = scope(&["doc.forms.fill", "doc.sign", "fields:fill:group=buyer"]);
        assert!(!check_field_action(Fill, Some("seller"), &parsed, none()));
        assert!(check_field_action(Sign, Some("seller"), &parsed, none()));
    }

    #[test]
    fn without_a_fields_scope_the_broad_capability_decides_for_every_field() {
        let fill = scope(&["doc.forms.fill"]);
        assert!(check_field_action(Fill, Some("seller"), &fill, none()));
        assert!(check_field_action(Fill, None, &fill, none()));
        assert!(!check_field_action(Sign, Some("seller"), &fill, none()));
        assert!(check_field_action(Fill, Some("seller"), &scope(&["*"]), none()));
    }

    #[test]
    fn pdf_permissions_stays_what_the_file_allows_design_is_never_narrowed() {
        let bits = decode_pdf_bits(Some((1 << 3) | (1 << 5)));
        let parsed = scope(&["pdf.permissions", "fields:*:group=buyer"]);
        assert!(check_field_action(Fill, Some("buyer"), &parsed, bits));
        assert!(!check_field_action(Fill, Some("seller"), &parsed, bits));
        assert!(check_capability(DocCapability::DocFormsModify, &parsed, bits));
    }

    #[test]
    fn some_field_a_fields_scope_or_the_broad_capability() {
        let buyer = scope(&["doc.open", "fields:fill:group=buyer", "fields:sign:group=buyer"]);
        assert!(check_any_field_action(Fill, &buyer, none()));
        assert!(check_any_field_action(Fill, &scope(&["doc.forms.fill"]), none()));
        assert!(!check_any_field_action(Fill, &scope(&["doc.open"]), none()));
        assert!(!check_any_field_action(Sign, &scope(&["fields:fill:group=buyer"]), none()));
    }

    #[test]
    fn your_own_default_group_always_another_with_fields_set_group_for_it() {
        use CollabEntity::{Annotations, Fields};
        assert!(check_set_group(Fields, "legal", Some("legal"), &[]));
        let buyer = scope(&["fields:set-group:group=buyer"]);
        assert!(check_set_group(Fields, "buyer", Some("legal"), &buyer));
        assert!(!check_set_group(Fields, "seller", Some("legal"), &buyer));
        // Each entity's set-group is its own.
        assert!(!check_set_group(
            Fields,
            "buyer",
            Some("legal"),
            &scope(&["annotations:set-group:all"])
        ));
        assert!(!check_set_group(
            Annotations,
            "buyer",
            Some("legal"),
            &scope(&["fields:set-group:all"])
        ));
    }

    #[test]
    fn field_rights_list_the_groups_with_the_callers_own_for_set_group() {
        let parsed = scope(&["fields:fill:group=buyer", "fields:set-group:group=seller"]);
        let rights = field_rights_of(&parsed, none(), Some("legal"));
        assert_eq!(rights.fill, FieldGroupRight::Groups(vec!["buyer".into()]));
        assert_eq!(rights.sign, FieldGroupRight::none());
        assert_eq!(
            rights.set_group,
            FieldGroupRight::Groups(vec!["seller".into(), "legal".into()])
        );
        assert!(rights.fill.covers(Some("buyer")));
        assert!(!rights.fill.covers(None));
        assert_eq!(field_rights_of(&scope(&["*"]), none(), None).sign, FieldGroupRight::All);
    }

    #[test]
    fn the_authority_refuses_another_group_without_the_scope() {
        let identity = IdentityClaims { group_id: Some("legal".into()), ..Default::default() };
        let authority =
            SessionAuthority::new(&["doc.forms.modify".into()], identity, none()).unwrap();
        assert!(authority.allows_field_group("legal"));
        assert!(!authority.allows_field_group("buyer"));

        let buyer = SessionAuthority::new(
            &[
                "doc.open".into(),
                "fields:fill:group=buyer".into(),
                "fields:sign:group=buyer".into(),
            ],
            IdentityClaims::default(),
            none(),
        )
        .unwrap();
        assert!(buyer.allows_field_write(Fill, Some("buyer")));
        assert!(!buyer.allows_field_write(Fill, Some("seller")));
        assert!(buyer.allows_some_field_write(Sign));
    }
}
