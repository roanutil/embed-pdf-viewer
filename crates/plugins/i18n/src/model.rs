//! `model.ts`: the i18n state, the current language, the loaded languages'
//! strings, and the language being loaded. Plain data, so `t()` is a pure
//! function of it. Every function below is a pure transition; the
//! controller applies them with `ctx.state.update`. A transition that
//! changes nothing returns the state it was given (the same `Rc`), so the
//! store sees no change.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::contract::{Locale, TranslationDictionary, TranslationNode};
use crate::js::{Inherited, JsRecord};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nState {
    /// The current language's code.
    pub locale: String,
    /// The loaded languages, by code: what `t()` reads. The same `Rc` until
    /// a language is added, replaced or removed.
    pub locales: Rc<JsRecord<Rc<Locale>>>,
    /// The code of a language in `loaders` being fetched, or `None`.
    pub loading: Option<String>,
    /// Strings added to a language before it loaded, by code; they merge in
    /// when it does.
    pub waiting_translations: Rc<JsRecord<TranslationDictionary>>,
}

/// `!!locales[code]`: an own pack, or an `Object.prototype` member, which a
/// JavaScript record lookup also finds (and which is truthy).
pub fn is_registered(locales: &JsRecord<Rc<Locale>>, code: &str) -> bool {
    locales.contains_key(code) || Inherited::of(code).is_some()
}

/// `initialI18nState()`: nothing loaded yet; the controller seeds the state
/// from the settings.
pub fn initial_i18n_state() -> I18nState {
    I18nState {
        locale: String::new(),
        locales: Rc::default(),
        loading: None,
        waiting_translations: Rc::default(),
    }
}

/// `seedFromSettings(state, settings)`: the settings' languages loaded, and
/// their `locale` current. A start language that is still to load shows
/// `fallback_locale` meanwhile, and `loading` names it, so the loader
/// fetches it once connected. `has_loader` is `settings.loaders[code] !==
/// undefined`.
pub fn seed_from_settings(
    state: &Rc<I18nState>,
    locale: Option<&str>,
    fallback_locale: &str,
    locales: &[Locale],
    has_loader: impl Fn(&str) -> bool,
) -> Rc<I18nState> {
    let seeded =
        locales.iter().fold(state.clone(), |state, locale| register_locale(&state, locale));
    let locale = locale.unwrap_or(fallback_locale);
    if is_registered(&seeded.locales, locale) || !has_loader(locale) {
        return Rc::new(I18nState { locale: locale.to_string(), ..(*seeded).clone() });
    }
    Rc::new(I18nState {
        locale: fallback_locale.to_string(),
        loading: Some(locale.to_string()),
        ..(*seeded).clone()
    })
}

/// `mergeTranslations(base, patch)`: a deep merge where later leaves win and
/// branches merge.
pub fn merge_translations(
    base: &TranslationDictionary,
    patch: &TranslationDictionary,
) -> TranslationDictionary {
    let mut merged = base.clone();
    for (key, value) in patch.iter() {
        let next = match (value, merged.get(key)) {
            (TranslationNode::Branch(value), Some(TranslationNode::Branch(current))) => {
                TranslationNode::Branch(merge_translations(current, value))
            }
            _ => value.clone(),
        };
        merged.insert(key, next);
    }
    merged
}

fn has_dotted_key(dictionary: &TranslationDictionary) -> bool {
    dictionary.iter().any(|(key, value)| {
        key.contains('.')
            || matches!(value, TranslationNode::Branch(branch) if has_dotted_key(branch))
    })
}

/// `nestKeys(dictionary)`: dotted keys as branches, the shape `t()` walks.
/// Keys merge in order, so a later one wins. A dictionary without a dotted
/// key comes back as it is.
pub fn nest_keys(dictionary: &TranslationDictionary) -> TranslationDictionary {
    if !has_dotted_key(dictionary) {
        return dictionary.clone();
    }
    let mut nested = TranslationDictionary::new();
    for (key, value) in dictionary.iter() {
        let leaf = match value {
            TranslationNode::Branch(branch) => TranslationNode::Branch(nest_keys(branch)),
            text => text.clone(),
        };
        // `key.split('.').reduceRight((inner, part) => ({ [part]: inner }), leaf)`
        let branch = key.split('.').rev().fold(leaf, |inner, part| {
            TranslationNode::Branch([(part.to_string(), inner)].into_iter().collect())
        });
        nested = match branch {
            TranslationNode::Branch(branch) => merge_translations(&nested, &branch),
            // `key.split('.')` has at least one part, so the fold wraps the
            // leaf at least once.
            TranslationNode::Text(_) => unreachable!("a key has at least one part"),
        };
    }
    nested
}

fn with_nested_keys(locale: &Locale) -> Locale {
    Locale { translations: nest_keys(&locale.translations), ..locale.clone() }
}

/// `setLocale(state, code)`: switch to a loaded language and end any load.
/// A code that isn't loaded changes nothing: a language in `loaders`
/// becomes current through [`start_locale_load`], [`register_locale`] and
/// then this transition, which the loader drives.
pub fn set_locale(state: &Rc<I18nState>, code: &str) -> Rc<I18nState> {
    if !is_registered(&state.locales, code) {
        return state.clone();
    }
    if state.locale == code && state.loading.is_none() {
        return state.clone();
    }
    Rc::new(I18nState { locale: code.to_string(), loading: None, ..(**state).clone() })
}

/// `registerLocale(state, locale)`: add a language, with any strings that
/// were added to it before it loaded on top.
pub fn register_locale(state: &Rc<I18nState>, locale: &Locale) -> Rc<I18nState> {
    let pack = with_nested_keys(locale);
    let mut locales = (*state.locales).clone();
    let Some(waiting) = state.waiting_translations.get(&locale.code) else {
        locales.insert(locale.code.clone(), Rc::new(pack));
        return Rc::new(I18nState { locales: Rc::new(locales), ..(**state).clone() });
    };
    let translations = merge_translations(&pack.translations, waiting);
    locales.insert(locale.code.clone(), Rc::new(Locale { translations, ..pack }));
    let mut still_waiting = (*state.waiting_translations).clone();
    still_waiting.remove(&locale.code);
    Rc::new(I18nState {
        locales: Rc::new(locales),
        waiting_translations: Rc::new(still_waiting),
        ..(**state).clone()
    })
}

/// `unregisterLocale(state, code)`. An `Object.prototype` member reads as
/// registered, so it copies the packs and removes nothing, as the
/// TypeScript's rest spread does.
pub fn unregister_locale(state: &Rc<I18nState>, code: &str) -> Rc<I18nState> {
    if !is_registered(&state.locales, code) {
        return state.clone();
    }
    let mut locales = (*state.locales).clone();
    locales.remove(code);
    Rc::new(I18nState { locales: Rc::new(locales), ..(**state).clone() })
}

/// `addTranslations(state, code, dictionary)`: merge strings into a
/// language, later keys winning. For a language that isn't loaded yet (one
/// in `loaders`), the strings wait and merge in when it loads; the
/// controller only lets codes with a loader get here.
///
/// A code that names an `Object.prototype` member reads as loaded in the
/// TypeScript, which merges into a pack made from that member; this port
/// leaves the state as it is (README, "What is not ported").
pub fn add_translations(
    state: &Rc<I18nState>,
    code: &str,
    dictionary: &TranslationDictionary,
) -> Rc<I18nState> {
    let strings = nest_keys(dictionary);
    let Some(pack) = state.locales.get(code) else {
        if Inherited::of(code).is_some() {
            return state.clone();
        }
        let waiting = state.waiting_translations.get(code).cloned().unwrap_or_default();
        let mut waiting_translations = (*state.waiting_translations).clone();
        waiting_translations.insert(code, merge_translations(&waiting, &strings));
        return Rc::new(I18nState {
            waiting_translations: Rc::new(waiting_translations),
            ..(**state).clone()
        });
    };
    let translations = merge_translations(&pack.translations, &strings);
    let mut locales = (*state.locales).clone();
    locales.insert(code, Rc::new(Locale { translations, ..(**pack).clone() }));
    Rc::new(I18nState { locales: Rc::new(locales), ..(**state).clone() })
}

/// `startLocaleLoad(state, code)`: a language in `loaders` is wanted; the
/// loader fetches whatever `loading` names.
pub fn start_locale_load(state: &Rc<I18nState>, code: &str) -> Rc<I18nState> {
    if state.loading.as_deref() == Some(code) {
        return state.clone();
    }
    Rc::new(I18nState { loading: Some(code.to_string()), ..(**state).clone() })
}

/// `endLocaleLoad(state, code)`: a load ended without switching, because it
/// failed or was cancelled. Only the load still wanted ends.
pub fn end_locale_load(state: &Rc<I18nState>, code: &str) -> Rc<I18nState> {
    if state.loading.as_deref() != Some(code) {
        return state.clone();
    }
    Rc::new(I18nState { loading: None, ..(**state).clone() })
}
