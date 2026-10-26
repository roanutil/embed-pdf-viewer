//! `scripting/overlay.ts`: the transaction overlay, a working copy of the
//! script field view that every pass mutates, and the canonical effect list
//! diffed from it at the end (values, display, appearance text; resets
//! folded back to one effect).

use std::collections::HashSet;

use epdf_acrojs::ScriptFieldInput;
use epdf_kernel::Error;
use indexmap::IndexMap;
use serde_json::{Value, json};

use crate::value::{get, str_of, strict_eq, string_of};

/// `Overlay`.
pub struct Overlay {
    pub original: Vec<ScriptFieldInput>,
    pub fields: Vec<ScriptFieldInput>,
    pub reset_keys: HashSet<String>,
    /// By ref key: the ref the last write named, and its text.
    pub appearances: IndexMap<String, (Value, Value)>,
}

/// `refKey(ref)`.
pub fn ref_key(reference: &Value) -> String {
    crate::engine::field_key_of_ref(reference)
}

/// `sameRef(left, right)`.
pub fn same_ref(left: &Value, right: &Value) -> bool {
    ref_key(left) == ref_key(right)
}

/// `sameValue(left, right)`: arrays element-wise with `===`, anything else
/// with `===`.
pub fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    match (left, right) {
        (Some(Value::Array(a)), Some(Value::Array(b))) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| strict_eq(Some(x), Some(y)))
        }
        _ => strict_eq(left, right),
    }
}

/// `fieldByRef(fields, ref)`, as a position.
pub fn field_index(fields: &[ScriptFieldInput], reference: &Value) -> Option<usize> {
    fields.iter().position(|field| same_ref(&field.field_ref, reference))
}

/// `snapshotField(snapshot, ref)`: by its own ref, or by full name for a
/// name ref.
pub fn snapshot_field<'a>(snapshot: &'a Value, reference: &Value) -> Option<&'a Value> {
    let by_name = str_of(reference, "kind") == Some("fqn");
    crate::value::items(snapshot, "fields").iter().find(|field| {
        same_ref(get(field, "ref").unwrap_or(&Value::Null), reference)
            || (by_name && strict_eq(get(field, "name"), get(reference, "name")))
    })
}

fn has(value: &Value, key: &str) -> bool {
    value.as_object().is_some_and(|o| o.contains_key(key))
}

/// `value ?? fallback`.
fn or(value: Option<&Value>, fallback: &str) -> Value {
    match value {
        None | Some(Value::Null) => json!(fallback),
        Some(v) if epdf_kernel::is_undefined(v) => json!(fallback),
        Some(v) => v.clone(),
    }
}

/// `scriptValueFromFormValue(field, value)`: a written value as scripts see
/// it, a toggle's export value (`'Off'` when clear).
pub fn script_value_from_form_value(
    field: &ScriptFieldInput,
    value: &Value,
) -> Result<Value, Error> {
    match field.family.as_str() {
        "text" | "combobox" if has(value, "value") => return Ok(or(get(value, "value"), "")),
        "radio" if has(value, "value") => return Ok(or(get(value, "value"), "Off")),
        "checkbox" => {
            if has(value, "value") {
                return Ok(or(get(value, "value"), "Off"));
            }
            if has(value, "checked") {
                if !crate::value::truthy(get(value, "checked")) {
                    return Ok(json!("Off"));
                }
                let Some(first) = field.export_values.as_ref().and_then(|v| v.first()) else {
                    return Err(Error::new(format!(
                        "Form field '{}' has no widget to check",
                        field.name
                    )));
                };
                return Ok(json!(first));
            }
        }
        "listbox" if has(value, "selectedValues") => {
            return Ok(get(value, "selectedValues").cloned().unwrap_or(Value::Null));
        }
        _ => {}
    }
    Err(Error::new(format!(
        "Form field '{}' ({}) does not take this value",
        field.name, field.family
    )))
}

/// `String(value ?? '')`.
pub fn string_or_empty(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(v) if epdf_kernel::is_undefined(v) => String::new(),
        Some(v) => string_of(Some(v)),
    }
}

/// `formValueFromScriptValue(field)`: a script value as the engine writes
/// it, or none for a family that holds none.
pub fn form_value_from_script_value(field: &ScriptFieldInput) -> Option<Value> {
    let value = field.value.as_ref().filter(|v| !epdf_kernel::is_undefined(v));
    match field.family.as_str() {
        "text" => Some(json!({ "value": string_or_empty(value) })),
        "checkbox" | "radio" => {
            // `value === null || String(value) === 'Off'`: an `undefined`
            // value writes the text "undefined", as `String` spells it.
            let clear = matches!(value, Some(Value::Null)) || string_of(value) == "Off";
            Some(json!({ "value": if clear { Value::Null } else { json!(string_of(value)) } }))
        }
        "combobox" => {
            let selected = match value {
                Some(Value::Array(items)) => items.first(),
                other => other,
            };
            let text = match selected {
                None | Some(Value::Null) => String::new(),
                Some(v) => string_of(Some(v)),
            };
            Some(json!({ "value": if text.is_empty() { Value::Null } else { json!(text) } }))
        }
        "listbox" => {
            let values = match value {
                Some(Value::Array(items)) => Value::Array(items.clone()),
                Some(Value::Null) => json!([]),
                Some(Value::String(s)) if s.is_empty() => json!([]),
                other => json!([string_of(other)]),
            };
            Some(json!({ "selectedValues": values }))
        }
        _ => None,
    }
}

/// `applyEffects(overlay, effects)`: fold script effects into the overlay.
pub fn apply_effects(overlay: &mut Overlay, effects: &[Value]) -> Result<(), Error> {
    for effect in effects {
        if str_of(effect, "kind") == Some("reset") {
            for reference in crate::value::items(effect, "refs") {
                let Some(i) = field_index(&overlay.fields, reference) else { continue };
                let field = &mut overlay.fields[i];
                field.value = field.default_value.clone();
                overlay.reset_keys.insert(ref_key(reference));
            }
            continue;
        }
        let reference = get(effect, "ref").cloned().unwrap_or(Value::Null);
        let Some(i) = field_index(&overlay.fields, &reference) else { continue };
        let key = ref_key(&reference);
        match str_of(effect, "kind") {
            Some("setValue") => {
                let value = get(effect, "value").cloned().unwrap_or(Value::Null);
                overlay.fields[i].value =
                    Some(script_value_from_form_value(&overlay.fields[i], &value)?);
                overlay.reset_keys.remove(&key);
            }
            Some("setDisplay") => {
                overlay.fields[i].display = string_of(get(effect, "display"));
            }
            _ => {
                let text = get(effect, "text").cloned().unwrap_or(Value::Null);
                overlay.appearances.insert(key, (reference, text));
            }
        }
    }
    Ok(())
}

/// `canonicalEffects(overlay)`: the overlay's difference from the original,
/// resets first, appearance text last.
pub fn canonical_effects(overlay: &Overlay) -> Vec<Value> {
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
    for (reference, text) in overlay.appearances.values() {
        effects.push(json!({ "kind": "setAppearanceText", "ref": reference, "text": text }));
    }
    effects
}
