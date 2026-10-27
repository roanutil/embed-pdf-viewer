//! A form script's effects as one change: `forms/apply.json` replayed
//! through `Host::call`.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run form-apply
//!
//! Upstream removed `forms.applyEffects` (98e5c270c): a script run's effects
//! are one `document.apply` change now, its ops in order, all of them or
//! none. Each case records the ops it sent; this wraps them as the
//! `document.apply` request the TypeScript engine's `doc.apply` sent, under
//! the case's `opId` and with the envelope's other fields left to their
//! defaults, and compares the change's answer, or its refusal's code.

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
    ops: Value,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    refused_with: Option<String>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("forms").join("apply.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("apply.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "document.apply.forms");
    file
}

/// The case's change, as `doc.apply` sent it: its `opId` is the case's,
/// which is the one its answer's meta names.
fn request(case: &Case) -> Value {
    json!({
        "kind": "document.apply",
        "jobId": "<job>",
        "opId": format!("form-apply:{}", case.name),
        "change": { "ops": case.ops },
    })
}

#[test]
fn every_change_agrees_with_the_typescript_engine() {
    let file = load();
    assert_eq!(file.cases.len(), 23, "the case list moved");
    let (mut applied, mut refused) = (0, 0);
    for case in &file.cases {
        // A fresh document per case: several write the same field.
        let mut replay = Replay::open(&file.fixture, &format!("form-apply:{}", case.name));
        let answer = replay.replay(&request(case));
        match (&case.result, &case.refused_with) {
            (Some(expected), None) => {
                applied += 1;
                let answer = answer.unwrap_or_else(|e| panic!("{}: refused: {e}", case.name));
                assert_eq!(
                    test_support::canonical_numbers(&answer["result"]),
                    test_support::canonical_numbers(expected),
                    "{}: {}",
                    case.name,
                    case.covers
                );
            }
            (None, Some(code)) => {
                refused += 1;
                let error = answer.expect_err(&case.name);
                assert_eq!(error["code"], json!(code), "{}: {} ({error})", case.name, case.covers);
            }
            _ => panic!("{}: records neither a result nor a refusal", case.name),
        }
    }
    assert_eq!((applied, refused), (14, 9), "the applied and refused cases moved");
}

/// The dictionaries the form ops below write: the text field (its own
/// widget), the radio group 9 and its widgets 7 and 8.
const WRITTEN: [u32; 4] = [4, 9, 7, 8];

/// What a write could leave behind: the form as `forms.list` reads it (the
/// fields' values and every widget row's flags and appearance state), and
/// each written dictionary's canonical bytes with its `/AP` and `/MK`
/// streams.
fn state_of(replay: &mut Replay) -> (Value, Vec<Option<Vec<u8>>>) {
    let form = replay.replay(&json!({ "kind": "forms.list" })).expect("the form lists");
    let key = epdf_engine::session::registry::SessionKey::document(replay.doc_id.as_str());
    let session = replay.host.sessions().session(&key).expect("the session");
    let document = session.document().expect("open").document();
    let canonical =
        WRITTEN.iter().map(|n| document.export_dict_canonical(*n, "AP MK", "")).collect();
    (form, canonical)
}

fn widget_row(form: &Value, object_number: u64) -> Value {
    form["widgets"]
        .as_array()
        .expect("widgets")
        .iter()
        .find(|row| row["ref"]["objectNumber"] == object_number)
        .cloned()
        .unwrap_or_else(|| panic!("no widget {object_number}"))
}

#[test]
fn a_refused_change_leaves_every_op_before_it_unwritten() {
    // All or none: the first ops write a value, hide a widget and keep a
    // radio group from printing; the last names no field, and the change
    // is refused with every op before it taken back, values, flags and
    // pictures alike.
    let file = load();
    let mut replay = Replay::open(&file.fixture, "all-or-none");
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let radio = json!({ "kind": "objectNumber", "objectNumber": 9 });
    let before = state_of(&mut replay);
    assert!(before.1.iter().all(Option::is_some), "a written object has no dictionary");
    let refused = replay.replay(&json!({ "kind": "document.apply", "change": { "ops": [
        { "type": "forms.setValue", "field": text, "value": { "value": "Changed" } },
        { "type": "forms.setDisplay", "field": text, "display": "hidden" },
        { "type": "forms.setValue", "field": radio, "value": { "value": "Red" } },
        { "type": "forms.setDisplay", "field": radio, "display": "noPrint" },
        { "type": "forms.setValue",
          "field": { "kind": "objectNumber", "objectNumber": 9999 }, "value": { "value": "x" } },
    ] } }));
    assert_eq!(refused.expect_err("the missing field refuses")["code"], json!("NotFound"));
    let after = state_of(&mut replay);
    assert_eq!(widget_row(&after.0, 4)["hidden"], json!(false), "a hidden widget was left");
    assert_eq!(widget_row(&after.0, 7)["print"], json!(true), "a noPrint widget was left");
    assert_eq!(after.0, before.0, "a refused change left the form changed");
    assert_eq!(after.1, before.1, "a refused change left a dictionary changed");
}

#[test]
fn a_value_write_undoes_and_redoes() {
    let file = load();
    let mut replay = Replay::open(&file.fixture, "undo");
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let radio = json!({ "kind": "objectNumber", "objectNumber": 9 });
    let original = state_of(&mut replay);
    let filled = replay
        .replay(&json!({ "kind": "document.apply", "opId": "fill", "change": { "ops": [
            { "type": "forms.setValue", "field": text, "value": { "value": "Ada" } },
            { "type": "forms.setDisplay", "field": text, "display": "hidden" },
            { "type": "forms.setValue", "field": radio, "value": { "value": "Red" } },
        ] } }))
        .expect("the fill applies");
    assert_eq!(filled["result"]["meta"]["undoable"], json!(true), "{filled}");
    let written = state_of(&mut replay);
    assert_eq!(widget_row(&written.0, 4)["hidden"], json!(true));
    assert_eq!(widget_row(&written.0, 7)["appearanceState"], json!("Red"));
    assert_ne!(written.1, original.1, "the fill wrote no dictionary");

    let undone = replay
        .replay(&json!({ "kind": "document.apply", "opId": "undo",
                         "change": { "undoOf": "fill" } }))
        .expect("the fill undoes");
    let types: Vec<&Value> =
        undone["result"]["items"].as_array().unwrap().iter().map(|i| &i["type"]).collect();
    assert_eq!(
        types,
        [&json!("forms.setValue"), &json!("forms.setDisplay"), &json!("forms.setValue")],
        "{undone}"
    );
    let back = state_of(&mut replay);
    assert_eq!(widget_row(&back.0, 4)["hidden"], json!(false), "the undo left the widget hidden");
    assert_eq!(back.0, original.0, "the undo left the form changed");
    assert_eq!(back.1, original.1, "the undo left a dictionary changed");

    replay
        .replay(&json!({ "kind": "document.apply", "opId": "redo",
                         "change": { "undoOf": "undo" } }))
        .expect("the undo redoes");
    let again = state_of(&mut replay);
    assert_eq!(again.0, written.0, "the redo is not the fill");
    assert_eq!(again.1, written.1, "the redo wrote other dictionaries");
}

#[test]
fn a_field_the_caller_may_not_fill_refuses_the_change() {
    // `authorizeFill`: a field in no group takes `doc.forms.fill`, and the
    // refusal names it (`authorizeFieldWrite`).
    let file = load();
    let mut replay = Replay::open(&file.fixture, "rights");
    let bits = json!({ "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
        "bit10": true, "bit11": true, "bit12": true });
    let refused = replay.replay(&json!({ "kind": "document.apply",
        "authority": { "identity": {}, "grants": { "scope": ["doc.forms.read"], "pdfBits": bits },
                       "protection": null },
        "change": { "ops": [{ "type": "forms.setValue",
            "field": { "kind": "objectNumber", "objectNumber": 4 }, "value": { "value": "x" } }] } }));
    let error = refused.expect_err("the caller may not fill the field");
    assert_eq!(error["code"], json!("Forbidden"), "{error}");
    assert_eq!(error["message"], json!("permission denied (target): doc.forms.fill"), "{error}");
    assert_eq!(
        error["details"],
        json!({ "required": "doc.forms.fill", "context": "target" }),
        "{error}"
    );
}
