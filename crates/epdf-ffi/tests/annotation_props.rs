//! Annotation refs and link targets against the engine's records. The old
//! `AnnotationProps` projection is gone with the TypeScript type it mirrored
//! (current fields are per kind: `packages/engine/core/src/annotation/kinds/shared-fields.ts`).

use epdf_ffi::{AnnotationRef, PageRef, PdfDestination, PdfLinkTarget};
use serde_json::json;

#[test]
fn an_object_number_ref_round_trips() {
    let engine = json!({
        "kind": "objectNumber",
        "page": { "kind": "objectNumber", "objectNumber": 4 },
        "objectNumber": 17,
    });
    let ffi = AnnotationRef::from_json(&engine).unwrap();
    assert_eq!(
        ffi,
        AnnotationRef::ObjectNumber {
            page: PageRef { page_object_number: 4 },
            annot_object_number: 17
        }
    );
    assert_eq!(ffi.to_json(), engine);
}

#[test]
fn a_base_index_ref_round_trips() {
    let engine = json!({
        "kind": "baseIndex",
        "page": { "kind": "objectNumber", "objectNumber": 4 },
        "baseIndex": 2,
    });
    let ffi = AnnotationRef::from_json(&engine).unwrap();
    assert_eq!(ffi, AnnotationRef::Index { page: PageRef { page_object_number: 4 }, index: 2 });
    assert_eq!(ffi.page(), PageRef { page_object_number: 4 });
    assert_eq!(ffi.to_json(), engine);
}

#[test]
fn a_fractional_object_number_is_refused() {
    let engine = json!({
        "kind": "objectNumber",
        "page": { "kind": "objectNumber", "objectNumber": 4 },
        "objectNumber": 1.5,
    });
    assert!(AnnotationRef::from_json(&engine).is_err());
    // A ref kind the engine no longer writes (`nm`) is refused too.
    let nm =
        json!({ "kind": "nm", "page": { "kind": "objectNumber", "objectNumber": 4 }, "nm": "a" });
    assert!(AnnotationRef::from_json(&nm).is_err());
}

#[test]
fn a_goto_link_keeps_its_page_and_drops_absent_and_null_coordinates() {
    let engine = json!({
        "kind": "goto",
        "destination": {
            "kind": "xyz",
            "page": { "kind": "objectNumber", "objectNumber": 9 },
            "left": 72,
            "zoom": null,
        },
    });
    let ffi = PdfLinkTarget::from_json(&engine).unwrap();
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
