//! Comparing canonical JSON (`tooling/plugin-conformance/src/canonical.ts`).
//!
//! Object member order is not compared; array order is. Numbers compare as
//! `f64`. Two integers below 2^53 in magnitude compare exactly. Anything else
//! compares with a relative tolerance of one part in 10^12. The tolerance is
//! there for transcendental functions: V8's `Math.sin` and the platform libm
//! may differ in the last bit, and that is not a difference in behaviour. It
//! is far below anything a coordinate or a zoom could carry, but not below an
//! epoch in milliseconds (about 2.5e12 scales it to 2.5 ms), which is why
//! integers are exempt: a timestamp off by one millisecond is a real
//! difference.

use serde_json::Value;

pub const RELATIVE_TOLERANCE: f64 = 1e-12;

/// 2^53: every integer below this in magnitude is exact in an `f64`, as in
/// JavaScript's `Number.isSafeInteger`.
const SAFE_INTEGER_BOUND: f64 = 9_007_199_254_740_992.0;

fn is_safe_integer(x: f64) -> bool {
    x.fract() == 0.0 && x.abs() < SAFE_INTEGER_BOUND
}

pub fn numbers_equal(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    // Counts, ids and epoch milliseconds: no rounding can make two of these
    // differ, so any difference is behaviour.
    if is_safe_integer(a) && is_safe_integer(b) {
        return false;
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

#[cfg(test)]
mod tests {
    use super::numbers_equal;

    #[test]
    fn integers_below_2_pow_53_compare_exactly() {
        // An epoch in milliseconds off by 2 ms used to pass the relative
        // tolerance (2.5e12 * 1e-12 = 2.5).
        assert!(!numbers_equal(2_559_009_906_002.0, 2_559_009_906_000.0));
        assert!(!numbers_equal(1_700_000_000_001.0, 1_700_000_000_000.0));
        assert!(numbers_equal(2_559_009_906_000.0, 2_559_009_906_000.0));
        assert!(!numbers_equal(-3.0, 3.0));
    }

    #[test]
    fn fractions_keep_the_relative_tolerance() {
        let x = 0.1_f64.sin();
        assert!(numbers_equal(x, f64::from_bits(x.to_bits() + 1)));
        assert!(numbers_equal(1.0, 1.0 + f64::EPSILON));
        assert!(!numbers_equal(1.0, 1.000_000_001));
    }
}
