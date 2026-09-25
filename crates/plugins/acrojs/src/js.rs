//! JavaScript's semantics where the ported TypeScript leans on them: number
//! printing, `Math.round`, `Math.max`/`Math.min` with NaN, `String.prototype
//! .trim`, UTF-16 lengths, `String(x)` of a JSON value, and
//! `encodeURIComponent`.
//!
//! Each one differs from the obvious Rust spelling somewhere a vector can
//! reach: `f64::round` rounds half away from zero, `f64::max` drops NaN,
//! `str::trim` removes U+0085 and keeps U+FEFF, `str::len` counts bytes, and
//! `{}` never prints an exponent.

use serde_json::Value;

/// `Number.prototype.toString()` (radix 10): ECMA-262 Number::toString.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    // `{:e}` prints the shortest digits that round-trip, as ECMA-262 asks.
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i64;
    let n = exponent.parse::<i64>().unwrap_or(0) + 1;
    if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let sign = if n - 1 < 0 { '-' } else { '+' };
        let rest = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        format!("{}{rest}e{sign}{}", &digits[..1], (n - 1).abs())
    }
}

/// `Math.round`: half toward positive infinity.
pub fn round(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.max(a, b)`: NaN if either is.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.max(b) }
}

/// `Math.min(a, b)`: NaN if either is.
pub fn min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() { f64::NAN } else { a.min(b) }
}

/// ECMAScript WhiteSpace and LineTerminator: what `trim` removes.
pub fn is_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// A canonical number: a JSON number, or `{"$number": "NaN" | "Infinity" |
/// "-Infinity"}`, which is how a vector writes the values JSON cannot.
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Object(o) => match o.get("$number").and_then(Value::as_str) {
            Some("NaN") => Some(f64::NAN),
            Some("Infinity") => Some(f64::INFINITY),
            Some("-Infinity") => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// `typeof value === 'undefined'` for a canonical value.
pub fn is_undefined(value: &Value) -> bool {
    epdf_kernel::is_undefined(value)
}

/// `String(value)` for a canonical JSON value.
pub fn string_of(value: &Value) -> String {
    if let Some(x) = number(value) {
        return number_to_string(x);
    }
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => s.clone(),
        // `Array.prototype.join`: null and undefined elements print empty.
        Value::Array(items) => {
            items
                .iter()
                .map(|item| {
                    if item.is_null() || is_undefined(item) {
                        String::new()
                    } else {
                        string_of(item)
                    }
                })
                .collect::<Vec<_>>()
                .join(",")
        }
        Value::Object(_) if is_undefined(value) => "undefined".into(),
        _ => "[object Object]".into(),
    }
}

/// `encodeURIComponent`. A Rust string holds no lone surrogate, so the
/// TypeScript's `URIError` has no counterpart.
pub fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-_.!~*'()".contains(c) {
            out.push(c);
        } else {
            let mut buffer = [0u8; 4];
            for byte in c.encode_utf8(&mut buffer).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_numbers_as_javascript_does() {
        for (x, want) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (123456789.0, "123456789"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (1e20, "100000000000000000000"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
        ] {
            assert_eq!(number_to_string(x), want, "{x}");
        }
    }

    #[test]
    fn rounds_half_up() {
        assert_eq!(round(0.5), 1.0);
        assert_eq!(round(-0.5), 0.0);
        assert_eq!(round(127.5), 128.0);
        assert_eq!(round(0.49999999999999994), 0.0);
        assert_eq!(round(-2.5), -2.0);
        assert!(round(f64::NAN).is_nan());
    }

    #[test]
    fn trims_the_javascript_set() {
        assert_eq!(trim("\u{FEFF} a \u{00A0}"), "a");
        assert_eq!(trim("\u{0085}a"), "\u{0085}a");
    }

    #[test]
    fn coerces_json_as_string_does() {
        use serde_json::json;
        assert_eq!(string_of(&json!(null)), "null");
        assert_eq!(string_of(&json!(true)), "true");
        assert_eq!(string_of(&json!(1.5)), "1.5");
        assert_eq!(string_of(&json!({ "$number": "NaN" })), "NaN");
        assert_eq!(string_of(&json!({ "$undefined": true })), "undefined");
        assert_eq!(string_of(&json!({ "a": 1 })), "[object Object]");
        assert_eq!(string_of(&json!(["RGB"])), "RGB");
        assert_eq!(string_of(&json!([null, { "$undefined": true }, "x", ["y", 2]])), ",,x,y,2");
    }

    #[test]
    fn encodes_uri_components() {
        assert_eq!(encode_uri_component("a b&c=é!'()*~"), "a%20b%26c%3D%C3%A9!'()*~");
        assert_eq!(encode_uri_component("😀"), "%F0%9F%98%80");
    }
}
