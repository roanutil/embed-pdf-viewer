//! `read/property-values.ts`: the values a style panel shows for a list of
//! properties. Three read differently: the border's value is `borderStyle`,
//! `dashArray` and `cloudyIntensity`; a text's font, size, colour,
//! alignment and formats read as the text shows them; and the link is an
//! attached link's target, not a field.

use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::TextStyle;
use serde_json::{Map, Value, json};

use crate::js::Object;

const TEXT_KEYS: [&str; 7] =
    ["fontFamily", "fontSize", "fontColor", "textAlign", "bold", "italic", "underline"];
const FORMATS: [&str; 3] = ["bold", "italic", "underline"];

/// What a panel reads beside the engine fields.
pub struct PropertySources<'a> {
    pub data: &'a Object,
    pub text: Option<&'a TextStyle>,
    pub link: Option<Value>,
}

fn nullish_or(value: Option<&Value>, fallback: Value) -> Value {
    match value {
        None | Some(Value::Null) => fallback,
        Some(v) if epdf_kernel::is_undefined(v) => fallback,
        Some(v) => v.clone(),
    }
}

/// `propertyValue(spec, sources)`: one property's value, by its key; `None` is `undefined`.
pub fn property_value(spec: &Value, sources: &PropertySources) -> Option<Value> {
    let key = spec.get("key").and_then(Value::as_str).unwrap_or("");
    if key == "link" {
        return Some(nullish_or(sources.link.as_ref(), Value::Null));
    }
    if key == "borderStyle" {
        return Some(json!({
            "borderStyle": nullish_or(sources.data.get("borderStyle"), json!("solid")),
            "dashArray": nullish_or(sources.data.get("dashArray"), Value::Null),
            "cloudyIntensity": nullish_or(sources.data.get("cloudyIntensity"), Value::Null),
        }));
    }
    if let Some(text) = sources.text
        && TEXT_KEYS.contains(&key)
    {
        let printed = text.json();
        let own =
            printed.get(key).filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v)).cloned();
        return own.or_else(|| FORMATS.contains(&key).then_some(Value::Bool(false)));
    }
    sources.data.get(key).filter(|v| !epdf_kernel::is_undefined(v)).cloned()
}

/// `propertyValues(properties, sources)`: every property's value, by key;
/// the border spreads into its three fields.
pub fn property_values(properties: &[Value], sources: &PropertySources) -> Value {
    let mut values = Map::new();
    for spec in properties {
        let key = spec.get("key").and_then(Value::as_str).unwrap_or("").to_string();
        let value = property_value(spec, sources);
        if key == "borderStyle" {
            if let Some(Value::Object(border)) = value {
                for (k, v) in border {
                    values.insert(k, v);
                }
            }
        } else {
            values.insert(key, value.unwrap_or_else(epdf_kernel::undefined));
        }
    }
    Value::Object(values)
}
