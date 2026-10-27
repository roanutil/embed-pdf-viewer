//! `signatures.analyze` and the working-copy snapshot over the signing
//! fixtures.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{page_ref, sig_call, sig_list, sig_open_fixture};

const FIX: &str = "packages/engine/main/test/fixtures";

fn analyze(host: &mut Host, doc: &str, index: u32, until: &str) -> Value {
    sig_call(
        host,
        json!({ "kind": "signatures.analyze", "docId": doc,
                "input": { "since": { "signatureIndex": index }, "until": until } }),
    )["analysis"]
        .take()
}

fn add_square(host: &mut Host, doc: &str, pon: u32) {
    sig_call(
        host,
        json!({ "kind": "annotations.create", "docId": doc, "page": page_ref(pon),
                "draft": { "subtype": "square",
                           "box": { "x": 10.0, "y": 10.0, "width": 40.0, "height": 40.0 } } }),
    );
}

#[test]
fn the_second_approval_is_a_permitted_change_after_the_first() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let analysis = analyze(&mut host, &doc, 0, "persisted");
    assert_eq!(analysis["later"]["revisionCount"], json!(1), "{analysis}");
    assert_eq!(analysis["verdict"], json!("permitted"), "{analysis}");
    assert_eq!(analysis["current"]["verdict"], analysis["verdict"]);
    assert_eq!(analysis["current"]["method"], json!("net-state"));
    assert_eq!(analysis["basis"]["source"], json!("persisted"));
    assert_eq!(analysis["policyVersion"], json!(4));
    assert_eq!(analysis["steps"].as_array().unwrap().len(), 1);
}

#[test]
fn the_last_signature_is_unchanged_on_the_saved_bytes() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let analysis = analyze(&mut host, &doc, 1, "persisted");
    assert_eq!(analysis["verdict"], json!("unchanged"), "{analysis}");
    assert_eq!(analysis["later"]["revisionCount"], json!(0));
}

#[test]
fn an_unsaved_annotation_after_an_approval_is_permitted_under_the_baseline() {
    // Policy version 4: an approval permits commenting (Acrobat's reading).
    let mut host = Host::new();
    let (doc, pon) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    add_square(&mut host, &doc, pon);
    let analysis = analyze(&mut host, &doc, 1, "working-copy");
    assert_eq!(analysis["basis"]["source"], json!("working-copy"), "{analysis}");
    assert_eq!(analysis["later"]["revisionCount"], json!(1));
    assert_eq!(analysis["verdict"], json!("permitted"), "{analysis}");
}

#[test]
fn an_edit_after_a_certification_that_forbids_it_is_judged_forbidden() {
    let mut host = Host::new();
    let (doc, pon) = sig_open_fixture(&mut host, &format!("{FIX}/signed_certified.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    // The fixture certifies with P=2: form fill and signing only.
    assert_eq!(snapshot["protection"]["certification"]["permission"], json!(2), "{snapshot}");
    add_square(&mut host, &doc, pon);
    let analysis = analyze(&mut host, &doc, 0, "working-copy");
    assert_eq!(analysis["current"]["primary"]["verdict"], json!("forbidden"), "{analysis}");
    assert_eq!(analysis["current"]["primary"]["rule"], json!("annotation"), "{analysis}");
    assert_eq!(analysis["verdict"], json!("forbidden"));
    assert_eq!(analysis["current"]["method"], json!("net-state+replay"));
}

#[test]
fn the_working_copy_snapshot_adds_a_revision_for_unsaved_edits() {
    let mut host = Host::new();
    let (doc, pon) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let saved = sig_list(
        &mut host,
        json!({ "kind": "signatures.list", "docId": doc, "workingCopy": true }),
    );
    assert_eq!(saved["revisions"].as_array().unwrap().len(), 3);
    add_square(&mut host, &doc, pon);
    let working = sig_list(
        &mut host,
        json!({ "kind": "signatures.list", "docId": doc, "workingCopy": true }),
    );
    assert_eq!(working["revisions"].as_array().unwrap().len(), 4, "{working}");
    let persisted = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(persisted["revisions"].as_array().unwrap().len(), 3);
}

#[test]
fn a_field_mdp_lock_is_listed() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/signed_fieldmdp.pdf"));
    let snapshot = sig_list(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert!(!snapshot["protection"]["fieldLocks"].as_array().unwrap().is_empty(), "{snapshot}");
}

#[test]
fn an_unsigned_field_has_nothing_to_analyze_from() {
    let mut host = Host::new();
    let (doc, _) = sig_open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let response = host.call_filled(
        &json!({ "kind": "signatures.analyze", "jobId": 1, "docId": doc,
                 "input": { "since": { "signatureIndex": 0 } } })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("InvalidArg"), "{envelope}");
}

#[test]
fn an_edit_after_an_approval_is_judged_forbidden() {
    // Rotating a page rewrites a page dictionary the approval sealed, which
    // no rule permits at any level.
    let mut host = Host::new();
    let (doc, pon) = sig_open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    sig_call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "change": { "ops": [
            { "type": "pages.rotate", "pages": [page_ref(pon)], "rotation": 90 }] } }),
    );
    let analysis = analyze(&mut host, &doc, 1, "working-copy");
    assert_eq!(analysis["current"]["primary"]["verdict"], json!("forbidden"), "{analysis}");
    assert_eq!(analysis["verdict"], json!("forbidden"));
    assert_eq!(analysis["current"]["primary"]["objectNumber"], json!(pon));
}
