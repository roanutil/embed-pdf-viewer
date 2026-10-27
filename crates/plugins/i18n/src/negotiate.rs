//! `negotiate.ts`: pure locale negotiation over language tags (RFC 5646),
//! strings in and a string out. The plugin never reads the platform; the
//! embedder feeds it whatever request list its environment offers.
//!
//! Matching, per requested code in preference order:
//!   1. exact match (case-insensitive):        `es-MX` → `es-MX`
//!   2. request narrowed to its language:      `en-GB` → `en`
//!   3. any available dialect of the language: `zh`    → `zh-Hans`
//!
//! Steps 2 and 3 match only within the script the language is written in,
//! as `Intl.Locale#maximize` gives it ([`crate::script`]): `zh-TW` and
//! `zh-HK` are Traditional Chinese, so `zh-CN` (Simplified) isn't theirs,
//! and they get no match until a Traditional language is available. Where
//! the script can't be told, the language alone decides.
//!
//! Returns the matched code exactly as it appears in `available`, or `None`.

use crate::script::script_of;

/// Whether two tags are written in the same script, or one of them can't
/// say.
fn same_script(left: &str, right: &str) -> bool {
    let (Some(left), Some(right)) = (script_of(left), script_of(right)) else { return true };
    left == right
}

/// `code.toLowerCase().split('-')[0]`.
fn language_of(code: &str) -> String {
    let lower = code.to_lowercase();
    match lower.split_once('-') {
        Some((language, _)) => language.to_string(),
        None => lower,
    }
}

pub fn negotiate_locale<A: AsRef<str>, R: AsRef<str>>(
    available: &[A],
    requested: &[R],
) -> Option<String> {
    // `Map<canonical, original>` in insertion order, first spelling wins.
    let mut by_canonical: Vec<(String, &str)> = Vec::new();
    for code in available {
        let code = code.as_ref();
        let canonical = code.to_lowercase();
        if !by_canonical.iter().any(|(c, _)| *c == canonical) {
            by_canonical.push((canonical, code));
        }
    }
    let get = |canonical: &str| by_canonical.iter().find(|(c, _)| c == canonical).map(|(_, o)| *o);

    for wanted in requested {
        if let Some(exact) = get(&wanted.as_ref().to_lowercase()) {
            return Some(exact.to_string());
        }
    }
    for wanted in requested {
        let wanted = wanted.as_ref();
        if let Some(narrowed) = get(&language_of(wanted))
            && same_script(wanted, narrowed)
        {
            return Some(narrowed.to_string());
        }
    }
    for wanted in requested {
        let wanted = wanted.as_ref();
        let language = language_of(wanted);
        for (canonical, original) in &by_canonical {
            if language_of(canonical) == language && same_script(wanted, original) {
                return Some(original.to_string());
            }
        }
    }
    None
}
