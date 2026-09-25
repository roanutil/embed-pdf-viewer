//! G12's first gate: a form field says what the TypeScript engine made it say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run form-write
//!
//! # Two halves, and the second is the one a field comparison misses
//!
//! A value write reports the WIDGETS whose appearance it re-baked, and that
//! set is not derivable from the value. A radio switch changes two, the one
//! turning on and the one turning off. Writing an already-checked checkbox
//! changes none. So the list is compared alongside the field, and a writer
//! that reported the wrong set would otherwise pass.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::forms::snapshot::FormFieldRef;
use epdf_engine::features::forms::write::{FormFieldValue, reset, set_value};
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
    document_cases: Vec<DocumentCase>,
}

/// An import or a repair: the operations that touch the whole form rather
/// than one field.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentCase {
    name: String,
    covers: String,
    /// The bytes the TypeScript imported. Recorded rather than re-derived,
    /// so both engines import the SAME payload: importing each engine's own
    /// export would compare two writers instead of two readers.
    #[serde(default)]
    payload: Vec<u8>,
    /// The format the caller declared, or absent to let the sniffer decide.
    #[serde(default)]
    import_of: Option<String>,
    #[serde(default)]
    declare_format: bool,
    #[serde(default)]
    repair: Option<RepairOptions>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    counts: Option<serde_json::Value>,
    #[serde(default)]
    snapshot: Option<serde_json::Value>,
    #[serde(default)]
    report: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RepairOptions {
    bake_appearances: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(rename = "ref")]
    reference: serde_json::Value,
    #[serde(default)]
    value: Option<serde_json::Value>,
    #[serde(default)]
    refused: bool,
    /// The error CODE the TypeScript reported. The message is deliberately
    /// not recorded: the two engines word their refusals differently, and
    /// comparing prose would pin the wording rather than the behaviour.
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    field: Option<serde_json::Value>,
    #[serde(default)]
    changed_widgets: Vec<serde_json::Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("forms").join("write.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("write.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "forms.write");
    file
}

/// Run one case against a fresh copy of the fixture.
///
/// Fresh per case, deliberately: several cases write the same field, and a
/// shared document would make each one depend on the order of the ones
/// before it.
fn run(
    file: &VectorFile,
    case: &Case,
) -> Result<epdf_engine::features::forms::write::FormSetValueResult, epdf_engine::error::EngineError>
{
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    let reference: FormFieldRef = serde_json::from_value(case.reference.clone())
        .unwrap_or_else(|e| panic!("{}: the ref does not decode: {e}", case.name));
    match &case.value {
        Some(value) => {
            let value: FormFieldValue = serde_json::from_value(value.clone())
                .unwrap_or_else(|e| panic!("{}: the value does not decode: {e}", case.name));
            set_value(owned.document(), &reference, &value, &CancelToken::never())
        }
        None => reset(owned.document(), &reference, &CancelToken::never()),
    }
}

#[test]
fn a_written_field_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 10, "the case list shrank");
    let mut written = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        written += 1;
        let result = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));

        assert_eq!(
            test_support::canonical_numbers(
                &serde_json::to_value(&result.field).expect("serialize")
            ),
            test_support::canonical_numbers(case.field.as_ref().expect("a recorded field")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            serde_json::to_value(&result.changed_widgets).expect("serialize"),
            serde_json::Value::Array(case.changed_widgets.clone()),
            "{}: the changed-widget set differs",
            case.name,
        );
    }

    assert!(written > 0, "no case writes anything");
}

#[test]
fn a_refused_write_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
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

    assert!(refusals >= 4, "the refusal list shrank");
}

#[test]
fn a_family_mismatch_says_which_two_families() {
    // The refusal CODE is the same one PDFium gives for a value the field
    // could have held, so the vector comparison cannot tell a caller that
    // confused two fields from one that sent a bad value. The message can,
    // and it is this engine's own, so it is pinned here rather than against
    // the oracle.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "toggle-on-text-refused").expect("the case");
    let error = run(&file, case).expect_err("a toggle does not apply to a text field");
    let message = error.to_string();
    assert!(message.contains("toggle"), "the message does not name the value: {message}");
    assert!(message.contains("Text"), "the message does not name the family: {message}");
}

#[test]
fn an_unwritable_field_says_why_it_is_unwritable() {
    // Same reasoning as the family mismatch: PDFium refuses a write with
    // object number zero too, and with the same code, so only the message
    // separates "this field cannot be written" from "that value is wrong".
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "direct-field-refused").expect("the case");
    let message = run(&file, case).expect_err("a direct field has no object number").to_string();
    assert!(
        message.contains("direct object"),
        "the message does not say why the field is unwritable: {message}",
    );
}

#[test]
fn a_form_model_is_a_snapshot_and_a_write_makes_it_stale() {
    // Why every operation loads a SECOND model to read its result back. The
    // first one walked the field tree before the write and is correct
    // forever about a document that no longer exists.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let reference = FormFieldRef::ObjectNumber { field: 4 };

    let stale = owned.document().form_model().expect("a form model");
    let index = stale.field_index_by_object_number(4).expect("the field is in the model");
    let before = stale.field(index).expect("the index resolved").value_at(0).expect("a value read");

    set_value(
        owned.document(),
        &reference,
        &FormFieldValue::Text { value: String::from("Grace Hopper") },
        &CancelToken::never(),
    )
    .expect("the write lands");

    let after_stale =
        stale.field(index).expect("the index still resolves").value_at(0).expect("a value read");
    assert_eq!(before, after_stale, "the old model reported the write, so it is not a snapshot");
    assert_eq!(
        read_value(owned.document(), &reference)["value"],
        serde_json::Value::from("Grace Hopper"),
        "a fresh model does not report the write",
    );
}

#[test]
fn a_refused_write_leaves_the_field_alone() {
    // A refusal that had already written something would be worse than one
    // that wrote nothing at all: the caller is told its value did not land
    // and the document says otherwise. Both halves are checked against one
    // document, because a fresh one would say nothing about the first.
    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "unknown-toggle-state-refused")
        .expect("the unknown-state case");

    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let reference: FormFieldRef =
        serde_json::from_value(case.reference.clone()).expect("the ref decodes");
    let value: FormFieldValue =
        serde_json::from_value(case.value.clone().expect("a value")).expect("the value decodes");

    let before = read_value(owned.document(), &reference);
    set_value(owned.document(), &reference, &value, &CancelToken::never())
        .expect_err("the state is not one the group has");
    assert_eq!(before, read_value(owned.document(), &reference), "the refusal wrote something");
}

/// The field's value, as the reader reports it.
fn read_value(document: &pdfium::Document, reference: &FormFieldRef) -> serde_json::Value {
    let model = document.form_model().expect("a form model");
    let snapshot =
        epdf_engine::features::forms::snapshot::read_form_snapshot(&model, &CancelToken::never())
            .expect("the form lists");
    let field = snapshot
        .fields
        .iter()
        .find(|field| &field.base.field_ref == reference)
        .expect("the field is in the snapshot");
    serde_json::to_value(&field.kind).expect("serialize")
}

/// Open a fresh copy of the fixture for a document-wide case.
fn open(file: &VectorFile, name: &str) -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{name}: read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn declared_format(case: &DocumentCase) -> Option<pdfium::FormDataFormat> {
    if !case.declare_format {
        return None;
    }
    match case.import_of.as_deref() {
        Some("fdf") => Some(pdfium::FormDataFormat::Fdf),
        Some("xfdf") => Some(pdfium::FormDataFormat::Xfdf),
        other => panic!("unhandled format {other:?}"),
    }
}

#[test]
fn an_import_applies_what_the_typescript_engine_applied() {
    let file = load();
    let mut imports = 0usize;

    for case in &file.document_cases {
        if case.repair.is_some() || case.refused {
            continue;
        }
        imports += 1;
        let session = open(&file, &case.name);
        let owned = session.document().expect("open");
        let result = epdf_engine::features::forms::write::import_data(
            owned.document(),
            &case.payload,
            declared_format(case),
            &CancelToken::never(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));

        let recorded = case.counts.as_ref().expect("recorded counts");
        for key in ["fieldsTotal", "fieldsApplied", "fieldsSkipped", "widgetsChanged"] {
            let mine = match key {
                "fieldsTotal" => result.fields_total,
                "fieldsApplied" => result.fields_applied,
                "fieldsSkipped" => result.fields_skipped,
                _ => result.widgets_changed,
            };
            assert_eq!(
                serde_json::Value::from(mine),
                recorded[key],
                "{}: {key}: {}",
                case.name,
                case.covers,
            );
        }
        assert_eq!(
            test_support::canonical_numbers(
                &serde_json::to_value(&result.snapshot).expect("serialize")
            ),
            test_support::canonical_numbers(case.snapshot.as_ref().expect("a snapshot")),
            "{}: the form after the import differs",
            case.name,
        );
    }

    assert!(imports >= 4, "the import list shrank");
}

#[test]
fn a_repair_reports_what_the_typescript_engine_reported() {
    let file = load();
    let mut repairs = 0usize;

    for case in &file.document_cases {
        let Some(options) = &case.repair else { continue };
        repairs += 1;
        let session = open(&file, &case.name);
        let owned = session.document().expect("open");
        let result = epdf_engine::features::forms::write::repair(
            owned.document(),
            options.bake_appearances,
            &CancelToken::never(),
        )
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            test_support::canonical_numbers(&serde_json::to_value(&result).expect("serialize")),
            test_support::canonical_numbers(case.report.as_ref().expect("a report")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert_eq!(repairs, 2, "both repair modes are covered");
}

#[test]
fn an_unreadable_payload_is_refused() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.document_cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let session = open(&file, &case.name);
        let owned = session.document().expect("open");
        let error = epdf_engine::features::forms::write::import_data(
            owned.document(),
            &case.payload,
            declared_format(case),
            &CancelToken::never(),
        )
        .expect_err(&case.name);
        assert_eq!(
            case.refused_with.as_deref(),
            Some("InvalidArg"),
            "{}: unhandled refusal code",
            case.name,
        );
        assert!(error.is(EngineErrorCode::InvalidArg), "{}: {error}", case.name);
    }

    assert!(refusals > 0, "no payload is refused");

    // An EMPTY payload is refused by this layer rather than by PDFium,
    // which answers it the same way it answers malformed bytes. Nothing
    // sent nothing, and saying so is more useful than saying the nothing
    // was badly formed.
    let session = open(&file, "empty");
    let owned = session.document().expect("open");
    let error = epdf_engine::features::forms::write::import_data(
        owned.document(),
        &[],
        None,
        &CancelToken::never(),
    )
    .expect_err("an empty payload");
    assert!(error.to_string().contains("empty"), "{error}");
}
