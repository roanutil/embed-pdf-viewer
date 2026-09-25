//! G12's structural gate: a form field is made, changed and removed the way
//! the TypeScript engine does it.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run form-field
//!
//! # Every case opens its own copy
//!
//! A create hands out object numbers that depend on what the document
//! already holds, so a shared document would make each case's numbers
//! depend on the cases before it.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::forms::field::{
    FormFieldDraft, FormFieldPatch, OwnedPageWidgets, attach_widget, create_field, delete_field,
    detach_widget, update_field,
};
use epdf_engine::features::forms::snapshot::FormFieldRef;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(default)]
    draft: Option<serde_json::Value>,
    #[serde(default)]
    patch_of: Option<serde_json::Value>,
    #[serde(default)]
    patch: Option<serde_json::Value>,
    #[serde(default)]
    delete_of: Option<serde_json::Value>,
    #[serde(default)]
    attach_to: Option<serde_json::Value>,
    #[serde(default)]
    detach_from: Option<serde_json::Value>,
    #[serde(default)]
    widget_object_number: Option<u32>,
    #[serde(default)]
    on_state: Option<String>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("forms").join("field.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("field.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "forms.field");
    file
}

fn reference(value: &serde_json::Value, name: &str) -> FormFieldRef {
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("{name}: the ref does not decode: {e}"))
}

/// Run one case and report what it produced, as JSON in the vector's shape.
fn run(
    file: &VectorFile,
    case: &Case,
) -> Result<serde_json::Value, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let cancel = CancelToken::never();

    if let Some(draft) = &case.draft {
        let draft: FormFieldDraft = serde_json::from_value(draft.clone())
            .unwrap_or_else(|e| panic!("{}: the draft does not decode: {e}", case.name));
        let widgets = OwnedPageWidgets::new(owned);
        let field = create_field(owned.document(), &widgets, &draft, &cancel)?;
        return Ok(serde_json::json!({ "field": field }));
    }
    if let Some(target) = &case.patch_of {
        let patch: FormFieldPatch = serde_json::from_value(case.patch.clone().expect("a patch"))
            .unwrap_or_else(|e| panic!("{}: the patch does not decode: {e}", case.name));
        let field =
            update_field(owned.document(), &reference(target, &case.name), &patch, &cancel)?;
        return Ok(serde_json::json!({ "field": field }));
    }
    if let Some(target) = &case.delete_of {
        let result = delete_field(owned.document(), &reference(target, &case.name), &cancel)?;
        return Ok(serde_json::json!({
            "deletedFieldObjectNumber": result.deleted_field_object_number,
            "detachedWidgets": result.detached_widgets,
        }));
    }
    if let Some(target) = &case.detach_from {
        let field = detach_widget(
            owned.document(),
            &reference(target, &case.name),
            case.widget_object_number.expect("a widget"),
            &cancel,
        )?;
        return Ok(serde_json::json!({ "field": field }));
    }
    let target = case.attach_to.as_ref().expect("an operation");
    let field = attach_widget(
        owned.document(),
        &reference(target, &case.name),
        case.widget_object_number.expect("a widget"),
        case.on_state.as_deref(),
        &cancel,
    )?;
    Ok(serde_json::json!({ "field": field }))
}

#[test]
fn a_structural_write_leaves_what_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 15, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let mine = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            test_support::canonical_numbers(&mine),
            test_support::canonical_numbers(case.result.as_ref().expect("a recorded result")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert!(applied >= 12, "the applied list shrank");
}

#[test]
fn a_refused_structural_write_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        // The merged field/widget case is the TypeScript's CASCADE
        // refusing, not its field writer: the field goes and the worker
        // host then asks the annotation plane to delete the same object,
        // which will not delete a widget. This port has no cascade, so the
        // field-plane half succeeds and there is nothing to compare.
        if case.name == "delete-merged-field-refused" {
            continue;
        }
        refusals += 1;
        let error = run(&file, case).expect_err(&case.name);
        let expected = match case.refused_with.as_deref() {
            Some("NotFound") => EngineErrorCode::NotFound,
            Some("InvalidArg") => EngineErrorCode::InvalidArg,
            other => panic!("{}: unhandled refusal code {other:?}", case.name),
        };
        assert!(error.is(expected), "{}: {} (got {error})", case.name, case.covers);
    }

    assert!(refusals >= 5, "the refusal list shrank");
}

#[test]
fn a_draft_refusal_says_what_was_wrong_with_the_draft() {
    // PDFium refuses each of these too, with the same code, and by then it
    // has usually written something. The message is this engine's own and
    // says which of the three it was.
    let file = load();
    for (name, expected) in [
        ("create-unnamed-refused", "needs a name"),
        ("create-text-with-options-refused", "has no options"),
        ("attach-off-state-refused", "\"Off\""),
    ] {
        let case = file.cases.iter().find(|c| c.name == name).expect(name);
        let message = run(&file, case).expect_err(name).to_string();
        assert!(message.contains(expected), "{name}: expected {expected:?} in {message:?}");
    }
}

#[test]
fn a_widget_lands_where_the_placement_put_it() {
    // The widget DTO is a REFERENCE: an annotation number and a page
    // number, and no geometry. So a widget placed in the wrong box reads
    // back identically, and the only way to see it is to ask the
    // annotation.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "create-text-full").expect("the case");
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    let draft: FormFieldDraft =
        serde_json::from_value(case.draft.clone().expect("a draft")).expect("the draft decodes");
    let placement = draft.placements().next().expect("one placement").clone();
    let widgets = OwnedPageWidgets::new(owned);
    create_field(owned.document(), &widgets, &draft, &CancelToken::never())
        .expect("the field is created");

    let page = owned.document().page(0).expect("page");
    let last = page.annotation_count() - 1;
    let written = page.annotation(last).expect("the new widget");
    let rect = written.rect();
    for (got, want) in [
        (f64::from(rect.left), placement.rect.left),
        (f64::from(rect.bottom), placement.rect.bottom),
        (f64::from(rect.right), placement.rect.right),
        (f64::from(rect.top), placement.rect.top),
    ] {
        assert!((got - want).abs() < 0.01, "the widget sits at {rect:?}, not {:?}", placement.rect);
    }
}

#[test]
fn a_radio_without_a_state_is_refused_before_the_field_exists() {
    // PDFium refuses the ADOPTION of a stateless radio widget too, and with
    // the same code. The difference is when: by then the field and the
    // widget both exist, and nothing in this layer takes them back. So the
    // check is in preflight, and this is what says so.
    let file = load();
    let case =
        file.cases.iter().find(|c| c.name == "create-radio-without-state-refused").expect("case");
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let before = owned.document().page(0).expect("page").annotation_count();
    let fields_before = owned.document().form_model().expect("a model").field_count();

    let draft: FormFieldDraft =
        serde_json::from_value(case.draft.clone().expect("a draft")).expect("the draft decodes");
    let widgets = OwnedPageWidgets::new(owned);
    create_field(owned.document(), &widgets, &draft, &CancelToken::never())
        .expect_err("a radio widget needs a state");

    assert_eq!(
        owned.document().page(0).expect("page").annotation_count(),
        before,
        "the refusal left a widget behind",
    );
    assert_eq!(
        owned.document().form_model().expect("a model").field_count(),
        fields_before,
        "the refusal left a field behind",
    );
}

#[test]
fn deleting_a_field_leaves_its_widgets_on_the_page() {
    // The half the vector cannot show, because the DTO it records is the
    // field's and the field is gone. A widget survives its field as an
    // inert annotation, which is what makes the detached list worth
    // reporting: something still has to remove them from the page.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let page = owned.document().page(0).expect("page");
    let before = page.annotation_count();

    let result = delete_field(
        owned.document(),
        &FormFieldRef::ObjectNumber { field: 9 },
        &CancelToken::never(),
    )
    .expect("the radio group deletes");

    assert_eq!(result.detached_widgets.len(), 2, "both widgets are reported");
    assert_eq!(
        page.annotation_count(),
        before,
        "a detached widget is still an annotation on its page",
    );
}
