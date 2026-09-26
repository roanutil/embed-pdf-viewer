//! `scripting/overlay.ts`: the transaction overlay, a working copy of the
//! script field view every pass mutates, and the canonical effect list
//! diffed from it at the end (values, display, appearance text; resets
//! folded back to one effect).

use epdf_acrojs::ScriptFieldInput;
use epdf_kernel::{Error, undefined};
use serde_json::{Value, json};

use crate::value::{get, str_of, strict_eq, string_of};

pub(crate) struct Appearance {
    pub(crate) key: String,
    pub(crate) field_ref: Value,
    pub(crate) text: Value,
}

pub(crate) struct Overlay {
    pub(crate) original: Vec<ScriptFieldInput>,
    pub(crate) fields: Vec<ScriptFieldInput>,
    pub(crate) reset_keys: Vec<String>,
    pub(crate) appearances: Vec<Appearance>,
}

impl Overlay {
    pub(crate) fn add_reset(&mut self, key: String) {
        if !self.reset_keys.contains(&key) {
            self.reset_keys.push(key);
        }
    }

    pub(crate) fn remove_reset(&mut self, key: &str) {
        self.reset_keys.retain(|k| k != key);
    }
}

/// `refKey(ref)`.
pub fn ref_key(r: &Value) -> String {
    if str_of(r, "kind") == Some("objectNumber") {
        format!("obj:{}", string_of(get(r, "fieldObjectNumber")))
    } else {
        format!("fqn:{}", string_of(get(r, "name")))
    }
}

pub(crate) fn same_ref(left: &Value, right: &Value) -> bool {
    ref_key(left) == ref_key(right)
}

/// `sameValue(left, right)`: arrays element-wise with `===`, anything else
/// with `===`.
pub(crate) fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| strict_eq(Some(x), Some(y)))
        }
        _ => strict_eq(left, right),
    }
}

pub(crate) fn field_index(fields: &[ScriptFieldInput], r: &Value) -> Option<usize> {
    fields.iter().position(|field| same_ref(&field.field_ref, r))
}

pub(crate) fn snapshot_fields(snapshot: &Value) -> &[Value] {
    get(snapshot, "fields").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

pub(crate) fn snapshot_field<'a>(snapshot: &'a Value, r: &Value) -> Option<&'a Value> {
    snapshot_fields(snapshot)
        .iter()
        .find(|field| same_ref(get(field, "ref").unwrap_or(&Value::Null), r))
}

/// `scriptValueFromFormValue(field, value)`. `None` is `undefined`.
pub(crate) fn script_value_from_form_value(
    field: &ScriptFieldInput,
    value: &Value,
) -> Result<Option<Value>, Error> {
    let family = field.family.as_str();
    let kind = str_of(value, "type");
    if family == "text" && kind == Some("text") {
        return Ok(get(value, "value").cloned());
    }
    if (family == "checkbox" || family == "radio") && kind == Some("toggle") {
        return Ok(Some(match get(value, "state") {
            Some(state) if !state.is_null() => state.clone(),
            _ => json!("Off"),
        }));
    }
    let values = || get(value, "values").and_then(Value::as_array).cloned().unwrap_or_default();
    if family == "combobox" && kind == Some("choice") {
        return Ok(Some(values().into_iter().next().filter(|v| !v.is_null()).unwrap_or(json!(""))));
    }
    if family == "listbox" && kind == Some("choice") {
        return Ok(Some(Value::Array(values())));
    }
    Err(Error::new(format!(
        "Form value type '{}' does not match field family '{}'",
        string_of(get(value, "type")),
        field.family
    )))
}

/// `String(value ?? '')`.
pub(crate) fn string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) => string_of(Some(v)),
    }
}

/// `formValueFromScriptValue(field)`.
pub(crate) fn form_value_from_script_value(field: &ScriptFieldInput) -> Option<Value> {
    let value = field.value.as_ref().filter(|v| !epdf_kernel::is_undefined(v));
    match field.family.as_str() {
        "text" => Some(json!({ "type": "text", "value": string_or_empty(value) })),
        "checkbox" | "radio" => {
            let state = match value {
                None | Some(Value::Null) => Value::Null,
                Some(v) if string_of(Some(v)) == "Off" => Value::Null,
                Some(v) => json!(string_of(Some(v))),
            };
            Some(json!({ "type": "toggle", "state": state }))
        }
        "combobox" => {
            let values = match value {
                Some(Value::Array(items)) => items.iter().take(1).cloned().collect(),
                _ => vec![json!(string_or_empty(value))],
            };
            Some(json!({ "type": "choice", "values": values }))
        }
        "listbox" => {
            let values = match value {
                Some(Value::Array(items)) => items.clone(),
                _ => vec![json!(string_or_empty(value))],
            };
            Some(json!({ "type": "choice", "values": values }))
        }
        _ => None,
    }
}

/// `applyEffects(overlay, effects)`: fold script effects into the overlay.
pub(crate) fn apply_effects(overlay: &mut Overlay, effects: &[Value]) -> Result<(), Error> {
    for effect in effects {
        if str_of(effect, "kind") == Some("reset") {
            for r in get(effect, "refs").and_then(Value::as_array).into_iter().flatten() {
                let Some(i) = field_index(&overlay.fields, r) else { continue };
                let field = &mut overlay.fields[i];
                field.value = field.default_value.clone();
                overlay.add_reset(ref_key(r));
            }
            continue;
        }
        let r = get(effect, "ref").cloned().unwrap_or_else(undefined);
        let Some(i) = field_index(&overlay.fields, &r) else { continue };
        let key = ref_key(&r);
        match str_of(effect, "kind") {
            Some("setValue") => {
                let value = get(effect, "value").cloned().unwrap_or_else(undefined);
                overlay.fields[i].value = script_value_from_form_value(&overlay.fields[i], &value)?;
                overlay.remove_reset(&key);
            }
            Some("setDisplay") => {
                overlay.fields[i].display = string_of(get(effect, "display"));
            }
            _ => {
                let text = get(effect, "text").cloned().unwrap_or_else(undefined);
                match overlay.appearances.iter_mut().find(|a| a.key == key) {
                    Some(existing) => {
                        existing.field_ref = r;
                        existing.text = text;
                    }
                    None => overlay.appearances.push(Appearance { key, field_ref: r, text }),
                }
            }
        }
    }
    Ok(())
}

/// `canonicalEffects(overlay)`: the overlay's difference from the original,
/// resets first, appearance text last.
pub(crate) fn canonical_effects(overlay: &Overlay) -> Vec<Value> {
    let mut effects = Vec::new();
    let mut resets = Vec::new();
    for field in &overlay.fields {
        let Some(i) = field_index(&overlay.original, &field.field_ref) else { continue };
        let original = &overlay.original[i];
        if !same_value(original.value.as_ref(), field.value.as_ref()) {
            let key = ref_key(&field.field_ref);
            if overlay.reset_keys.contains(&key)
                && same_value(field.value.as_ref(), field.default_value.as_ref())
            {
                resets.push(field.field_ref.clone());
            } else if let Some(value) = form_value_from_script_value(field) {
                effects.push(json!({ "kind": "setValue", "ref": field.field_ref, "value": value }));
            }
        }
        if original.display != field.display {
            effects.push(
                json!({ "kind": "setDisplay", "ref": field.field_ref, "display": field.display }),
            );
        }
    }
    if !resets.is_empty() {
        effects.insert(0, json!({ "kind": "reset", "refs": resets }));
    }
    for a in &overlay.appearances {
        effects.push(json!({ "kind": "setAppearanceText", "ref": a.field_ref, "text": a.text }));
    }
    effects
}
