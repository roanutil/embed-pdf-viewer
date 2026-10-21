//! G12's structural gate: a form field is made, changed and removed the way
//! the TypeScript engine does it.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run form-field
//!
//! Each case's recorded `request` (`forms.createField`, `forms.updateField`,
//! `forms.deleteField` or `forms.detachWidget`) is replayed through
//! `Host::call` with `common::replay`; the case literals keep the spelling
//! the engine took before upstream's v3 forms (`fieldObjectNumber`,
//! `pageObjectNumber`, `onState`), which no request carries now.
//!
//! # Every case opens its own copy
//!
//! A create hands out object numbers that depend on what the document
//! already holds, so a shared document would make each case's numbers
//! depend on the cases before it.

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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(default)]
    draft: Option<Value>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    /// The wire request the TypeScript sent.
    request: Value,
    #[serde(default)]
    result: Option<Value>,
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

fn case<'a>(file: &'a VectorFile, name: &str) -> &'a Case {
    file.cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

/// The pool of object numbers upstream's local engine reserves for the
/// creates its main thread names: a create that names none is numbered past
/// it.
const OBJECT_NUMBER_POOL_SIZE: u32 = 64;

/// The fixture open as upstream's local engine opens it: `open.fatMem`, a
/// layer over the bytes whose new objects are numbered past the base
/// file's, with the pool its main thread names reserved.
fn open(file: &VectorFile, doc_id: &str) -> Replay {
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": doc_id,
            "password": null, "reserveObjectNumbers": OBJECT_NUMBER_POOL_SIZE })
        .to_string(),
        vec![test_support::fixture(&file.fixture)],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, doc_id)
}

/// Run one case against a fresh copy of the fixture.
fn run(file: &VectorFile, case: &Case) -> (Replay, Result<Value, Value>) {
    let mut replay = open(file, &format!("form-field:{}", case.name));
    let answer = replay.replay(&case.request);
    (replay, answer)
}

/// What a case records of a write's answer: the field read back, or, for a
/// delete, what its meta says went (`changedFields`, `changedWidgets`).
fn recorded_shape(kind: &str, answer: &Value) -> Value {
    let result = &answer["result"];
    if kind == "forms.deleteField" {
        return json!({
            "changedFields": result["meta"]["changedFields"],
            "changedWidgets": result["meta"]["changedWidgets"],
        });
    }
    json!({ "field": result["field"] })
}

/// The form as `forms.list` reads it now.
fn form(replay: &mut Replay) -> Value {
    replay.replay(&json!({ "kind": "forms.list" })).expect("the form lists")
}

#[test]
fn a_structural_write_leaves_what_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 20, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let kind = case.request["kind"].as_str().expect("a kind");
        let (_, answer) = run(&file, case);
        let answer = answer.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            test_support::canonical_numbers(&recorded_shape(kind, &answer)),
            test_support::canonical_numbers(case.result.as_ref().expect("a recorded result")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert!(applied >= 15, "the applied list shrank");
}

#[test]
fn a_refused_structural_write_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let (_, answer) = run(&file, case);
        let error = answer.expect_err(&case.name);
        assert_eq!(
            error["code"].as_str(),
            case.refused_with.as_deref(),
            "{}: {} (got {error})",
            case.name,
            case.covers,
        );
    }

    assert!(refusals >= 7, "the refusal list shrank");
}

#[test]
fn a_draft_refusal_says_what_was_wrong_with_the_draft() {
    // The messages are upstream's (`FormMutator.createField`,
    // `onStateOf`): an unnamed field is the native create's refusal, options
    // on a text field the options write's, and a stateless radio button the
    // placement's own, before anything is written.
    let file = load();
    for (name, expected) in [
        ("create-unnamed-refused", "cannot create field \"\" (name conflict or invalid)"),
        ("create-text-with-options-refused", "options rejected"),
        (
            "create-radio-without-state-refused",
            "every radio button needs an exportValue other than \"Off\"",
        ),
    ] {
        let (_, answer) = run(&file, case(&file, name));
        let error = answer.expect_err(name);
        assert_eq!(error["message"], expected, "{name}");
    }
}

#[test]
fn a_refused_create_leaves_no_field_behind() {
    // Options on a text field are refused by the options write, after the
    // field exists: the create is all or none, so the field goes with it.
    let file = load();
    let refused = case(&file, "create-text-with-options-refused");
    let mut replay = open(&file, "form-field:all-or-none");
    let before = form(&mut replay);
    replay.replay(&refused.request).expect_err("options on a text field");
    let after = form(&mut replay);
    assert_eq!(after["fields"], before["fields"], "the refused create left a field");
    assert_eq!(after["widgets"], before["widgets"]);
}

#[test]
fn a_widget_lands_where_the_placement_put_it() {
    // The field's widget is a REFERENCE: an annotation and a page, and no
    // geometry. So a widget placed in the wrong box reads back identically,
    // and the only way to see it is to read its row.
    let file = load();
    let full = case(&file, "create-text-full");
    let (mut replay, answer) = run(&file, full);
    let answer = answer.expect("the field is created");
    let widget = answer["result"]["field"]["widgets"][0]["ref"].clone();
    let row = form(&mut replay)["widgets"]
        .as_array()
        .expect("widget rows")
        .iter()
        .find(|row| row["ref"] == widget)
        .cloned()
        .unwrap_or_else(|| panic!("no row for {widget}"));
    let placed = &full.request["draft"]["widgets"][0]["rect"];
    for edge in ["x", "y", "width", "height"] {
        let (got, want) = (row["rect"][edge].as_f64(), placed[edge].as_f64());
        assert!(
            (got.unwrap() - want.unwrap()).abs() < 0.01,
            "the widget sits at {}, not {placed}",
            row["rect"]
        );
    }
    // And in the file where the case's draft put it, in the file's
    // coordinates.
    let pdf = &full.draft.as_ref().expect("a draft")["widget"]["rect"];
    let height = pdf["top"].as_f64().unwrap() - pdf["bottom"].as_f64().unwrap();
    assert!((row["rect"]["height"].as_f64().unwrap() - height).abs() < 0.01);
    // The answer's row says the same.
    assert_eq!(answer["result"]["widgets"][0]["rect"], row["rect"]);
}

#[test]
fn a_radio_without_a_state_is_refused_before_the_field_exists() {
    // The adoption would refuse a stateless radio widget too, by when the
    // field and the widget both exist. So the check is in preflight.
    let file = load();
    let refused = case(&file, "create-radio-without-state-refused");
    let mut replay = open(&file, "form-field:radio");
    let before = form(&mut replay);
    replay.replay(&refused.request).expect_err("a radio widget needs a state");
    let after = form(&mut replay);
    assert_eq!(
        after["widgets"].as_array().map(Vec::len),
        before["widgets"].as_array().map(Vec::len),
        "the refusal left a widget behind",
    );
    assert_eq!(
        after["fields"].as_array().map(Vec::len),
        before["fields"].as_array().map(Vec::len),
        "the refusal left a field behind",
    );
}

#[test]
fn deleting_a_field_takes_its_widgets_off_their_pages() {
    // The half the vector cannot show: the field is gone, and so are its
    // widgets (upstream's cascade). A merged field/widget is one dictionary,
    // gone with it in the same write (60602f579).
    let file = load();
    for (name, widgets) in [("delete-radio-group", 2), ("delete-merged-field-refused", 1)] {
        let deleting = case(&file, name);
        let mut replay = open(&file, &format!("form-field:cascade-{name}"));
        let before = form(&mut replay)["widgets"].as_array().map(Vec::len).unwrap();
        let answer = replay.replay(&deleting.request).expect("the field deletes");
        let removed = answer["result"]["meta"]["changedWidgets"].as_array().unwrap().clone();
        assert_eq!(removed.len(), widgets, "{name}: every widget is reported");
        let after = form(&mut replay)["widgets"].as_array().unwrap().clone();
        assert_eq!(after.len(), before - widgets, "{name}: the widgets left their pages");
        for widget in &removed {
            assert!(after.iter().all(|row| row["ref"] != widget["ref"]), "{name}: {widget}");
        }
    }
}
