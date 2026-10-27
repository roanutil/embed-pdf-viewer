//! `input.ts`: the VM's detached field view, built from the engine's lossless
//! form snapshot.

use std::collections::HashMap;

use epdf_kernel::Error;
use serde_json::{Value, json};

use crate::actions::truthy;
use crate::types::{FormSnapshot, ScriptFieldInput, ScriptFieldOption, ScriptValue};

/// `valueFromEntry`: `None` for an entry kind the switch does not know, where
/// the TypeScript returns `undefined`.
fn value_from_entry(entry: &Value) -> Option<ScriptValue> {
    match entry.get("kind").and_then(Value::as_str) {
        Some("scalar") => Some(entry.get("value").cloned().unwrap_or(Value::Null)),
        Some("array") => Some(entry.get("values").cloned().unwrap_or_else(|| json!([]))),
        Some("none" | "unsupported") => Some(Value::Null),
        _ => None,
    }
}

fn is_toggle(family: &str) -> bool {
    family == "checkbox" || family == "radio"
}

/// `fieldValueFromEntry`: a checkbox or radio group shows the export value of
/// its checked widget, as Acrobat's `field.value` does, or `'Off'`.
fn field_value_from_entry(family: &str, widgets: &[Value], entry: &Value) -> Option<ScriptValue> {
    if !is_toggle(family) {
        return value_from_entry(entry);
    }
    if entry.get("kind").and_then(Value::as_str) != Some("scalar") {
        return Some(json!("Off"));
    }
    let value = entry.get("value").cloned().unwrap_or(Value::Null);
    // `widgets.find((widget) => widget.onState === entry.value)`, then
    // `checked?.exportValue ?? entry.value`.
    let export = widgets
        .iter()
        .find(|widget| widget.get("onState").is_some_and(|on| *on == value))
        .and_then(|widget| widget.get("exportValue"))
        .filter(|export| !export.is_null());
    Some(export.cloned().unwrap_or(value))
}

/// The `Map` key `scriptFieldsFromSnapshot` files a widget row under: its
/// object number, or 0 for a ref of another kind. `-0` and `0` are one key,
/// as `Map` compares with SameValueZero.
fn row_key(number: f64) -> u64 {
    if number == 0.0 { 0f64.to_bits() } else { number.to_bits() }
}

/// `displayOf`: how a widget shows, as Acrobat's `field.display` reads its
/// flags. A field with no row shows as visible.
fn display_of(row: Option<&Value>) -> &'static str {
    let Some(row) = row else { return "visible" };
    if truthy(row.get("hidden")) {
        "hidden"
    } else if truthy(row.get("noView")) {
        "noView"
    } else if truthy(row.get("print")) {
        "visible"
    } else {
        "noPrint"
    }
}

fn malformed(what: &str) -> Error {
    Error::named("TypeError", format!("[acrojs] malformed form field: {what}"))
}

/// Build the VM's field view from `doc.forms.list()`. The snapshot is the
/// engine's DTO; one without the members the TypeScript reads is refused.
pub fn script_fields_from_snapshot(
    snapshot: &FormSnapshot,
) -> Result<Vec<ScriptFieldInput>, Error> {
    let fields =
        snapshot.get("fields").and_then(Value::as_array).ok_or_else(|| malformed("fields"))?;
    let rows =
        snapshot.get("widgets").and_then(Value::as_array).ok_or_else(|| malformed("widgets"))?;
    // `new Map(rows)`: a later row under the same key replaces an earlier one.
    let mut by_number: HashMap<u64, &Value> = HashMap::new();
    for row in rows {
        let number = if row.pointer("/ref/kind").and_then(Value::as_str) == Some("objectNumber") {
            row.pointer("/ref/objectNumber").and_then(Value::as_f64).unwrap_or(f64::NAN)
        } else {
            0.0
        };
        by_number.insert(row_key(number), row);
    }
    fields
        .iter()
        .map(|field| {
            let text = |key: &str| {
                field
                    .get(key)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| malformed(key))
            };
            let flag =
                |key: &str| field.get(key).and_then(Value::as_bool).ok_or_else(|| malformed(key));
            let family = text("family")?;
            let options = match field.get("options") {
                None => None,
                Some(options) => Some(
                    options
                        .as_array()
                        .ok_or_else(|| malformed("options"))?
                        .iter()
                        .map(|option| {
                            Ok(ScriptFieldOption {
                                label: option
                                    .get("label")
                                    .and_then(Value::as_str)
                                    .ok_or_else(|| malformed("label"))?
                                    .into(),
                                value: option
                                    .get("value")
                                    .and_then(Value::as_str)
                                    .ok_or_else(|| malformed("value"))?
                                    .into(),
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?,
                ),
            };
            let widgets = field.get("widgets").and_then(Value::as_array);
            // Toggles read `field.widgets` unguarded, so a toggle without
            // them is refused; any other family only reads the first one.
            let toggle_widgets = if is_toggle(&family) {
                Some(widgets.ok_or_else(|| malformed("widgets"))?)
            } else {
                None
            };
            let widget_list = toggle_widgets.map_or(&[][..], Vec::as_slice);
            // A field shows as its first widget does.
            let first_number = widgets
                .and_then(|widgets| widgets.first())
                .and_then(|widget| widget.get("objectNumber"))
                .and_then(Value::as_f64)
                .unwrap_or(-1.0);
            let export_values = toggle_widgets
                .map(|widgets| {
                    widgets
                        .iter()
                        .map(|widget| {
                            widget
                                .get("exportValue")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .ok_or_else(|| malformed("exportValue"))
                        })
                        .collect::<Result<Vec<_>, Error>>()
                })
                .transpose()?;
            Ok(ScriptFieldInput {
                field_ref: field.get("ref").cloned().ok_or_else(|| malformed("ref"))?,
                name: text("name")?,
                value: field_value_from_entry(
                    &family,
                    widget_list,
                    field.get("valueEntry").unwrap_or(&Value::Null),
                ),
                default_value: field_value_from_entry(
                    &family,
                    widget_list,
                    field.get("defaultValueEntry").unwrap_or(&Value::Null),
                ),
                family,
                display: display_of(by_number.get(&row_key(first_number)).copied()).into(),
                read_only: flag("readOnly")?,
                required: flag("required")?,
                options,
                export_values,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(n: u32, extra: Value) -> Value {
        let mut widget = json!({ "objectNumber": n });
        if let (Some(widget), Value::Object(extra)) = (widget.as_object_mut(), extra) {
            widget.extend(extra);
        }
        widget
    }

    fn field(n: u32, name: &str, family: &str, widgets: Vec<Value>, over: Value) -> Value {
        let mut field = json!({
            "ref": { "kind": "objectNumber", "objectNumber": n },
            "name": name, "family": family, "readOnly": false, "required": false,
            "valueEntry": { "kind": "none" }, "defaultValueEntry": { "kind": "none" },
            "widgets": widgets,
        });
        if let (Some(field), Value::Object(over)) = (field.as_object_mut(), over) {
            field.extend(over);
        }
        field
    }

    fn row(n: u32, flags: Value) -> Value {
        let mut row = json!({ "ref": { "kind": "objectNumber", "objectNumber": n } });
        if let (Some(row), Value::Object(flags)) = (row.as_object_mut(), flags) {
            row.extend(flags);
        }
        row
    }

    /// The vector's snapshot has no widget rows, so `displayOf` and the
    /// toggle export lookup are pinned here. The expected answer is what
    /// `scriptFieldsFromSnapshot` (`packages/core/acrojs/src/input.ts`)
    /// returned for this same input under tsx.
    #[test]
    fn display_and_toggle_values_match_the_typescript() {
        let none = json!({});
        let snapshot = json!({
            "fields": [
                field(1, "hidden", "text", vec![widget(11, none.clone())], none.clone()),
                field(2, "noview", "text", vec![widget(12, none.clone())], none.clone()),
                field(3, "noprint", "text", vec![widget(13, none.clone())], none.clone()),
                field(4, "printed", "text", vec![widget(14, none.clone())], none.clone()),
                field(5, "hidden-wins", "text", vec![widget(15, none.clone())], none.clone()),
                field(6, "no-row", "text", vec![widget(99, none.clone())], none.clone()),
                field(7, "no-widgets", "text", vec![], none.clone()),
                field(8, "dup-last-wins", "text", vec![widget(18, none.clone())], none.clone()),
                field(9, "second-widget-ignored", "text",
                    vec![widget(19, none.clone()), widget(14, none.clone())], none.clone()),
                field(10, "fqn-row-key-0", "text", vec![widget(0, none.clone())], none.clone()),
                field(20, "radio-export", "radio", vec![
                    widget(21, json!({ "onState": "A", "exportValue": "Alpha" })),
                    widget(22, json!({ "onState": "B", "exportValue": "Beta" })),
                ], json!({
                    "valueEntry": { "kind": "scalar", "value": "B" },
                    "defaultValueEntry": { "kind": "scalar", "value": "Z" },
                })),
                field(30, "checkbox-array", "checkbox",
                    vec![widget(31, json!({ "onState": "On", "exportValue": "Yes" }))],
                    json!({ "valueEntry": { "kind": "array", "values": ["On"] } })),
                field(40, "checkbox-on", "checkbox",
                    vec![widget(41, json!({ "onState": "On", "exportValue": "Yes" }))],
                    json!({ "valueEntry": { "kind": "scalar", "value": "On" } })),
            ],
            "widgets": [
                row(11, json!({ "hidden": true, "print": true })),
                row(12, json!({ "noView": true, "print": true })),
                row(13, json!({ "print": false })),
                row(14, json!({ "print": true })),
                row(15, json!({ "hidden": true, "noView": true, "print": false })),
                row(18, json!({ "hidden": true })),
                row(18, json!({ "print": true })),
                { "ref": { "kind": "fqn", "name": "x" }, "hidden": true },
            ],
        });
        let got: Vec<(String, String, Value, Value)> = script_fields_from_snapshot(&snapshot)
            .unwrap()
            .into_iter()
            .map(|f| {
                (
                    f.name,
                    f.display,
                    f.value.unwrap_or(Value::Null),
                    f.export_values.map_or(Value::Null, |v| json!(v)),
                )
            })
            .collect();
        let expected = [
            ("hidden", "hidden", Value::Null, Value::Null),
            ("noview", "noView", Value::Null, Value::Null),
            ("noprint", "noPrint", Value::Null, Value::Null),
            ("printed", "visible", Value::Null, Value::Null),
            ("hidden-wins", "hidden", Value::Null, Value::Null),
            ("no-row", "visible", Value::Null, Value::Null),
            ("no-widgets", "visible", Value::Null, Value::Null),
            ("dup-last-wins", "visible", Value::Null, Value::Null),
            ("second-widget-ignored", "visible", Value::Null, Value::Null),
            ("fqn-row-key-0", "hidden", Value::Null, Value::Null),
            ("radio-export", "visible", json!("Beta"), json!(["Alpha", "Beta"])),
            ("checkbox-array", "visible", json!("Off"), json!(["Yes"])),
            ("checkbox-on", "visible", json!("Yes"), json!(["Yes"])),
        ]
        .map(|(n, d, v, e)| (n.to_string(), d.to_string(), v, e));
        assert_eq!(got, expected);
        let radio = &script_fields_from_snapshot(&snapshot).unwrap()[10];
        assert_eq!(radio.default_value, Some(json!("Z")), "no widget has onState Z");
    }

    #[test]
    fn a_checked_widget_without_an_export_value_shows_the_state() {
        let snapshot = json!({
            "fields": [field(1, "r", "radio", vec![widget(2, json!({ "onState": "On" }))],
                json!({ "valueEntry": { "kind": "scalar", "value": "On" } }))],
            "widgets": [],
        });
        // `checked?.exportValue ?? entry.value`; the field's own exportValues
        // read is refused, as the TypeScript would carry `undefined`.
        let error = script_fields_from_snapshot(&snapshot).unwrap_err();
        assert_eq!(error.message, "[acrojs] malformed form field: exportValue");
        assert_eq!(
            field_value_from_entry(
                "radio",
                &[widget(2, json!({ "onState": "On" }))],
                &json!({ "kind": "scalar", "value": "On" })
            ),
            Some(json!("On"))
        );
    }

    #[test]
    fn a_snapshot_without_widget_rows_or_toggle_widgets_is_refused() {
        let no_rows = json!({ "fields": [] });
        assert_eq!(
            script_fields_from_snapshot(&no_rows).unwrap_err().message,
            "[acrojs] malformed form field: widgets"
        );
        let toggle = json!({
            "fields": [{ "ref": {}, "name": "b", "family": "checkbox", "readOnly": false,
                "required": false, "valueEntry": { "kind": "none" } }],
            "widgets": [],
        });
        assert_eq!(
            script_fields_from_snapshot(&toggle).unwrap_err().message,
            "[acrojs] malformed form field: widgets"
        );
    }
}
