//! JavaScript semantics the TypeScript leans on, over JSON values.

use serde_json::Value;

/// A member as JavaScript reads it: `None` (an absent member) and the
/// `$undefined` marker are both `undefined`.
fn defined(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !epdf_kernel::is_undefined(v))
}

/// `a === b` over two members: `undefined` equals only `undefined`, numbers
/// compare by value (NaN never), strings, booleans and `null` by value, and
/// two objects or arrays never (a JSON copy has no identity to share).
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (defined(a), defined(b)) {
        (None, None) => true,
        (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        (Some(Value::String(a)), Some(Value::String(b))) => a == b,
        _ => false,
    }
}

/// JavaScript truthiness of a JSON value.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => !epdf_kernel::is_undefined(value),
    }
}

/// `${value}` in a template literal.
pub fn template(value: Option<&Value>) -> String {
    match defined(value) {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => epdf_kernel::js::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                if v.is_null() || epdf_kernel::is_undefined(v) {
                    String::new()
                } else {
                    template(Some(v))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}
