//! `input.ts`: the VM's detached field view, built from the engine's lossless
//! form snapshot.

use epdf_kernel::Error;
use serde_json::{Value, json};

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

fn field_value_from_entry(family: &str, entry: &Value) -> Option<ScriptValue> {
    if (family == "checkbox" || family == "radio")
        && entry.get("kind").and_then(Value::as_str) == Some("none")
    {
        return Some(json!("Off"));
    }
    value_from_entry(entry)
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
            let flag = |key: &str| {
                field
                    .pointer(&format!("/flags/{key}"))
                    .and_then(Value::as_bool)
                    .ok_or_else(|| malformed(key))
            };
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
            Ok(ScriptFieldInput {
                field_ref: field.get("ref").cloned().ok_or_else(|| malformed("ref"))?,
                name: text("name")?,
                value: field_value_from_entry(
                    &family,
                    field.get("valueEntry").unwrap_or(&Value::Null),
                ),
                default_value: field_value_from_entry(
                    &family,
                    field.get("defaultValueEntry").unwrap_or(&Value::Null),
                ),
                family,
                // Form DTOs do not yet aggregate widget visibility.
                display: "visible".into(),
                read_only: flag("readOnly")?,
                required: flag("required")?,
                options,
            })
        })
        .collect()
}
