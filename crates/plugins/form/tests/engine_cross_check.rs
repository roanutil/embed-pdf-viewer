//! The form's value rules are a pure copy (`src/engine.rs`) of the engine's,
//! because `epdf-engine` links PDFium and this crate builds for wasm32
//! (Ruling13.15). On a native host the two copies are held together here:
//! what a write or a reset leaves of a field, and which writes are refused,
//! for each family's corners.

#![cfg(not(target_arch = "wasm32"))]

use epdf_engine::features::forms::snapshot::FormFieldDto;
use epdf_engine::features::forms::value::{FormFieldValue, field_after_reset, field_after_value};
use epdf_engine::page_space::PageCoordinates;
use epdf_plugin_form::engine::{self, FieldWithWidgets};
use serde_json::{Value, json};

fn widget(n: u32, on: &str, checked: bool) -> Value {
    let page = json!({ "kind": "objectNumber", "objectNumber": 1 });
    json!({
        "ref": { "kind": "objectNumber", "page": page, "objectNumber": n },
        "objectNumber": n, "page": page,
        "onState": on, "exportValue": on, "checked": checked,
    })
}

fn base(n: u32, name: &str, value_entry: Value, default_entry: Value) -> Value {
    json!({
        "ref": { "kind": "objectNumber", "objectNumber": n }, "name": name, "origin": "acroform",
        "readOnly": false, "required": false, "noExport": false,
        "alternateName": null, "mappingName": null,
        "valueEntry": value_entry, "defaultValueEntry": default_entry,
        "groupId": null, "createdBy": null, "createdAt": null, "filledBy": null,
        "filledByName": null, "filledAt": null, "importedBy": null,
    })
}

fn with(mut field: Value, members: Value) -> Value {
    for (key, value) in members.as_object().unwrap() {
        field[key] = value.clone();
    }
    field
}

fn plain_widget(n: u32) -> Value {
    let page = json!({ "kind": "objectNumber", "objectNumber": 1 });
    json!({ "ref": { "kind": "objectNumber", "page": page, "objectNumber": n }, "objectNumber": n, "page": page })
}

fn options(values: &[&str], selected: &[&str]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|v| json!({ "label": v.to_uppercase(), "value": v, "selected": selected.contains(v) }))
            .collect(),
    )
}

fn fields() -> Vec<Value> {
    let scalar = |v: &str| json!({ "kind": "scalar", "value": v });
    let none = json!({ "kind": "none" });
    vec![
        with(
            base(1, "text", scalar("abc"), scalar("dflt")),
            json!({
                "family": "text", "value": "abc", "defaultValue": "dflt", "maxLength": 4,
                "multiline": false, "password": false, "comb": false, "widgets": [plain_widget(101)],
            }),
        ),
        with(
            base(2, "free", none.clone(), none.clone()),
            json!({
                "family": "text", "value": "", "defaultValue": "", "maxLength": null,
                "multiline": true, "password": false, "comb": false, "widgets": [plain_widget(102)],
            }),
        ),
        with(
            base(3, "agree", scalar("Off"), scalar("Yes")),
            json!({
                "family": "checkbox", "checked": false, "exportValue": "Yes",
                "widgets": [widget(103, "Yes", false), widget(104, "Yes", false)],
            }),
        ),
        with(
            base(5, "size", scalar("M"), scalar("1")),
            json!({
                "family": "radio", "value": "M", "radiosInUnison": false, "noToggleToOff": true,
                "widgets": [widget(105, "S", false), widget(106, "M", true), widget(107, "M", false)],
            }),
        ),
        with(
            base(8, "unison", scalar("Off"), none.clone()),
            json!({
                "family": "radio", "value": "Off", "radiosInUnison": true, "noToggleToOff": false,
                "widgets": [widget(108, "A", false), widget(109, "A", false), widget(110, "B", false)],
            }),
        ),
        with(
            base(11, "colour", scalar("r"), scalar("")),
            json!({
                "family": "combobox", "value": "r", "defaultValue": "", "edit": true,
                "options": options(&["r", "g", ""], &["r"]), "widgets": [plain_widget(111)],
            }),
        ),
        with(
            base(
                12,
                "colours",
                json!({ "kind": "array", "values": ["g", "r"] }),
                json!({ "kind": "array", "values": ["b", "r"] }),
            ),
            json!({
                "family": "listbox", "selectedValues": ["r", "g"], "defaultValue": ["r", "b"],
                "multiSelect": true, "options": options(&["r", "g", "b"], &["r", "g"]), "topIndex": 0,
                "widgets": [plain_widget(112)],
            }),
        ),
        with(
            base(13, "single", none.clone(), none),
            json!({
                "family": "listbox", "selectedValues": [], "defaultValue": [], "multiSelect": false,
                "options": options(&["x", "y"], &[]), "topIndex": 1, "widgets": [plain_widget(113)],
            }),
        ),
    ]
}

fn values() -> Vec<Value> {
    vec![
        json!({ "value": "abcdefg" }),
        json!({ "value": "a😀b😀c" }),
        json!({ "value": null }),
        json!({ "value": "" }),
        json!({ "value": "Off" }),
        json!({ "value": "M" }),
        json!({ "value": "A" }),
        json!({ "value": "Yes" }),
        json!({ "value": "nope" }),
        json!({ "value": "g" }),
        json!({ "checked": true }),
        json!({ "checked": false }),
        json!({ "selectedValues": [] }),
        json!({ "selectedValues": ["g", "r"] }),
        json!({ "selectedValues": ["r", "r"] }),
        json!({ "selectedValues": ["x"] }),
        json!({ "selectedValues": ["q"] }),
    ]
}

fn engine_field(field: &Value) -> FormFieldDto<PageCoordinates> {
    serde_json::from_value(field.clone()).unwrap_or_else(|e| panic!("{e}: {field}"))
}

#[test]
fn a_value_write_leaves_what_the_engine_leaves() {
    let mut compared = 0;
    let mut written = 0;
    for field in fields() {
        for value in values() {
            let ours = engine::field_after_value(
                &FieldWithWidgets { field: field.clone(), widgets: vec![] },
                &value,
            );
            let typed: FormFieldValue = serde_json::from_value(value.clone()).unwrap();
            let theirs = field_after_value(&engine_field(&field), &typed);
            match (ours, theirs) {
                (Ok(ours), Ok(theirs)) => {
                    assert_eq!(
                        ours.field,
                        serde_json::to_value(&theirs).unwrap(),
                        "{field} {value}"
                    );
                    written += 1;
                }
                (Err(_), Err(_)) => {}
                (ours, theirs) => panic!(
                    "{field} {value}: ours {:?}, the engine's {:?}",
                    ours.map(|o| o.field),
                    theirs.map(|t| serde_json::to_value(&t).unwrap())
                ),
            }
            compared += 1;
        }
    }
    assert_eq!(compared, 8 * 17);
    eprintln!("{written} writes of {compared} were taken");
    assert!(written >= 40, "too few writes taken to compare: {written}");
}

#[test]
fn a_reset_leaves_what_the_engine_leaves() {
    for field in fields() {
        let ours =
            engine::field_after_reset(&FieldWithWidgets { field: field.clone(), widgets: vec![] });
        let theirs = field_after_reset(&engine_field(&field));
        match (ours, theirs) {
            (Ok(ours), Ok(theirs)) => {
                assert_eq!(ours.field, serde_json::to_value(&theirs).unwrap(), "{field}");
            }
            (Err(_), Err(_)) => {}
            (ours, theirs) => panic!(
                "{field}: ours {:?}, the engine's {:?}",
                ours.map(|o| o.field),
                theirs.map(|t| serde_json::to_value(&t).unwrap())
            ),
        }
    }
}
