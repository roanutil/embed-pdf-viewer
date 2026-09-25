//! The JavaScript built-ins this plugin's strings depend on, where Rust's
//! answer differs. The numeric ones (`Math.round`, `Math.max`, number
//! printing) come from [`epdf_geometry::js`]; these three are the ones only
//! this plugin needs.

use std::cmp::Ordering;

use epdf_kernel::Error;

pub use epdf_geometry::js::{max, min, number_to_string, round};

/// `Number.prototype.toFixed(digits)`: the exact binary value rounded at
/// `digits` places, a tie going to the larger magnitude (`(0.125).toFixed(2)`
/// is `0.13`, where Rust's `{:.2}` rounds half to even). At `1e21` and past
/// it, and for the non-finite values, it is `Number#toString`.
pub fn to_fixed(x: f64, digits: usize) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return number_to_string(x);
    }
    let negative = x < 0.0;
    // 1100 places is past the longest exact decimal expansion of any f64
    // (1074 fractional digits for the smallest subnormal), so this string is
    // the exact value and the rounding below is decided on exact digits.
    let exact = format!("{:.1100}", x.abs());
    let (int_part, frac_part) = exact.split_once('.').expect("fixed notation has a point");
    let mut kept: Vec<u8> = int_part.bytes().chain(frac_part.bytes().take(digits)).collect();
    let round_up = frac_part.as_bytes().get(digits).is_some_and(|d| *d >= b'5');
    if round_up {
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let int_len = kept.len() - digits;
    let mut out = String::with_capacity(kept.len() + 2);
    if negative {
        out.push('-');
    }
    out.push_str(std::str::from_utf8(&kept[..int_len]).expect("ascii"));
    if digits > 0 {
        out.push('.');
        out.push_str(std::str::from_utf8(&kept[int_len..]).expect("ascii"));
    }
    out
}

/// Printable ASCII in the order of the CLDR root collation, which is what
/// `String.prototype.localeCompare` uses under Node's full ICU. Letters of
/// both cases share a primary weight; lowercase sorts first at the tertiary
/// level.
const ROOT_ORDER: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ";

fn primary(c: char) -> (u32, u32) {
    match ROOT_ORDER.find(c.to_ascii_lowercase()) {
        Some(i) => (i as u32, u32::from(c.is_ascii_uppercase())),
        // Outside printable ASCII: after it, by code point. The plugin's keys
        // never carry such a character.
        None => (ROOT_ORDER.len() as u32 + c as u32, 0),
    }
}

/// `a.localeCompare(b)` for the strings this plugin compares (tile keys):
/// primary weights over the whole string first, then case.
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let pa = a.chars().map(|c| primary(c).0);
    let pb = b.chars().map(|c| primary(c).0);
    pa.cmp(pb).then_with(|| {
        let ta = a.chars().map(|c| primary(c).1);
        let tb = b.chars().map(|c| primary(c).1);
        ta.cmp(tb)
    })
}

/// `String(error)`: `Error.prototype.toString`.
pub fn error_string(error: &Error) -> String {
    match (error.name.is_empty(), error.message.is_empty()) {
        (_, true) => error.name.clone(),
        (true, false) => error.message.clone(),
        (false, false) => format!("{}: {}", error.name, error.message),
    }
}

/// The reason `AbortController.abort()` gives when called without one: a
/// `DOMException` named `AbortError`.
pub fn dom_abort_error() -> Error {
    Error::named("AbortError", "This operation was aborted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_matches_the_spec() {
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(1.005, 2), "1.00");
        assert_eq!(to_fixed(7.65, 2), "7.65");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(9.999, 2), "10.00");
        assert_eq!(to_fixed(-0.001, 2), "-0.00");
        assert_eq!(to_fixed(-0.0, 1), "0.0");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
        assert_eq!(to_fixed(f64::NAN, 1), "NaN");
        assert_eq!(to_fixed(7.65, 1), "7.7");
    }

    #[test]
    fn locale_compare_puts_punctuation_before_digits() {
        assert_eq!(locale_compare("3,1|a1", "3,10|a1"), Ordering::Less);
        assert_eq!(locale_compare("1,3", "10,3"), Ordering::Less);
        assert_eq!(locale_compare("a", "A"), Ordering::Less);
        assert_eq!(locale_compare("a", "b"), Ordering::Less);
        assert_eq!(locale_compare("B", "a"), Ordering::Greater);
        assert_eq!(locale_compare("ab", "ab"), Ordering::Equal);
        assert_eq!(locale_compare("a", "a|"), Ordering::Less);
        assert_eq!(locale_compare("z", "é"), Ordering::Less);
        assert_eq!(locale_compare("é", "€"), Ordering::Less);
    }

    #[test]
    fn error_string_is_error_to_string() {
        assert_eq!(error_string(&Error::named("EngineError", "boom")), "EngineError: boom");
        assert_eq!(error_string(&Error::named("AbortError", "")), "AbortError");
        assert_eq!(error_string(&Error::named("", "bare")), "bare");
    }
}
