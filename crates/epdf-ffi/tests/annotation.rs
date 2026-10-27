//! The annotation records against the engine's JSON
//! (`packages/engine/core/src/identity/AnnotationRef.ts`,
//! `dto/PdfDestination.ts`, `dto/PdfLinkTarget.ts`).

use epdf_ffi::{
    AnnotationRef, CreationDraftAnchor, PageDestination, PageLinkTarget, PageRef, PdfDestination,
    PdfLinkTarget,
};
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

#[test]
fn an_unknown_target_kind_is_invalid_state() {
    let error = PdfLinkTarget::from_json(&json!({ "kind": "submit-form" })).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let error = PageLinkTarget::from_json(&json!({ "kind": "submit-form" })).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

// A link's target is in page space (`packages/plugin/link/src/contract.ts:16-22`).

#[test]
fn a_page_space_xyz_target_keeps_its_x_and_y() {
    let json = json!({
        "kind": "goto",
        "destination": {
            "kind": "xyz", "page": { "kind": "objectNumber", "objectNumber": 7 }, "x": 0.0, "y": 10.0
        }
    });
    let target = PageLinkTarget::from_json(&json).unwrap();
    assert_eq!(
        target,
        PageLinkTarget::Goto {
            destination: PageDestination::Xyz {
                page: PAGE,
                x: Some(0.0),
                y: Some(10.0),
                zoom: None
            }
        }
    );
    assert_eq!(target.to_json(), json);
}

#[test]
fn a_page_space_fit_r_target_keeps_its_rectangle() {
    let json = json!({
        "kind": "goto",
        "destination": {
            "kind": "fitR", "page": { "kind": "objectNumber", "objectNumber": 7 },
            "x": 1.0, "y": 2.0, "width": 30.0, "height": 40.0
        }
    });
    let target = PageLinkTarget::from_json(&json).unwrap();
    assert_eq!(
        target,
        PageLinkTarget::Goto {
            destination: PageDestination::FitR {
                page: PAGE,
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 40.0
            }
        }
    );
    assert_eq!(target.to_json(), json);
    // A fitR without its size is refused, not read as a smaller rectangle.
    let short = json!({
        "kind": "fitR", "page": { "kind": "objectNumber", "objectNumber": 7 }, "x": 1, "y": 2
    });
    assert_eq!(PageDestination::from_json(&short).unwrap_err().code(), Some("invalid-state"));
}

#[test]
fn every_page_destination_kind_round_trips() {
    for destination in [
        PageDestination::Xyz { page: PAGE, x: Some(1.0), y: None, zoom: Some(2.0) },
        PageDestination::Fit { page: PAGE },
        PageDestination::FitH { page: PAGE, y: Some(3.0) },
        PageDestination::FitV { page: PAGE, x: None },
        PageDestination::FitR { page: PAGE, x: 1.0, y: 2.0, width: 3.0, height: 4.0 },
        PageDestination::FitB { page: PAGE },
        PageDestination::FitBH { page: PAGE, y: None },
        PageDestination::FitBV { page: PAGE, x: Some(5.0) },
    ] {
        let json = destination.to_json();
        assert_eq!(PageDestination::from_json(&json).unwrap(), destination);
    }
}

/// A creation draft's anchor as the core reads it, with `count` points.
fn draft_anchor(count: serde_json::Value) -> epdf_annotation_core::types::CreationDraftAnchor {
    let object = json!({
        "kind": "creation", "subtype": "polygon",
        "page": { "kind": "objectNumber", "objectNumber": 7 },
        "bounds": { "x": 0, "y": 0, "width": 10, "height": 10 },
        "pointCount": count, "minPoints": 3, "canFinish": false,
    });
    epdf_annotation_core::types::CreationDraftAnchor::from_object(object.as_object().unwrap())
        .unwrap()
}

#[test]
fn a_draft_point_count_that_is_not_a_whole_u32_is_invalid_state() {
    // As a page number is: refused, not clamped to 0 or `u32::MAX`.
    for count in [json!(-1), json!(1.5), json!(4_294_967_296_u64), json!(1e300)] {
        let error = CreationDraftAnchor::try_from(draft_anchor(count.clone())).unwrap_err();
        assert_eq!(error.code(), Some("invalid-state"), "{count}");
    }
    let anchor = CreationDraftAnchor::try_from(draft_anchor(json!(2))).unwrap();
    assert_eq!((anchor.point_count, anchor.min_points, anchor.closed), (2, 3, true));
}
