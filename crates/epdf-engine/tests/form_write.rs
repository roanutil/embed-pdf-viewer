//! G12's first gate: a form field says what the TypeScript engine made it say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run form-write
//!
//! Each case's recorded `request` (`forms.setValue` or `forms.reset`) is
//! replayed through `Host::call` with `common::replay`.
//!
//! # Two halves, and the second is the one a field comparison misses
//!
//! A value write reports the WIDGETS whose appearance it re-baked
//! (`meta.changedWidgets`, 7bccc4f35), and that set is not derivable from
//! the value. A radio switch changes two, the one turning on and the one
//! turning off. Writing an already-checked checkbox changes none. So the
//! list is compared alongside the field, and a writer that reported the
//! wrong set would otherwise pass.
//!
//! The import cases went with FDF and XFDF (7f0b73a3c): form bundles
//! replaced them, which `form_transfer.rs` covers.

mod common;

use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Value, json};

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

/// A repair: the operation that touches the whole form rather than one
/// field.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentCase {
    name: String,
    covers: String,
    #[serde(default)]
    repair: Option<RepairOptions>,
    #[serde(default)]
    report: Option<Value>,
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
    #[serde(default)]
    refused: bool,
    /// The error CODE the TypeScript reported. The message is deliberately
    /// not recorded: the two engines word their refusals differently, and
    /// comparing prose would pin the wording rather than the behaviour.
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    field: Option<Value>,
    /// The wire request the TypeScript sent.
    request: Value,
    #[serde(default)]
    changed_widgets: Vec<Value>,
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

/// The field `reference` names, as `forms.list` reads it now.
fn read_field(replay: &mut Replay, reference: &Value) -> Value {
    let form = replay.replay(&json!({ "kind": "forms.list" })).expect("the form lists");
    form["fields"]
        .as_array()
        .expect("fields")
        .iter()
        .find(|field| {
            &field["ref"] == reference
                || (reference["kind"] == "fqn" && field["name"] == reference["name"])
        })
        .cloned()
        .unwrap_or_else(|| panic!("no field {reference} in {form}"))
}

/// Run one case against a fresh copy of the fixture.
///
/// Fresh per case, deliberately: several cases write the same field, and a
/// shared document would make each one depend on the order of the ones
/// before it.
fn run(file: &VectorFile, case: &Case) -> (Replay, Result<Value, Value>) {
    let mut replay = Replay::open(&file.fixture, &format!("form-write:{}", case.name));
    let answer = replay.replay(&case.request);
    (replay, answer)
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
        let (mut replay, answer) = run(&file, case);
        let answer = answer.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let result = &answer["result"];
        let field = match result.get("field") {
            Some(field) => field.clone(),
            // A reset answers the fields it changed: the field is read back.
            None => read_field(&mut replay, &case.request["refs"][0]),
        };
        assert_eq!(
            test_support::canonical_numbers(&field),
            test_support::canonical_numbers(case.field.as_ref().expect("a recorded field")),
            "{}: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            result["meta"]["changedWidgets"],
            Value::Array(case.changed_widgets.clone()),
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
        let (_, answer) = run(&file, case);
        let error = answer.expect_err(&case.name);
        let expected = case.refused_with.as_deref().expect("a recorded code");
        assert_eq!(error["code"], json!(expected), "{}: {} (got {error})", case.name, case.covers);
    }

    assert!(refusals >= 4, "the refusal list shrank");
}

#[test]
fn a_family_mismatch_says_which_family_and_which_shape() {
    // The refusal CODE is the same one PDFium gives for a value the field
    // could have held, so the vector comparison cannot tell a caller that
    // confused two fields from one that sent a bad value. The message can:
    // `fieldValueWriteOf` names the family and the shapes it takes.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "toggle-on-text-refused").expect("the case");
    let (_, answer) = run(&file, case);
    let error = answer.expect_err("a checked value does not apply to a text field");
    assert_eq!(error["code"], json!("InvalidArg"), "{error}");
    let message = error["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("text field takes { value }") && message.contains("{ checked }"),
        "the message does not name the family and the shape: {message}",
    );
}

#[test]
fn a_direct_field_is_not_found_by_the_number_a_write_takes() {
    // A field stored as a direct object has no object number: a read names
    // it by its full name, and the write, which resolves the field again by
    // the number the read found (0), finds nothing (`writeValue`).
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "direct-field-refused").expect("the case");
    let (_, answer) = run(&file, case);
    let error = answer.expect_err("a direct field has no object number");
    assert_eq!(error["code"], json!("NotFound"), "{error}");
    assert!(error["message"].as_str().unwrap_or_default().contains("object 0"), "{error}");
}

#[test]
fn a_form_model_is_a_snapshot_and_a_write_makes_it_stale() {
    // Why every operation loads a SECOND model to read its result back. The
    // first one walked the field tree before the write and is correct
    // forever about a document that no longer exists.
    let file = load();
    let mut replay = Replay::open(&file.fixture, "stale");
    let stale_value = {
        let session = replay
            .host
            .sessions()
            .session(&epdf_engine::session::registry::SessionKey::document("stale"))
            .expect("the session");
        let document = session.document().expect("open").document();
        let stale = document.form_model().expect("a form model");
        let index = stale.field_index_by_object_number(4).expect("the field is in the model");
        let before =
            stale.field(index).expect("the index resolved").value_at(0).expect("a value read");
        drop(stale);
        before
    };
    let reference = json!({ "kind": "objectNumber", "objectNumber": 4 });
    replay
        .replay(&json!({ "kind": "forms.setValue", "ref": reference,
                         "value": { "value": "Grace Hopper" } }))
        .expect("the write lands");
    // The model loaded before the write still reads what was there, and a
    // fresh one reads the write.
    let after = {
        let session = replay
            .host
            .sessions()
            .session(&epdf_engine::session::registry::SessionKey::document("stale"))
            .expect("the session");
        let document = session.document().expect("open").document();
        let fresh = document.form_model().expect("a form model");
        let index = fresh.field_index_by_object_number(4).expect("the field");
        fresh.field(index).expect("the index resolved").value_at(0).expect("a value")
    };
    assert_ne!(stale_value, after, "the write did not land");
    assert_eq!(read_field(&mut replay, &reference)["value"], json!("Grace Hopper"));
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
    let mut replay = Replay::open(&file.fixture, "refused");
    let reference = case.request["ref"].clone();
    let before = read_field(&mut replay, &reference);
    replay.replay(&case.request).expect_err("the state is not one the group has");
    assert_eq!(before, read_field(&mut replay, &reference), "the refusal wrote something");
}

#[test]
fn a_repair_reports_what_the_typescript_engine_reported() {
    let file = load();
    let mut repairs = 0usize;

    for case in &file.document_cases {
        let Some(options) = &case.repair else { continue };
        repairs += 1;
        let mut replay = Replay::open(&file.fixture, &case.name);
        let mut answer = replay
            .replay(&json!({ "kind": "forms.repair", "opId": "repair",
                             "bakeAppearances": options.bake_appearances }))
            .unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let mut result = answer["result"].take();
        // Not recorded: the write's meta, which names no page and can't be
        // undone (a repair is a final change).
        let meta = result.as_object_mut().and_then(|r| r.remove("meta"));
        assert_eq!(
            meta,
            Some(json!({ "affectedPages": [], "cacheDelta": null, "opId": "repair",
                         "undoable": false })),
            "{}",
            case.name
        );
        assert_eq!(
            test_support::canonical_numbers(&result),
            test_support::canonical_numbers(case.report.as_ref().expect("a report")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert_eq!(repairs, 2, "both repair modes are covered");
}
