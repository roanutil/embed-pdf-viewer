use epdf_annotation_core as core;
use epdf_annotation_core::js::Nullable;
use epdf_ffi::{
    AnnotationBorder, AnnotationPropKey, AnnotationPropsPatch, AnnotationRef, PageRef,
    PdfDestination, PdfLinkTarget,
};

#[test]
fn an_object_number_ref_round_trips() {
    let core = core::AnnotationRef::ObjectNumber {
        page: core::PageRef::new(4),
        annot_object_number: 17.0,
    };
    let ffi = AnnotationRef::try_from(core.clone()).unwrap();
    assert_eq!(
        ffi,
        AnnotationRef::ObjectNumber {
            page: PageRef { page_object_number: 4 },
            annot_object_number: 17
        }
    );
    assert_eq!(core::AnnotationRef::from(ffi), core);
}

#[test]
fn a_fractional_object_number_is_refused() {
    let core =
        core::AnnotationRef::ObjectNumber { page: core::PageRef::new(4), annot_object_number: 1.5 };
    assert!(AnnotationRef::try_from(core).is_err());
}

#[test]
fn a_props_patch_clears_through_its_clear_list() {
    let patch = AnnotationPropsPatch {
        color: Some("#ff0000".into()),
        clear: vec![AnnotationPropKey::InteriorColor, AnnotationPropKey::Link],
        ..Default::default()
    };
    let core = core::AnnotationPropsPatch::try_from(patch).unwrap();
    assert_eq!(core.color.as_deref(), Some("#ff0000"));
    assert_eq!(core.interior_color, Nullable::Null);
    assert_eq!(core.link, Nullable::Null);
    assert_eq!(core.keys(), ["color", "interiorColor", "link"]);
}

#[test]
fn a_patch_that_sets_and_clears_one_key_is_refused() {
    let patch = AnnotationPropsPatch {
        interior_color: Some("#000000".into()),
        clear: vec![AnnotationPropKey::InteriorColor],
        ..Default::default()
    };
    let error = core::AnnotationPropsPatch::try_from(patch).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn clearing_a_key_that_has_no_null_state_is_refused() {
    let patch =
        AnnotationPropsPatch { clear: vec![AnnotationPropKey::Color], ..Default::default() };
    let error = core::AnnotationPropsPatch::try_from(patch).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn a_dashed_border_round_trips() {
    let core = core::Border::Dashed { dash: vec![3.0, 2.0] };
    let ffi = AnnotationBorder::from(core.clone());
    assert_eq!(ffi, AnnotationBorder::Dashed { dash: vec![3.0, 2.0] });
    assert_eq!(core::Border::from(ffi), core);
}

#[test]
fn a_goto_link_keeps_its_page_and_drops_undefined_coordinates() {
    let core = core::PdfLinkTarget::Goto {
        destination: core::PdfDestination::Xyz {
            page: core::PageRef::new(9),
            left: Nullable::Value(72.0),
            top: Nullable::Undefined,
            zoom: Nullable::Null,
        },
    };
    let ffi = PdfLinkTarget::from(core);
    assert_eq!(
        ffi,
        PdfLinkTarget::Goto {
            destination: PdfDestination::Xyz {
                page: PageRef { page_object_number: 9 },
                left: Some(72.0),
                top: None,
                zoom: None,
            }
        }
    );
}
