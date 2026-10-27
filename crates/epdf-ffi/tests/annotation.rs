//! The annotation records against the engine's JSON
//! (`packages/engine/core/src/identity/AnnotationRef.ts`,
//! `dto/PdfDestination.ts`, `dto/PdfLinkTarget.ts`).

use epdf_ffi::{AnnotationRef, PageRef, PdfDestination, PdfLinkTarget};
use serde_json::json;

const PAGE: PageRef = PageRef { page_object_number: 7 };

#[test]
fn every_destination_kind_round_trips() {
    for destination in [
        PdfDestination::Xyz { page: PAGE, left: Some(1.0), top: None, zoom: Some(2.0) },
        PdfDestination::Fit { page: PAGE },
        PdfDestination::FitH { page: PAGE, top: Some(3.0) },
        PdfDestination::FitV { page: PAGE, left: None },
        PdfDestination::FitR { page: PAGE, left: 1.0, bottom: 2.0, right: 3.0, top: 4.0 },
        PdfDestination::FitB { page: PAGE },
        PdfDestination::FitBH { page: PAGE, top: None },
        PdfDestination::FitBV { page: PAGE, left: Some(5.0) },
    ] {
        let json = destination.to_json();
        assert_eq!(PdfDestination::from_json(&json).unwrap(), destination);
    }
}

#[test]
fn an_absent_coordinate_goes_back_as_absent() {
    let json = PdfDestination::Xyz { page: PAGE, left: None, top: None, zoom: None }.to_json();
    assert_eq!(
        json,
        json!({ "kind": "xyz", "page": { "kind": "objectNumber", "objectNumber": 7 } })
    );
}

#[test]
fn a_null_coordinate_arrives_as_none() {
    let destination = PdfDestination::from_json(&json!({
        "kind": "xyz", "page": { "kind": "objectNumber", "objectNumber": 7 }, "left": null, "top": 9
    }))
    .unwrap();
    assert_eq!(
        destination,
        PdfDestination::Xyz { page: PAGE, left: None, top: Some(9.0), zoom: None }
    );
}

#[test]
fn every_target_kind_round_trips() {
    for target in [
        PdfLinkTarget::Goto { destination: PdfDestination::Fit { page: PAGE } },
        PdfLinkTarget::Uri { uri: "https://example.invalid".into() },
        PdfLinkTarget::GotoRemote { file: "other.pdf".into() },
        PdfLinkTarget::Launch { path: "app".into() },
        PdfLinkTarget::Javascript,
        PdfLinkTarget::Named { name: "NextPage".into() },
        PdfLinkTarget::Unsupported,
    ] {
        let json = target.to_json();
        assert_eq!(PdfLinkTarget::from_json(&json).unwrap(), target);
    }
    // The kinds as the engine spells them.
    assert_eq!(
        PdfLinkTarget::GotoRemote { file: "f".into() }.to_json(),
        json!({ "kind": "goto-remote", "file": "f" })
    );
}

#[test]
fn an_annotation_ref_with_a_fractional_number_is_invalid_state() {
    let page = json!({ "kind": "objectNumber", "objectNumber": 7 });
    let error = AnnotationRef::from_json(
        &json!({ "kind": "objectNumber", "page": page, "objectNumber": 8.5 }),
    )
    .unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let ok = json!({ "kind": "objectNumber", "page": page, "objectNumber": 8 });
    assert_eq!(
        AnnotationRef::from_json(&ok).unwrap(),
        AnnotationRef::ObjectNumber { page: PAGE, annot_object_number: 8 }
    );
}
