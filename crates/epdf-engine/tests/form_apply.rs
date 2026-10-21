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

#[test]
fn a_refused_change_leaves_every_op_before_it_unwritten() {
    // All or none: the first op writes, the second names no field, and the
    // change is refused with the first op taken back, its value and the
    // widget it drew.
    let file = load();
    let mut replay = Replay::open(&file.fixture, "all-or-none");
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let read = |replay: &mut Replay| {
        let form = replay.replay(&json!({ "kind": "forms.list" })).expect("the form lists");
        form["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["ref"] == text)
            .cloned()
            .expect("the text field")
    };
    let before = read(&mut replay);
    let refused = replay.replay(&json!({ "kind": "document.apply", "change": { "ops": [
        { "type": "forms.setValue", "field": text, "value": { "value": "Changed" } },
        { "type": "forms.setDisplay", "field": text, "display": "hidden" },
        { "type": "forms.setValue",
          "field": { "kind": "objectNumber", "objectNumber": 9999 }, "value": { "value": "x" } },
    ] } }));
    assert_eq!(refused.expect_err("the missing field refuses")["code"], json!("NotFound"));
    assert_eq!(read(&mut replay), before, "a refused change wrote its first ops");
}

#[test]
fn a_value_write_undoes_and_redoes() {
    let file = load();
    let mut replay = Replay::open(&file.fixture, "undo");
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let value = |replay: &mut Replay| {
        let form = replay.replay(&json!({ "kind": "forms.list" })).expect("the form lists");
        form["fields"].as_array().unwrap().iter().find(|f| f["ref"] == text).unwrap()["value"]
            .clone()
    };
    let original = value(&mut replay);
    replay
        .replay(&json!({ "kind": "document.apply", "opId": "fill", "change": { "ops": [
            { "type": "forms.setValue", "field": text, "value": { "value": "Ada" } },
            { "type": "forms.setDisplay", "field": text, "display": "noPrint" },
        ] } }))
        .expect("the fill applies");
    assert_eq!(value(&mut replay), json!("Ada"));
    let undone = replay
        .replay(&json!({ "kind": "document.apply", "opId": "undo",
                         "change": { "undoOf": "fill" } }))
        .expect("the fill undoes");
    assert_eq!(value(&mut replay), original);
    let types: Vec<&Value> =
        undone["result"]["items"].as_array().unwrap().iter().map(|i| &i["type"]).collect();
    assert_eq!(types, [&json!("forms.setDisplay"), &json!("forms.setValue")], "{undone}");
    replay
        .replay(&json!({ "kind": "document.apply", "opId": "redo",
                         "change": { "undoOf": "undo" } }))
        .expect("the undo redoes");
    assert_eq!(value(&mut replay), json!("Ada"));
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
}
