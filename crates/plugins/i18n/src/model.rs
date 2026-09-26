//! `model.ts`: the i18n slice. Pure and serializable: locale packs are data,
//! so they live here, and `t()` is a pure function of this state.

use serde::{Deserialize, Serialize};

use crate::contract::{I18nConfig, Locale, TranslationDictionary, TranslationNode};
use crate::js::{Inherited, JsRecord};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nState {
    pub locale: String,
    pub fallback_locale: String,
    /// Registered packs by code: the lookup table `t()` reads.
    pub locales: JsRecord<Locale>,
    /// Code of a lazy pack being fetched (drives switcher spinners).
    pub loading: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum I18nAction {
    /// Switch to a registered locale; the reducer ignores unregistered codes.
    /// Loading a lazy pack goes through `LoadStarted` (the controller routes).
    #[serde(rename = "I18N/SET_LOCALE")]
    SetLocale { locale: String },
    #[serde(rename = "I18N/REGISTER_LOCALE")]
    RegisterLocale { locale: Locale },
    #[serde(rename = "I18N/UNREGISTER_LOCALE")]
    UnregisterLocale { locale: String },
    #[serde(rename = "I18N/ADD_TRANSLATIONS")]
    AddTranslations { locale: String, translations: TranslationDictionary },
    #[serde(rename = "I18N/LOAD_STARTED")]
    LoadStarted { locale: String },
    #[serde(rename = "I18N/LOAD_FAILED")]
    LoadFailed { locale: String },
}

/// `!!locales[code]`: an own pack, or an `Object.prototype` member, which a
/// JavaScript record lookup also finds (and which is truthy).
pub fn is_registered(locales: &JsRecord<Locale>, code: &str) -> bool {
    locales.contains_key(code) || Inherited::of(code).is_some()
}

/// `initialI18nState(config)`.
pub fn initial_i18n_state(config: &I18nConfig) -> I18nState {
    let fallback_locale = config.fallback_locale.clone().unwrap_or_else(|| "en".to_string());
    let mut locales = JsRecord::new();
    for locale in config.locales.iter().flatten() {
        locales.insert(locale.code.clone(), locale.clone());
    }
    let locale = config.locale.clone().unwrap_or_else(|| fallback_locale.clone());
    // A startup locale that is a lazy pack: show the fallback chain until the
    // pack arrives. Seeding `loading` makes the loader fetch it at connect.
    let needs_load = !is_registered(&locales, &locale) && config.loader(&locale).is_some();
    I18nState { loading: needs_load.then(|| locale.clone()), locale, fallback_locale, locales }
}

/// `mergeTranslations(base, patch)`: a deep merge where later leaves win and
/// branches merge.
pub fn merge_translations(
    base: &TranslationDictionary,
    patch: &TranslationDictionary,
) -> TranslationDictionary {
    let mut out = base.clone();
    for (key, value) in patch.iter() {
        let merged = match (value, out.get(key)) {
            (TranslationNode::Branch(value), Some(TranslationNode::Branch(current))) => {
                TranslationNode::Branch(merge_translations(current, value))
            }
            _ => value.clone(),
        };
        out.insert(key, merged);
    }
    out
}

/// `i18nReducer(state, action)`. `None` is `return state`.
pub fn i18n_reducer(state: &I18nState, action: &I18nAction) -> Option<I18nState> {
    match action {
        I18nAction::SetLocale { locale } => {
            // Only registered packs can become current: lazy ones arrive via
            // LOAD_STARTED, REGISTER_LOCALE, SET_LOCALE (the loader drives the middle).
            if !is_registered(&state.locales, locale) {
                return None;
            }
            if state.locale == *locale && state.loading.is_none() {
                return None;
            }
            Some(I18nState { locale: locale.clone(), loading: None, ..state.clone() })
        }
        I18nAction::RegisterLocale { locale } => {
            let mut locales = state.locales.clone();
            locales.insert(locale.code.clone(), locale.clone());
            Some(I18nState { locales, ..state.clone() })
        }
        I18nAction::UnregisterLocale { locale } => {
            if !is_registered(&state.locales, locale) {
                return None;
            }
            let mut locales = state.locales.clone();
            locales.remove(locale);
            Some(I18nState { locales, ..state.clone() })
        }
        I18nAction::AddTranslations { locale, translations } => {
            // An inherited member reads as a pack in the TypeScript too; it
            // has no translations to merge into, and is outside the port.
            let pack = state.locales.get(locale)?;
            let mut locales = state.locales.clone();
            locales.insert(
                locale.clone(),
                Locale {
                    translations: merge_translations(&pack.translations, translations),
                    ..pack.clone()
                },
            );
            Some(I18nState { locales, ..state.clone() })
        }
        I18nAction::LoadStarted { locale } => {
            if state.loading.as_deref() == Some(locale.as_str()) {
                None
            } else {
                Some(I18nState { loading: Some(locale.clone()), ..state.clone() })
            }
        }
        I18nAction::LoadFailed { locale } => {
            if state.loading.as_deref() == Some(locale.as_str()) {
                Some(I18nState { loading: None, ..state.clone() })
            } else {
                None
            }
        }
    }
}
