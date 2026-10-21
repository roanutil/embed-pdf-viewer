//! Writes not yet ported to the runtime 1461b0c51 API are refused BEFORE
//! anything is written.
//!
//! re-port (Task 8): delete these once `/DV` writes are ported (8.21). Each
//! asserts NotImplemented and that nothing was written: the document saves
//! to the same bytes it did before the request. A create and an update
//! write `/RD` the way upstream does since 8.14 and 8.15: their writes have
//! no `rectDifferences`, and the appearance writes `/RD`.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::forms::field::{
    FormFieldDraft, FormFieldPatch, OwnedPageWidgets, create_field, update_field,
};
use epdf_engine::features::forms::snapshot::FormFieldRef;
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
        "objectNumber": 4,
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
