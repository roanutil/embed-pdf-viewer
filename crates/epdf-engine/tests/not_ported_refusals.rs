//! Writes not yet ported to the runtime 1461b0c51 API are refused BEFORE
//! anything is written.
//!
//! re-port (Task 8): delete these once `/RD` and `/DV` writes are ported. Each
//! asserts NotImplemented and that nothing was written: the document saves
//! to the same bytes it did before the request, or, where an earlier write
//! in the test makes saves differ anyway, the page reads back the same.

use epdf_engine::cancel::CancelToken;
use epdf_engine::clock::{FixedClock, Timestamp};
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
use epdf_engine::features::annotation::read::page::read_page_annotations;
use epdf_engine::features::annotation::write::context::AnnotationWriteContext;
use epdf_engine::features::annotation::write::create::create_annotation;
use epdf_engine::features::annotation::write::draft::{AnnotationActor, AnnotationDraft};
use epdf_engine::features::annotation::write::patch::AnnotationPatch;
use epdf_engine::features::annotation::write::update::update_annotation;
use epdf_engine::features::forms::field::{
    FormFieldDraft, FormFieldPatch, OwnedPageWidgets, create_field, update_field,
};
use epdf_engine::features::forms::snapshot::FormFieldRef;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::ids::SeededIdSource;
use epdf_engine::revision::RevisionToken;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;

const FORM_FIXTURE: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";

fn open(bytes: Vec<u8>) -> DocumentSession {
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn saved(document: &pdfium::Document) -> Vec<u8> {
    document.save_to_buffer(pdfium::PdfSaveMode::Incremental).expect("save")
}

fn field_count(document: &pdfium::Document) -> u32 {
    document.form_model().expect("a form model").field_count()
}

fn square_draft(extra: serde_json::Value) -> AnnotationDraft {
    let mut draft = serde_json::json!({
        "subtype": "square",
        "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
    });
    draft.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    serde_json::from_value(draft).expect("the draft decodes")
}

fn rect_differences() -> serde_json::Value {
    serde_json::json!({ "left": 2.0, "top": 2.0, "right": 2.0, "bottom": 2.0 })
}

#[test]
fn a_create_with_rect_differences_is_refused_before_the_annotation_exists() {
    let session = open(test_support::fixture(test_support::REPORT));
    let owned = session.document().expect("open");
    let document = owned.document();
    let page_object_number =
        PageObjectNumber::new(document.page_object_number_by_index(0)).expect("indirect page");
    let state = read_page_annotations(
        document,
        0,
        page_object_number,
        &RevisionToken::new("<session>", page_object_number, 0),
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists")
    .page_state;
    let page = document.page(0).expect("page");
    let annotations_before = page.annotation_count();
    let bytes_before = saved(document);

    let error = create_annotation(
        document,
        &page,
        page_object_number,
        &state,
        &state,
        &square_draft(serde_json::json!({ "rectDifferences": rect_differences() })),
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty(),
        &FixedClock(Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect_err("refused");

    assert_eq!(error.code, EngineErrorCode::NotImplemented, "{error}");
    assert_eq!(page.annotation_count(), annotations_before);
    assert_eq!(saved(document), bytes_before, "the refusal wrote to the document");
}

#[test]
fn an_update_with_rect_differences_is_refused_before_the_identity_stamp() {
    let session = open(test_support::fixture(test_support::REPORT));
    let owned = session.document().expect("open");
    let document = owned.document();
    let page_object_number =
        PageObjectNumber::new(document.page_object_number_by_index(0)).expect("indirect page");
    let state = read_page_annotations(
        document,
        0,
        page_object_number,
        &RevisionToken::new("<session>", page_object_number, 0),
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists")
    .page_state;
    let page = document.page(0).expect("page");
    let created = create_annotation(
        document,
        &page,
        page_object_number,
        &state,
        &state,
        &square_draft(serde_json::json!({})),
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty(),
        &FixedClock(Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect("a square without /RD still creates");
    // Bytes are no use here: two incremental saves of a document with an
    // unsaved create already differ from each other. The page's annotations
    // read back as DTOs carry `/NM` and every field a patch could touch.
    let listing = || {
        let annotations = read_page_annotations(
            document,
            0,
            page_object_number,
            &RevisionToken::new("<session>", page_object_number, 0),
            &mut ActionReadBudgetTracker::default(),
            &CancelToken::never(),
        )
        .expect("the page lists");
        serde_json::to_value(&annotations).expect("serialize")
    };
    let annotations_before = page.annotation_count();
    let listing_before = listing();

    // The opacity is written before `/RD` in apply_shape_patch, so a refusal
    // made there rather than in preflight would leave it changed.
    let patch: AnnotationPatch = serde_json::from_value(serde_json::json!({
        "subtype": "square",
        "opacity": 0.5,
        "rectDifferences": rect_differences(),
    }))
    .expect("the patch decodes");
    let error = update_annotation(
        document,
        &page,
        0,
        page_object_number,
        &created.created.base.annotation_ref,
        &patch,
        &AnnotationWriteContext::empty(),
        &state,
        &state,
        &FixedClock(Timestamp::utc(1_700_000_100_000)),
        &SeededIdSource::new(7),
        &CancelToken::never(),
    )
    .expect_err("refused");

    assert_eq!(error.code, EngineErrorCode::NotImplemented, "{error}");
    assert_eq!(page.annotation_count(), annotations_before);
    assert_eq!(listing(), listing_before, "the refusal wrote to the annotation");
}

#[test]
fn a_field_create_with_a_default_value_is_refused_before_the_field_exists() {
    let bytes = std::fs::read(test_support::workspace_root().join(FORM_FIXTURE)).expect("read");
    let session = open(bytes);
    let owned = session.document().expect("open");
    let document = owned.document();
    let fields_before = field_count(document);
    let bytes_before = saved(document);

    let draft: FormFieldDraft = serde_json::from_value(serde_json::json!({
        "family": "text",
        "name": "new.with_default",
        "defaultValue": "nothing yet",
    }))
    .expect("the draft decodes");
    let widgets = OwnedPageWidgets::new(owned);
    let error =
        create_field(document, &widgets, &draft, &CancelToken::never()).expect_err("refused");

    assert_eq!(error.code, EngineErrorCode::NotImplemented, "{error}");
    assert_eq!(field_count(document), fields_before);
    assert_eq!(saved(document), bytes_before, "the refusal wrote to the document");
}

#[test]
fn a_field_update_touching_the_default_value_is_refused_before_the_rename() {
    let bytes = std::fs::read(test_support::workspace_root().join(FORM_FIXTURE)).expect("read");
    let session = open(bytes);
    let owned = session.document().expect("open");
    let document = owned.document();
    let reference: FormFieldRef = serde_json::from_value(serde_json::json!({
        "kind": "objectNumber",
        "fieldObjectNumber": 4,
    }))
    .expect("the ref decodes");

    for default_value in [serde_json::json!("again"), serde_json::Value::Null] {
        let fields_before = field_count(document);
        let bytes_before = saved(document);
        // The rename comes first in update_field, so a refusal after it
        // would leave the field renamed.
        let patch: FormFieldPatch = serde_json::from_value(serde_json::json!({
            "family": "text",
            "name": "renamed_by_a_refused_patch",
            "defaultValue": default_value,
        }))
        .expect("the patch decodes");
        let error =
            update_field(document, &reference, &patch, &CancelToken::never()).expect_err("refused");

        assert_eq!(error.code, EngineErrorCode::NotImplemented, "{default_value}: {error}");
        assert_eq!(field_count(document), fields_before);
        assert_eq!(saved(document), bytes_before, "{default_value}: the refusal wrote");
    }
}
