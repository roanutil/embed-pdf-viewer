//! `reducer.ts`: pure and serializable. Locale packs are data, so they live
//! here, and `t()` is a pure function of this state.

use crate::js::{Inherited, JsRecord};
use crate::types::{I18nAction, I18nConfig, I18nState, Locale};

/// `!!locales[code]`: an own pack, or an `Object.prototype` member, which a
/// JavaScript record lookup also finds (and which is truthy).
pub fn is_registered(locales: &JsRecord<Locale>, code: &str) -> bool {
    locales.contains_key(code) || Inherited::of(code).is_some()
}

pub fn initial_i18n_state(config: &I18nConfig) -> I18nState {
    let fallback_locale = config.fallback_locale.clone().unwrap_or_else(|| "en".to_string());
    let mut locales = JsRecord::new();
    for locale in config.locales.iter().flatten() {
        locales.insert(locale.code.clone(), locale.clone());
    }
    let locale = config.locale.clone().unwrap_or_else(|| fallback_locale.clone());
    // A startup locale that is a lazy pack: show the fallback chain until the
    // pack arrives. Seeding `loading` makes the effect fetch it at startup.
    let needs_load = !is_registered(&locales, &locale) && config.loader(&locale).is_some();
    I18nState { loading: needs_load.then(|| locale.clone()), locale, fallback_locale, locales }
}

/// `None` is `return state`.
pub fn i18n_reducer(state: &I18nState, action: &I18nAction) -> Option<I18nState> {
    match action {
        I18nAction::SetLocale { locale } => {
            // Only registered packs can become current.
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
