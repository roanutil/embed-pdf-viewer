//! The JavaScript built-ins this plugin's strings depend on, where Rust's
//! answer differs. The numeric ones (`Math.round`, `Math.max`, number
//! printing, `toFixed`) come from [`epdf_geometry::js`]; these three are the
//! ones only this plugin needs.

use std::cmp::Ordering;

use epdf_kernel::Error;

pub use epdf_geometry::js::{max, min, number_to_string, round, to_fixed};

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
