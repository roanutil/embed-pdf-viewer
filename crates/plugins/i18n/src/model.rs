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
    /// The languages' strings, by code: what `t()` reads. The same `Rc`
    /// until a language is added, replaced or removed.
    pub locales: Rc<JsRecord<Rc<Locale>>>,
    /// The language to show once it and the fallback language have loaded,
    /// or `None`.
    pub loading: Option<String>,
    /// The fallback language while its loader runs, or `None`; no language
    /// shows before it.
    pub loading_fallback: Option<String>,
    /// The languages whose loader has run, by code (`Record<string, true>`).
    pub fetched: Rc<JsRecord<bool>>,
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
        loading_fallback: None,
        fetched: Rc::default(),
        waiting_translations: Rc::default(),
    }
}

/// `state.fetched[code]`: an own entry, or an `Object.prototype` member,
/// which a JavaScript record lookup also finds (and which is truthy).
fn is_fetched(state: &I18nState, code: &str) -> bool {
    state.fetched.contains_key(code) || Inherited::of(code).is_some()
}

/// `needsFetch(state, loaders, code)`: whether a language's loader is still
/// to run. A language in `locales` too shows those strings meanwhile.
/// `has_loader` is `loaders[code] !== undefined`.
pub fn needs_fetch(state: &I18nState, has_loader: impl Fn(&str) -> bool, code: &str) -> bool {
    has_loader(code) && !is_fetched(state, code)
}

/// `isLoaded(state, loaders, code)`: whether a language has all its
/// strings: it's in, and its loader, if it has one, has run.
pub fn is_loaded(state: &I18nState, has_loader: impl Fn(&str) -> bool, code: &str) -> bool {
    is_registered(&state.locales, code) && !needs_fetch(state, has_loader, code)
}

/// `isReady(state, loaders, code)`: whether a language can show now: it's
/// loaded, and the fallback language isn't on its way.
pub fn is_ready(state: &I18nState, has_loader: impl Fn(&str) -> bool, code: &str) -> bool {
    is_loaded(state, has_loader, code) && state.loading_fallback.is_none()
}

/// `followFallback(state, settings)`: the fallback language waits for its
/// loader when it has one that hasn't run.
pub fn follow_fallback(
    state: &Rc<I18nState>,
    fallback_locale: &str,
    has_loader: impl Fn(&str) -> bool,
) -> Rc<I18nState> {
    let loading_fallback =
        needs_fetch(state, has_loader, fallback_locale).then(|| fallback_locale.to_string());
    if state.loading_fallback == loading_fallback {
        return state.clone();
    }
    Rc::new(I18nState { loading_fallback, ..(**state).clone() })
}

/// `seedFromSettings(state, settings)`: the settings' languages in, the
/// fallback language fetched when it has a loader, and their `locale`
/// current once it can show. Until then `loading` names it, so the loader
/// fetches it once connected, and the fallback language shows, or, while
/// that loads too, the first language in `locales`. `has_loader` is
/// `settings.loaders[code] !== undefined`.
pub fn seed_from_settings(
    state: &Rc<I18nState>,
    locale: Option<&str>,
    fallback_locale: &str,
    locales: &[Locale],
    has_loader: impl Fn(&str) -> bool,
) -> Rc<I18nState> {
    let registered =
        locales.iter().fold(state.clone(), |state, locale| register_locale(&state, locale));
    let seeded = follow_fallback(&registered, fallback_locale, &has_loader);
    let locale = locale.unwrap_or(fallback_locale);
    let known = is_registered(&seeded.locales, locale) || has_loader(locale);
    if !known || is_ready(&seeded, &has_loader, locale) {
        return Rc::new(I18nState { locale: locale.to_string(), ..(*seeded).clone() });
    }
    let meanwhile = if is_registered(&seeded.locales, fallback_locale) {
        fallback_locale
    } else {
        seeded.locales.keys().next().unwrap_or(fallback_locale)
    };
    Rc::new(I18nState {
        locale: meanwhile.to_string(),
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
    let mut fetched = (*state.fetched).clone();
    fetched.remove(code);
    Rc::new(I18nState { locales: Rc::new(locales), fetched: Rc::new(fetched), ..(**state).clone() })
}

/// `arriveLocale(state, code, locale)`: a language from its loader arrived:
/// it registers in place of any strings the language had, and a wait for
/// it as the fallback language ends.
pub fn arrive_locale(state: &Rc<I18nState>, code: &str, locale: &Locale) -> Rc<I18nState> {
    let registered = register_locale(state, locale);
    let mut fetched = (*registered.fetched).clone();
    fetched.insert(code, true);
    let loading_fallback =
        registered.loading_fallback.clone().filter(|waiting| waiting.as_str() != code);
    Rc::new(I18nState { fetched: Rc::new(fetched), loading_fallback, ..(*registered).clone() })
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

/// `endFallbackLoad(state, code)`: the fallback language's loader failed:
/// languages show without it.
pub fn end_fallback_load(state: &Rc<I18nState>, code: &str) -> Rc<I18nState> {
    if state.loading_fallback.as_deref() != Some(code) {
        return state.clone();
    }
    Rc::new(I18nState { loading_fallback: None, ..(**state).clone() })
}
