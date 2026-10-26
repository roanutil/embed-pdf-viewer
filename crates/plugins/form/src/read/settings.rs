//! `read/settings.ts`: a field's settings, described for a settings panel
//! the way an annotation kind describes its look: which settings the
//! field's family has, how each is edited, and its value. `update()`
//! writes them.

use serde_json::{Map, Value, json};

use crate::model::{canonical_key, field_by_key};
use crate::value::{get, items, str_of};

fn setting(key: &str, control: &str, label: &str, label_key: &str) -> Value {
    json!({ "key": key, "control": control, "label": label, "labelKey": label_key })
}

/// What every field has.
fn common() -> Vec<Value> {
    vec![
        setting("name", "text", "Name", "form.settings.name"),
        setting("alternateName", "text", "Tooltip", "form.settings.tooltip"),
        setting("required", "flag", "Required", "form.settings.required"),
        setting("readOnly", "flag", "Read-only", "form.settings.readOnly"),
        setting("noExport", "flag", "Don't export", "form.settings.noExport"),
    ]
}

fn options_setting() -> Value {
    setting("options", "options", "Options", "form.settings.options")
}

fn option_values(field: &Value) -> Vec<Value> {
    items(field, "options")
        .iter()
        .map(|o| get(o, "value").cloned().unwrap_or(Value::Null))
        .collect()
}

/// What each family adds.
fn family_settings(field: &Value) -> Vec<Value> {
    match str_of(field, "family") {
        Some("text") => vec![
            setting("defaultValue", "text", "Default value", "form.settings.defaultValue"),
            json!({
                "key": "maxLength", "control": "number", "label": "Maximum length",
                "labelKey": "form.settings.maxLength", "min": 1,
            }),
            setting("multiline", "flag", "Several lines", "form.settings.multiline"),
            setting("password", "flag", "Password", "form.settings.password"),
            setting("comb", "flag", "One character per box", "form.settings.comb"),
        ],
        Some("radio") => vec![
            setting(
                "radiosInUnison",
                "flag",
                "Buttons with one value turn on together",
                "form.settings.radiosInUnison",
            ),
            setting("noToggleToOff", "flag", "Keep one on", "form.settings.keepOneOn"),
        ],
        Some("combobox") => vec![
            options_setting(),
            setting("edit", "flag", "Allow typing", "form.settings.allowTyping"),
            json!({
                "key": "defaultValue", "control": "choice", "label": "Default value",
                "labelKey": "form.settings.defaultValue", "options": option_values(field),
                "free": get(field, "edit").cloned().unwrap_or(Value::Null),
            }),
        ],
        Some("listbox") => vec![
            options_setting(),
            setting("multiSelect", "flag", "Several choices", "form.settings.multiSelect"),
            json!({
                "key": "defaultValue", "control": "choices", "label": "Default value",
                "labelKey": "form.settings.defaultValue", "options": option_values(field),
            }),
        ],
        _ => Vec::new(),
    }
}

/// `valuesOf(field, settings)`: each setting's value, as the field holds it.
fn values_of(field: &Value, settings: &[Value]) -> Value {
    let mut values = Map::new();
    for setting in settings {
        let key = str_of(setting, "key").unwrap_or("").to_string();
        let value = match key.as_str() {
            "name" => {
                let name = crate::value::string_of(get(field, "name"));
                json!(name.rsplit('.').next().unwrap_or(&name))
            }
            "alternateName" => match get(field, "alternateName") {
                Some(v) if !v.is_null() => v.clone(),
                _ => json!(""),
            },
            other => match get(field, other) {
                Some(v) if !v.is_null() => v.clone(),
                _ => Value::Null,
            },
        };
        values.insert(key, value);
    }
    Value::Object(values)
}

impl crate::controller::Core {
    /// `getFieldSettings(ref)`: a field's settings for a settings panel, or
    /// `None` for a field the form doesn't have.
    pub fn get_field_settings(&self, reference: &Value) -> Option<Value> {
        let index = self.view();
        let field = field_by_key(&index, &canonical_key(&index, reference))?;
        let mut properties = common();
        properties.extend(family_settings(field));
        let values = values_of(field, &properties);
        Some(json!({ "properties": properties, "values": values }))
    }
}
