#![cfg(feature = "uniffi")]

use std::collections::HashMap;

use epdf_ffi::ViewerError;
use epdf_plugin_i18n::Locale;
use epdf_plugin_i18n::ffi::{LocalePack, TextDirection, locale_pack_from_json};

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn pack(code: &str, entries: &[(&str, &str)]) -> LocalePack {
    LocalePack {
        code: code.into(),
        name: code.to_uppercase(),
        dir: None,
        translations: entries.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
    }
}

#[test]
fn a_locale_json_file_flattens_to_dotted_keys() {
    let pack = locale_pack_from_json(
        r#"{"code":"es","name":"Español","dir":"ltr",
            "translations":{"zoom":{"in":"Acercar","out":"Alejar"},"pages":{"one":"{count} página","other":"{count} páginas"}}}"#
            .into(),
    )
    .unwrap();
    assert_eq!(pack.code, "es");
    assert_eq!(pack.dir, Some(TextDirection::Ltr));
    let expected: HashMap<String, String> = [
        ("zoom.in", "Acercar"),
        ("zoom.out", "Alejar"),
        ("pages.one", "{count} página"),
        ("pages.other", "{count} páginas"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    assert_eq!(pack.translations, expected);
}

#[test]
fn json_that_is_not_a_locale_is_invalid_input() {
    let error = locale_pack_from_json(r#"{"code":"es"}"#.into()).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_flat_pack_rebuilds_the_tree_the_plugin_reads() {
    let locale =
        Locale::try_from(pack("es", &[("zoom.in", "Acercar"), ("title", "Visor")])).unwrap();
    let json = serde_json::to_value(&locale.translations).unwrap();
    assert_eq!(json, serde_json::json!({ "title": "Visor", "zoom": { "in": "Acercar" } }));
}

#[test]
fn a_key_that_is_both_leaf_and_branch_is_refused() {
    let error = Locale::try_from(pack("es", &[("a", "x"), ("a.b", "y")])).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"), "{error:?}");
    assert!(error.message.contains("\"a"), "{error:?}");
}

#[test]
fn an_empty_key_segment_is_refused() {
    let error = Locale::try_from(pack("es", &[("zoom..in", "x")])).unwrap_err();
    assert_eq!(error.code(), Some("invalid-input"), "{error:?}");
}
