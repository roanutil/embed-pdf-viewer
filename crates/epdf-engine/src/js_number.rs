//! JavaScript's number coercions, for the wire values the TypeScript worker
//! passes to the WASM runtime unchecked.
//!
//! A worker request field the TypeScript never validates reaches a WebAssembly
//! `i32` parameter as whatever JavaScript value it was. `createWasmFunctions`
//! (`engine/runtime/src/wasm/wasm-runtime.ts`) hands a `number` slot through
//! untouched, Emscripten's `cwrap` returns the raw export for an all-numeric
//! signature, and the WebAssembly JS API converts the argument with
//! `ToInt32(ToNumber(v))`. These are those two operations, over the values
//! JSON can carry. The Rust mirrors the WASM runtime only: the Node native
//! addon reads an `int32_t` with `napi_get_value_int32` and ignores its status
//! (`engine/runtime/build/generate-napi-binding.mjs:65`), so a non-number
//! reaches it as an indeterminate value.

use serde_json::Value;

/// ECMAScript `ToNumber`, for a JSON value. `None` is `undefined`.
pub(crate) fn to_number(value: Option<&Value>) -> f64 {
    let Some(value) = value else { return f64::NAN };
    match value {
        Value::Null => 0.0,
        Value::Bool(flag) => f64::from(u8::from(*flag)),
        Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
        Value::String(text) => string_to_number(text),
        // ToPrimitive of an array is its `join(",")`. Two or more elements
        // always put a comma in it, which no numeric literal has.
        Value::Array(items) => match items.as_slice() {
            [] => 0.0,
            // An element's ToString, then ToNumber of that. For every kind
            // but a boolean that is the element's own ToNumber: a number's
            // ToString round-trips, null and an empty array join as "".
            [Value::Bool(_)] => f64::NAN,
            [item] => to_number(Some(item)),
            _ => f64::NAN,
        },
        // "[object Object]".
        Value::Object(_) => f64::NAN,
    }
}

/// ECMAScript `ToInt32`: truncate, then wrap modulo 2^32. NaN and the
/// infinities are 0.
pub(crate) fn to_int32(number: f64) -> i32 {
    if !number.is_finite() {
        return 0;
    }
    // `rem_euclid` on an integral f64 is exact, and the result is below 2^32.
    number.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

/// ECMAScript `StringToNumber`: `StringNumericLiteral`, trimmed of
/// JavaScript's white space and line terminators, else NaN.
fn string_to_number(text: &str) -> f64 {
    let text = text.trim_matches(is_js_space);
    if text.is_empty() {
        return 0.0;
    }
    if let Some(value) = non_decimal(text) {
        return value;
    }
    let (sign, unsigned) = match text.as_bytes()[0] {
        b'+' => (1.0, &text[1..]),
        b'-' => (-1.0, &text[1..]),
        _ => (1.0, text),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_unsigned_decimal(unsigned) {
        return f64::NAN;
    }
    // Rust's parse rounds to nearest, as ToNumber does, and accepts every
    // form `is_unsigned_decimal` lets through.
    unsigned.parse::<f64>().map_or(f64::NAN, |value| sign * value)
}

/// `0x`, `0o` and `0b` literals, which take no sign. `None` for anything
/// else.
fn non_decimal(text: &str) -> Option<f64> {
    let radix = match text.get(..2)? {
        "0x" | "0X" => 16,
        "0o" | "0O" => 8,
        "0b" | "0B" => 2,
        _ => return None,
    };
    let digits = &text[2..];
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return Some(f64::NAN);
    }
    // Exact while it fits in a u128, whose conversion rounds to nearest as
    // ToNumber does. Past 2^128 the rest is accumulated in f64, which can
    // round differently in the last place; ToInt32 of anything that large is
    // 0 either way, so only the ordering of two such tiers could differ.
    let mut exact: u128 = 0;
    let mut chars = digits.chars();
    while let Some(c) = chars.next() {
        let digit = c.to_digit(radix).expect("checked above");
        match exact.checked_mul(u128::from(radix)).and_then(|v| v.checked_add(u128::from(digit))) {
            Some(next) => exact = next,
            None => {
                let mut approximate = exact as f64 * f64::from(radix) + f64::from(digit);
                for c in chars.by_ref() {
                    let digit = c.to_digit(radix).expect("checked above");
                    approximate = approximate * f64::from(radix) + f64::from(digit);
                }
                return Some(approximate);
            }
        }
    }
    Some(exact as f64)
}

/// `StrUnsignedDecimalLiteral` without `Infinity`: digits with an optional
/// fraction, or a fraction alone, then an optional exponent. No separators.
fn is_unsigned_decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = 0;
    let whole = count_digits(&bytes[at..]);
    at += whole;
    let mut fraction = 0;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        fraction = count_digits(&bytes[at..]);
        at += fraction;
    }
    if whole == 0 && fraction == 0 {
        return false;
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        let exponent = count_digits(&bytes[at..]);
        if exponent == 0 {
            return false;
        }
        at += exponent;
    }
    at == bytes.len()
}

fn count_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// JavaScript's `WhiteSpace` and `LineTerminator`: not Rust's
/// `char::is_whitespace`, which counts U+0085 and misses U+FEFF.
fn is_js_space(c: char) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn int32(value: Value) -> i32 {
        to_int32(to_number(Some(&value)))
    }

    #[test]
    fn numbers_truncate_and_wrap_as_to_int32_does() {
        assert_eq!(int32(json!(1.7)), 1);
        assert_eq!(int32(json!(-1.7)), -1);
        assert_eq!(int32(json!(-0.5)), 0);
        assert_eq!(int32(json!(4_294_967_297u64)), 1);
        assert_eq!(int32(json!(2_147_483_648u64)), -2_147_483_648);
        assert_eq!(int32(json!(-4_294_967_297i64)), -1);
        assert_eq!(int32(json!(1e300)), 0);
    }

    #[test]
    fn absent_null_and_booleans_coerce_as_to_number_does() {
        assert!(to_number(None).is_nan());
        assert_eq!(to_int32(to_number(None)), 0);
        assert_eq!(int32(json!(null)), 0);
        assert_eq!(int32(json!(true)), 1);
        assert_eq!(int32(json!(false)), 0);
    }

    #[test]
    fn strings_parse_as_string_to_number_does() {
        assert_eq!(int32(json!("1")), 1);
        assert_eq!(int32(json!(" \u{FEFF}2\n")), 2);
        assert_eq!(int32(json!("")), 0);
        assert_eq!(int32(json!("1.9")), 1);
        assert_eq!(int32(json!("1e1")), 10);
        assert_eq!(int32(json!(".5e1")), 5);
        assert_eq!(int32(json!("0x11")), 17);
        assert_eq!(int32(json!("0b11")), 3);
        assert_eq!(int32(json!("0o11")), 9);
        assert_eq!(int32(json!("4294967297")), 1);
        for nan in ["own", "1_000", "-0x1", "inf", "NaN", "1e", "\u{0085}1", "1.2.3", "."] {
            assert!(to_number(Some(&json!(nan))).is_nan(), "{nan:?}");
            assert_eq!(int32(json!(nan)), 0, "{nan:?}");
        }
        assert_eq!(to_number(Some(&json!("-Infinity"))), f64::NEG_INFINITY);
        assert_eq!(int32(json!("Infinity")), 0);
    }

    #[test]
    fn arrays_and_objects_coerce_through_their_string_form() {
        assert_eq!(int32(json!([])), 0);
        assert_eq!(int32(json!([7.9])), 7);
        assert_eq!(int32(json!(["3"])), 3);
        assert_eq!(int32(json!([[5]])), 5);
        assert_eq!(int32(json!([null])), 0);
        assert!(to_number(Some(&json!([true]))).is_nan());
        assert!(to_number(Some(&json!([1, 2]))).is_nan());
        assert!(to_number(Some(&json!({ "tier": 1 }))).is_nan());
    }
}
