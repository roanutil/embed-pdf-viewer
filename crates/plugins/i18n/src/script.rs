//! `scriptOf` from `negotiate.ts`: `new Intl.Locale(code).maximize().script`,
//! with `null` when the constructor throws or the tag maximizes to no script.
//!
//! The constructor takes a structurally valid Unicode BCP 47 locale
//! identifier, the grammar [`crate::plural::parse`] checks, and throws a
//! `RangeError` for anything else. `maximize` adds the likely script from
//! CLDR's likely subtags, which [`crate::script_data`] holds as ICU applies
//! them, generated from Node: what every two- and three-letter language
//! subtag maximizes to alone and with every region subtag, and the few
//! places the unknown script `Zzzz` does not follow the rule below. Every
//! other language (one ICU has no likely subtags for, which keeps no script)
//! maximizes to no script, and variants and extensions leave the script as
//! it is.
//!
//! One thing ICU does is not here: a legacy alias keyed by a language and a
//! variant together. Among those CLDR lists, only `art-lojban` (to `jbo`)
//! turns a language with no likely script into one with a script: ICU
//! answers `Latn`, this module `None`, which [`crate::negotiate`] reads as
//! "can't tell". The others (`cel-gaulish`, `zh-hakka`, `zh-xiang`,
//! `zh-guoyu`, `aar-saaho`, `hy-arevela`, `hy-arevmda`, `no-bokmal`,
//! `no-nynorsk`) land on the script their language already has, or on none.
//!
//! The rule for `Zzzz`: without a region, ICU replaces it with the
//! language's likely script and keeps it for a language that has none; with
//! a region, it keeps it.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::plural::parse;
use crate::script_data::{LANGUAGE_SCRIPTS, REGION_SCRIPTS, ZZZZ_SCRIPTS};

static BY_LANGUAGE: LazyLock<HashMap<&str, &str>> = LazyLock::new(|| {
    LANGUAGE_SCRIPTS
        .iter()
        .flat_map(|(script, lines)| {
            lines.iter().flat_map(|line| line.split_whitespace()).map(move |lang| (lang, *script))
        })
        .collect()
});

static BY_REGION: LazyLock<HashMap<(&str, &str), &str>> = LazyLock::new(|| {
    REGION_SCRIPTS.iter().map(|(lang, region, script)| ((*lang, *region), *script)).collect()
});

static ZZZZ: LazyLock<HashMap<(&str, &str), &str>> = LazyLock::new(|| {
    ZZZZ_SCRIPTS.iter().map(|(lang, region, script)| ((*lang, *region), *script)).collect()
});

/// A table's script: an empty one is none.
fn script(found: &str) -> Option<String> {
    (!found.is_empty()).then(|| found.to_string())
}

/// `Latn` for `latn`: a script subtag as `Intl.Locale` prints it.
fn title_case(script: &str) -> String {
    let mut chars = script.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// `new Intl.Locale(code).maximize().script ?? null`, with `None` for a tag
/// the constructor refuses as well.
pub fn script_of(code: &str) -> Option<String> {
    let tag = parse(code)?;
    let language = tag.language.as_str();
    let region = tag.region.as_deref();
    let likely = || {
        region
            .and_then(|region| BY_REGION.get(&(language, region)))
            .map(|found| script(found))
            .unwrap_or_else(|| BY_LANGUAGE.get(language).map(|found| found.to_string()))
    };
    match tag.script.as_deref() {
        None => likely(),
        Some("zzzz") => match ZZZZ.get(&(language, region.unwrap_or(""))) {
            Some(found) => script(found),
            None if region.is_some() => Some("Zzzz".to_string()),
            None => Some(BY_LANGUAGE.get(language).copied().unwrap_or("Zzzz").to_string()),
        },
        Some(given) => Some(title_case(given)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `new Intl.Locale(tag).maximize().script ?? null` under Node 24.20.0
    /// (ICU 78.3), with `None` where the constructor throws.
    #[test]
    fn scripts_match_intl_locale_maximize() {
        let cases: &[(&str, Option<&str>)] = &[
            ("zh", Some("Hans")),
            ("zh-TW", Some("Hant")),
            ("zh-HK", Some("Hant")),
            ("zh-419", Some("Hans")),
            ("zh-Hans-TW", Some("Hans")),
            ("cmn-TW", Some("Hant")),
            ("yue", Some("Hant")),
            ("sr", Some("Cyrl")),
            ("sr-ME", Some("Latn")),
            ("sh", Some("Latn")),
            ("sh-RS", Some("Latn")),
            ("sh-Cyrl", Some("Cyrl")),
            ("cnr", Some("Latn")),
            ("cnr-RS", Some("Cyrl")),
            ("und", Some("Latn")),
            ("und-TW", Some("Hant")),
            ("und-Hant-CN", Some("Hant")),
            ("en-Cyrl", Some("Cyrl")),
            ("EN-latn", Some("Latn")),
            ("en-Zzzz", Some("Latn")),
            ("zh-Zzzz", Some("Hans")),
            ("zh-Zzzz-TW", Some("Zzzz")),
            ("und-Zzzz-TW", Some("Hant")),
            ("xx-Zzzz", Some("Zzzz")),
            ("xx-Shaw", Some("Shaw")),
            ("xx", None),
            ("xx-TW", None),
            ("abcdefgh", None),
            ("abcde-Cyrl", Some("Cyrl")),
            ("ca-ES-valencia", Some("Latn")),
            ("hy-arevmda", Some("Armn")),
            ("en-US-u-rg-twzzzz", Some("Latn")),
            ("en-t-ja", Some("Latn")),
            ("i-klingon", None),
            ("root", None),
            ("", None),
            ("en_US", None),
            ("zh-min", None),
            ("zh-TW-u-ca-x", None),
        ];
        for (tag, want) in cases {
            assert_eq!(script_of(tag).as_deref(), *want, "{tag:?}");
        }
    }

    /// ICU canonicalizes a legacy language-and-variant alias first; this
    /// port does not (module docs). `art-lojban` is `jbo-Latn-001` in ICU.
    #[test]
    fn a_language_and_variant_alias_is_not_applied() {
        assert_eq!(script_of("art-lojban"), None);
        assert_eq!(script_of("jbo"), Some("Latn".to_string()));
        assert_eq!(script_of("zh-hakka").as_deref(), Some("Hans"));
    }
}
