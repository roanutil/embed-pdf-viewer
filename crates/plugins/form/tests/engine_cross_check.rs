//! The form's engine rules are a pure copy (`src/engine.rs`) of the
//! engine's, because `epdf-engine` links PDFium and this crate builds for
//! wasm32 (Ruling13.15). On a native host the two copies are held together
//! here, for each family's corners: what a write, a reset or an update
//! leaves of a field, a draft's default, which widget members a family
//! takes, and where rows go when they are reordered; and which of those the
//! two refuse, with the same words.

#![cfg(not(target_arch = "wasm32"))]

use epdf_engine::error::EngineError;
use epdf_engine::features::forms::field::{
    FormFieldDraft, FormFieldPatch, default_entry_of_draft, fit_widget_to_family, form_field_after,
};
use epdf_engine::features::forms::snapshot::{FormFieldDto, FormFieldFamily};
use epdf_engine::features::forms::value::{FormFieldValue, field_after_reset, field_after_value};
use epdf_engine::mutation::list_position::{ListPosition, reorder_part, reordered_list};
use epdf_engine::page_space::PageCoordinates;
use epdf_kernel::Error;
use epdf_plugin_form::engine::{self, FieldWithWidgets, annotation_key};
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

/// A value with each whole number written as an integer, as JavaScript
/// prints `0` for the `0.0` a copy computes in floating point.
fn canonical(value: Value) -> Value {
    match value {
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.fract() == 0.0 && x.abs() < 9e15 => json!(x as i64),
            _ => Value::Number(n),
        },
        Value::Array(items) => Value::Array(items.into_iter().map(canonical).collect()),
        Value::Object(members) => {
            Value::Object(members.into_iter().map(|(k, v)| (k, canonical(v))).collect())
        }
        other => other,
    }
}

/// The two outcomes agree: the same value, or a refusal in the same words.
fn agree<T: std::fmt::Debug>(
    what: &str,
    ours: Result<Value, Error>,
    theirs: Result<T, EngineError>,
    theirs_json: impl Fn(T) -> Value,
) -> bool {
    match (ours, theirs) {
        (Ok(ours), Ok(theirs)) => {
            assert_eq!(canonical(ours), canonical(theirs_json(theirs)), "{what}");
            true
        }
        (Err(ours), Err(theirs)) => {
            assert_eq!(ours.message, theirs.message, "{what}");
            false
        }
        (ours, theirs) => panic!("{what}: ours {ours:?}, the engine's {theirs:?}"),
    }
}

/// Patches as the TypeScript types them. A list default on a text field is
/// not one: the engine refuses it, where the TypeScript (and this copy)
/// writes it as given; that pair is left out.
fn patches() -> Vec<Value> {
    vec![
        json!({}),
        json!({ "maxLength": 2 }),
        json!({ "maxLength": 10 }),
        json!({ "maxLength": null }),
        json!({ "defaultValue": "x" }),
        json!({ "defaultValue": null }),
        json!({ "defaultValue": ["r"] }),
        json!({ "defaultValue": ["r", "g"] }),
        json!({ "defaultValue": "g" }),
        json!({ "defaultValue": "q" }),
        json!({ "defaultValue": ["q"] }),
        json!({ "options": [{ "label": "Blue", "value": "b" }, { "label": "Red", "value": "r" }] }),
        json!({
            "options": [{ "label": "B", "value": "b" }, { "label": "X", "value": "x" }],
            "defaultValue": ["x"],
        }),
        json!({ "options": [], "defaultValue": null }),
    ]
}

#[test]
fn an_update_leaves_what_the_engine_leaves() {
    let (mut compared, mut taken) = (0, 0);
    for field in fields() {
        for patch in patches() {
            if field["family"] == "text" && patch["defaultValue"].is_array() {
                continue;
            }
            let ours = engine::form_field_after(
                &FieldWithWidgets { field: field.clone(), widgets: vec![] },
                &patch,
            )
            .map(|after| after.field);
            let typed: FormFieldPatch =
                serde_json::from_value(patch.clone()).unwrap_or_else(|e| panic!("{e}: {patch}"));
            let current: FormFieldDto = serde_json::from_value(field.clone()).unwrap();
            let theirs = form_field_after(&current, &typed);
            let what = format!("{field} {patch}");
            taken += usize::from(agree(&what, ours, theirs, |t| serde_json::to_value(t).unwrap()));
            compared += 1;
        }
    }
    assert_eq!(compared, 8 * 14 - 2 * 4);
    assert!(taken >= 40, "too few updates taken to compare: {taken}");
}

/// Drafts as the TypeScript types them: one string for a text field's or a
/// dropdown's default, a list for a list box's.
fn drafts() -> Vec<Value> {
    let named = |family: &str, members: Value| {
        let mut draft = json!({ "family": family, "name": "d" });
        for (key, value) in members.as_object().unwrap() {
            draft[key] = value.clone();
        }
        draft
    };
    let options = json!([
        { "label": "A", "value": "a" },
        { "label": "B", "value": "b" },
        { "label": "C", "value": "c" },
    ]);
    vec![
        named("text", json!({})),
        named("text", json!({ "defaultValue": "hello" })),
        named("checkbox", json!({ "defaultValue": "Yes" })),
        named("radio", json!({})),
        named("pushbutton", json!({})),
        named("signature", json!({})),
        named("combobox", json!({ "options": options })),
        named("combobox", json!({ "options": options, "defaultValue": "b" })),
        named("combobox", json!({ "options": options, "defaultValue": "z" })),
        named("combobox", json!({ "options": options, "defaultValue": "z", "edit": true })),
        named("listbox", json!({ "options": options })),
        named(
            "listbox",
            json!({ "options": options, "defaultValue": ["c", "a"], "multiSelect": true }),
        ),
        named("listbox", json!({ "options": options, "defaultValue": ["c", "a"] })),
        named("listbox", json!({ "options": options, "defaultValue": ["b"] })),
        named("listbox", json!({ "options": options, "defaultValue": ["z"] })),
    ]
}

#[test]
fn a_draft_default_is_the_engine_default() {
    let mut taken = 0;
    for draft in drafts() {
        let ours = engine::default_entry_of_draft(&draft);
        let typed: FormFieldDraft<PageCoordinates> =
            serde_json::from_value(draft.clone()).unwrap_or_else(|e| panic!("{e}: {draft}"));
        let theirs = default_entry_of_draft(&typed);
        taken += usize::from(agree(&draft.to_string(), ours, theirs, |t| {
            serde_json::to_value(t).unwrap()
        }));
    }
    assert!(taken >= 10, "too few defaults taken to compare: {taken}");
}

#[test]
fn a_widget_takes_the_members_its_family_takes() {
    let families =
        ["text", "checkbox", "radio", "combobox", "listbox", "pushbutton", "signature", "unknown"];
    let values = [
        json!({ "rect": { "x": 1 } }),
        json!({ "caption": "Go", "fontSize": 12 }),
        json!({ "caption": null }),
        json!({ "checkStyle": "cross" }),
        json!({ "checkStyle": null, "caption": null }),
        json!({ "caption": "Go", "checkStyle": "check" }),
    ];
    let mut taken = 0;
    for family in families {
        for value in &values {
            let ours = engine::fit_widget_to_family(family, value);
            let typed: FormFieldFamily = serde_json::from_value(json!(family)).unwrap();
            let theirs = fit_widget_to_family(typed, value.as_object().unwrap().clone());
            taken += usize::from(agree(&format!("{family} {value}"), ours, theirs, Value::Object));
        }
    }
    assert!(taken >= 20, "too few widgets taken to compare: {taken}");
}

fn row(n: u32) -> Value {
    json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": 1 }, "objectNumber": n })
}

#[test]
fn a_reordered_list_is_the_engine_order() {
    let order: Vec<Value> = (1..=5).map(row).collect();
    let moves: Vec<Vec<Value>> = vec![
        vec![],
        vec![row(3)],
        vec![row(5), row(1)],
        vec![row(2), row(2)],
        vec![row(9)],
        vec![row(1), row(2), row(3), row(4), row(5)],
    ];
    let positions = [
        json!("start"),
        json!("end"),
        json!({ "before": row(1) }),
        json!({ "after": row(1) }),
        json!({ "before": row(3) }),
        json!({ "after": row(5) }),
        json!({ "after": row(9) }),
    ];
    let (mut compared, mut taken) = (0, 0);
    for moved in &moves {
        for position in &positions {
            let ours =
                engine::reordered_list(&order, moved, position, &annotation_key).map(Value::Array);
            let typed: ListPosition<Value> = serde_json::from_value(position.clone()).unwrap();
            let theirs = reordered_list(&order, moved, &typed, |r: &Value| annotation_key(r));
            let what = format!("{} to {position}", Value::Array(moved.clone()));
            taken += usize::from(agree(&what, ours, theirs, Value::Array));
            compared += 1;
        }
    }
    assert_eq!(compared, 6 * 7);
    assert!(taken >= 15, "too few orders taken to compare: {taken}");
}

#[test]
fn a_reordered_part_is_the_engine_part() {
    let list: Vec<u32> = (1..=8).collect();
    let parts: [&dyn Fn(&u32) -> bool; 3] = [&|n| n % 2 == 0, &|n| *n > 4, &|_| true];
    // Keys named once each, as `reordered_list` leaves them: it refuses a
    // row named twice. With a key named twice the TypeScript (and this copy)
    // places that item twice, where the native engine places it once.
    let orders: [&[u32]; 5] = [&[], &[8, 2], &[6, 4], &[9, 7, 2], &[1, 3, 5, 7]];
    for in_part in parts {
        for order in orders {
            let keys: Vec<String> = order.iter().map(u32::to_string).collect();
            let ours = engine::reorder_part(&list, in_part, &keys, &|n: &u32| n.to_string());
            let theirs = reorder_part(&list, in_part, order, |n: &u32| *n);
            assert_eq!(ours, theirs, "{order:?}");
        }
    }
}
