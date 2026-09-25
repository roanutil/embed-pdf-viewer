//! Reading the engine's DTOs, which this port keeps as JSON, with
//! JavaScript's rules: a missing member is `undefined`, `String(x)` prints
//! numbers the way `Number#toString` does, and `===` compares numbers by
//! value and objects by identity.

use epdf_kernel::{is_undefined, undefined};
use serde_json::{Map, Value};

/// `v[key]`, with a JSON `undefined` marker read as absent.
pub fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key).filter(|m| !is_undefined(m))
}

/// `v[key]` as the value a JavaScript read yields: `undefined` when absent.
pub fn member(v: &Value, key: &str) -> Value {
    get(v, key).cloned().unwrap_or_else(undefined)
}

/// `v[key]` as a number, when it is one.
pub fn num(v: &Value, key: &str) -> Option<f64> {
    get(v, key).and_then(Value::as_f64)
}

/// `v[key]` as a number, `NaN` when absent or not a number (what arithmetic
/// on `undefined` gives).
pub fn f64_of(v: &Value, key: &str) -> f64 {
    num(v, key).unwrap_or(f64::NAN)
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    get(v, key).and_then(Value::as_str)
}

/// JavaScript truthiness (absent is `undefined`).
pub fn truthy(v: Option<&Value>) -> bool {
    epdf_acrojs::actions::truthy(v)
}

/// `String(x)`, with an absent value as `undefined`.
pub fn string_of(v: Option<&Value>) -> String {
    match v {
        None => "undefined".into(),
        Some(v) => epdf_acrojs::js::string_of(v),
    }
}

/// `a === b`. Objects and arrays are distinct references in a value read
/// from JSON, so they are never identical to another value.
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => match (a, b) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(x), Value::Bool(y)) => x == y,
            (Value::String(x), Value::String(y)) => x == y,
            _ => match (epdf_acrojs::js::number(a), epdf_acrojs::js::number(b)) {
                (Some(x), Some(y)) => x == y,
                _ => {
                    // Two undefined markers are the same value.
                    is_undefined(a) && is_undefined(b)
                }
            },
        },
        (Some(v), None) | (None, Some(v)) => is_undefined(v),
    }
}

/// An object from `(key, value)` members, dropping `undefined` ones as a
/// JavaScript object literal read back through JSON would.
pub fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
    let mut out = Map::new();
    for (key, value) in members {
        out.insert(key.to_string(), value);
    }
    Value::Object(out)
}

/// `JSON.stringify(value)` for a value with no `undefined` in it: numbers
/// print as `Number#toString` does, strings escape as JSON does.
pub fn json_stringify(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match n.as_f64() {
            Some(x) if x.is_finite() => out.push_str(&epdf_geometry::js::number_to_string(x)),
            _ => out.push_str("null"),
        },
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(members) => {
            out.push('{');
            let mut first = true;
            for (key, item) in members {
                if is_undefined(item) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                out.push_str(&serde_json::to_string(key).unwrap_or_default());
                out.push(':');
                write_json(item, out);
            }
            out.push('}');
        }
    }
}

/// `s.slice(start, end)` over UTF-16 code units, with `String#slice`'s
/// reading of negative, fractional and out-of-range offsets. `end` absent is
/// the string's length.
pub fn utf16_slice(s: &str, start: f64, end: Option<f64>) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let len = units.len() as f64;
    let clamp = |rel: f64| {
        let rel = if rel.is_nan() { 0.0 } else { rel.trunc() };
        if rel < 0.0 { (len + rel).max(0.0) } else { rel.min(len) }
    };
    let from = clamp(start) as usize;
    let to = clamp(end.unwrap_or(len)) as usize;
    if from >= to {
        return String::new();
    }
    String::from_utf16_lossy(&units[from..to])
}

/// A number as canonical JSON: the non-finite values as the conformance
/// format spells them.
pub fn number(x: f64) -> Value {
    epdf_kernel::js::number(x)
}

/// `Number(string)`.
pub fn string_to_number(s: &str) -> f64 {
    epdf_annotation_core::js::string_to_number(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn slices_utf16_like_javascript() {
        assert_eq!(utf16_slice("😀x", 0.0, Some(2.0)), "😀");
        assert_eq!(utf16_slice("abcd", -1.0, None), "d");
        assert_eq!(utf16_slice("abcd", 0.0, Some(-1.0)), "abc");
        assert_eq!(utf16_slice("abcd", 1.5, Some(2.9)), "b");
        assert_eq!(utf16_slice("abcd", 3.0, Some(1.0)), "");
        assert_eq!(utf16_slice("abcd", f64::NAN, Some(9.0)), "abcd");
    }

    #[test]
    fn stringifies_numbers_as_javascript() {
        assert_eq!(
            json_stringify(&json!({ "a": 1.0, "b": [0.5, "é"] })),
            r#"{"a":1,"b":[0.5,"é"]}"#
        );
    }

    #[test]
    fn strict_equality() {
        assert!(strict_eq(Some(&json!(1)), Some(&json!(1.0))));
        assert!(!strict_eq(Some(&json!([1])), Some(&json!([1]))));
        assert!(!strict_eq(Some(&json!(null)), None));
        assert!(strict_eq(None, Some(&undefined())));
    }
}
