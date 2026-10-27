//! `strings/index.ts`: EmbedPDF's own strings, such as the standard
//! commands' labels, in the eight languages of the standard stamps. They sit
//! under the app's strings: a key the app gives a string replaces
//! EmbedPDF's (`addTranslations` under the same key).
//!
//! Each language is the TypeScript module's object as JSON
//! (`strings/<code>.ts` printed with `JSON.stringify`), in the module's key
//! order.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use crate::contract::TranslationDictionary;
use crate::negotiate::negotiate_locale;

/// `EMBEDPDF_STRINGS`, in its key order: `CODES` is `Object.keys` of it.
const SOURCES: [(&str, &str); 8] = [
    ("en", include_str!("en.json")),
    ("nl", include_str!("nl.json")),
    ("de", include_str!("de.json")),
    ("fr", include_str!("fr.json")),
    ("es", include_str!("es.json")),
    ("zh-CN", include_str!("zh-cn.json")),
    ("sv", include_str!("sv.json")),
    ("ja", include_str!("ja.json")),
];

static EMBEDPDF_STRINGS: LazyLock<Vec<(&str, TranslationDictionary)>> = LazyLock::new(|| {
    SOURCES
        .iter()
        .map(|(code, json)| {
            let dictionary = serde_json::from_str(json)
                .unwrap_or_else(|e| panic!("strings/{code}.json is a dictionary: {e}"));
            (*code, dictionary)
        })
        .collect()
});

thread_local! {
    /// `languageOf`: which of EmbedPDF's languages each code reads, decided
    /// once per code.
    static LANGUAGE_OF: RefCell<HashMap<String, Option<usize>>> = RefCell::default();
}

/// `embedpdfStringsFor(code)`: EmbedPDF's strings for a language code,
/// matched as [`negotiate_locale`] matches: `nl-BE` reads Dutch and `zh`
/// Simplified Chinese. `None` for a language EmbedPDF has no strings in.
pub fn embedpdf_strings_for(code: &str) -> Option<&'static TranslationDictionary> {
    let strings = &*EMBEDPDF_STRINGS;
    let language = LANGUAGE_OF.with(|cache| {
        if let Some(language) = cache.borrow().get(code) {
            return *language;
        }
        let codes: Vec<&str> = strings.iter().map(|(code, _)| *code).collect();
        let language = negotiate_locale(&codes, &[code])
            .and_then(|matched| codes.iter().position(|c| *c == matched));
        cache.borrow_mut().insert(code.to_string(), language);
        language
    });
    language.map(|index| &strings[index].1)
}
