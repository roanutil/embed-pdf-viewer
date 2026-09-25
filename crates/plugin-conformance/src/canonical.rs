//! Comparing canonical JSON (`tooling/plugin-conformance/src/canonical.ts`).
//!
//! Object member order is not compared; array order is. Numbers compare as
//! `f64`, with a relative tolerance of one part in 10^12. The tolerance is
//! there for transcendental functions: V8's `Math.sin` and the platform libm
//! may differ in the last bit, and that is not a difference in behaviour. It
//! is far below anything a coordinate, a zoom or a count could carry.

use serde_json::Value;

pub const RELATIVE_TOLERANCE: f64 = 1e-12;

pub fn numbers_equal(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let scale = a.abs().max(b.abs()).max(1.0);
    (a - b).abs() <= RELATIVE_TOLERANCE * scale
}

/// `None` when equal, else the path to the first difference and both sides.
pub fn diff(expected: &Value, actual: &Value) -> Option<String> {
    diff_at("$", expected, actual)
}

pub fn equal(expected: &Value, actual: &Value) -> bool {
    diff(expected, actual).is_none()
}

fn short(value: &Value) -> String {
    let text = value.to_string();
    if text.len() > 400 {
        format!("{}…", &text[..text.char_indices().nth(400).map_or(text.len(), |(i, _)| i)])
    } else {
        text
    }
}

fn diff_at(path: &str, expected: &Value, actual: &Value) -> Option<String> {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap_or(f64::NAN), b.as_f64().unwrap_or(f64::NAN));
            if numbers_equal(a, b) { None } else { Some(format!("{path}: expected {a}, got {b}")) }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                if let Some(d) = diff_at(&format!("{path}[{i}]"), x, y) {
                    return Some(d);
                }
            }
            if a.len() != b.len() {
                let extra = if a.len() > b.len() { &a[b.len()] } else { &b[a.len()] };
                return Some(format!(
                    "{path}: expected {} items, got {} (first unmatched: {})",
                    a.len(),
                    b.len(),
                    short(extra)
                ));
            }
            None
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, x) in a {
                match b.get(key) {
                    Some(y) => {
                        if let Some(d) = diff_at(&format!("{path}.{key}"), x, y) {
                            return Some(d);
                        }
                    }
                    None => return Some(format!("{path}.{key}: missing (expected {})", short(x))),
                }
            }
            for (key, y) in b {
                if !a.contains_key(key) {
                    return Some(format!("{path}.{key}: unexpected {}", short(y)));
                }
            }
            None
        }
        _ if expected == actual => None,
        _ => Some(format!("{path}: expected {}, got {}", short(expected), short(actual))),
    }
}

/// A text key for matching arguments against a table: numbers normalized so
/// `1` and `1.0` agree, members sorted.
pub fn key(value: &Value) -> String {
    fn norm(value: &Value) -> Value {
        match value {
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(0.0);
                if f.fract() == 0.0 && f.abs() < 9.0e15 {
                    Value::from(f as i64)
                } else {
                    Value::from(f)
                }
            }
            Value::Array(items) => Value::Array(items.iter().map(norm).collect()),
            // Sorted: with `preserve_order` a map prints in insertion order,
            // and two argument objects that differ only in member order are
            // the same arguments.
            Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().map(|(k, v)| (k.clone(), norm(v))).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(entries.into_iter().collect())
            }
            other => other.clone(),
        }
    }
    norm(value).to_string()
}
