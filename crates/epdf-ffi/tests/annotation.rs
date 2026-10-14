use epdf_annotation_core as ac;
use epdf_ffi::{AnnotationRef, PageRef, PdfDestination, PdfLinkTarget};

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
        let core = ac::PdfDestination::from(destination.clone());
        assert_eq!(PdfDestination::from(&core), destination);
    }
}

#[test]
fn an_absent_coordinate_goes_back_as_absent() {
    let core = ac::PdfDestination::from(PdfDestination::Xyz {
        page: PAGE,
        left: None,
        top: None,
        zoom: None,
    });
    let json = serde_json::to_value(&core).unwrap();
    assert_eq!(
        json,
        serde_json::json!({ "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 7 } })
    );
}

#[test]
fn a_null_coordinate_arrives_as_none() {
    let core: ac::PdfDestination = serde_json::from_value(serde_json::json!({
        "kind": "xyz", "page": { "kind": "objectNumber", "pageObjectNumber": 7 }, "left": null, "top": 9
    }))
    .unwrap();
    assert_eq!(
        PdfDestination::from(&core),
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
        let core = ac::PdfLinkTarget::from(target.clone());
        assert_eq!(PdfLinkTarget::from(&core), target);
    }
}

#[test]
fn an_annotation_ref_with_a_fractional_number_is_invalid_state() {
    let core =
        ac::AnnotationRef::ObjectNumber { page: ac::PageRef::new(7), annot_object_number: 8.5 };
    let error = AnnotationRef::try_from(&core).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
    let ok =
        ac::AnnotationRef::ObjectNumber { page: ac::PageRef::new(7), annot_object_number: 8.0 };
    assert_eq!(
        AnnotationRef::try_from(&ok).unwrap(),
        AnnotationRef::ObjectNumber { page: PAGE, annot_object_number: 8 }
    );
}
