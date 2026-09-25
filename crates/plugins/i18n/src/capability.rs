//! `capability.ts`: selectors (pure reads over state) and intents (the only
//! writers). Built synchronously when the kernel is created, so `t()` works
//! before the engine exists.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::Ctx;

use crate::reducer::is_registered;
use crate::translate::translate;
use crate::types::{Dir, I18nAction, I18nConfig, I18nState, Locale, LocaleInfo, TranslateOptions};

#[derive(Clone)]
pub struct I18nCapability {
    ctx: Ctx<I18nState, I18nAction>,
    config: Rc<I18nConfig>,
    /// Dev signal, once per offender.
    warned: Rc<RefCell<HashSet<String>>>,
}

impl I18nCapability {
    pub(crate) fn new(ctx: Ctx<I18nState, I18nAction>, config: Rc<I18nConfig>) -> Self {
        I18nCapability { ctx, config, warned: Rc::default() }
    }

    fn warn_once(&self, id: String, message: impl FnOnce() -> String) {
        if self.warned.borrow_mut().insert(id) {
            self.config.warn(&message());
        }
    }

    /// Translate a key: current locale → fallback locale → `options.fallback` → the key.
    pub fn t(&self, key: &str, options: Option<&TranslateOptions>) -> String {
        let state = self.ctx.state();
        let result = translate(&state, key, options);
        if !result.found && options.and_then(|o| o.fallback.as_ref()).is_none() {
            self.warn_once(format!("key:{key}"), || {
                format!("[i18n] missing translation \"{key}\" (locale: {})", state.locale)
            });
        }
        result.text
    }

    pub fn locale(&self) -> String {
        self.ctx.state().locale.clone()
    }

    /// Text direction of the current locale.
    pub fn dir(&self) -> Dir {
        let state = self.ctx.state();
        state.locales.get(&state.locale).and_then(|l| l.dir).unwrap_or(Dir::Ltr)
    }

    /// Every known locale (registered, then lazy ones not yet loaded).
    pub fn locales(&self) -> Vec<LocaleInfo> {
        let state = self.ctx.state();
        let mut known: Vec<LocaleInfo> = state
            .locales
            .values()
            .map(|locale| LocaleInfo {
                code: locale.code.clone(),
                name: locale.name.clone(),
                dir: locale.dir.unwrap_or(Dir::Ltr),
                loaded: true,
            })
            .collect();
        for code in self.config.loaders.iter().flat_map(|l| l.keys()) {
            if !is_registered(&state.locales, code) {
                known.push(LocaleInfo {
                    code: code.to_string(),
                    name: code.to_string(),
                    dir: Dir::Ltr,
                    loaded: false,
                });
            }
        }
        known
    }

    /// Code of the lazy pack being fetched right now, if any.
    pub fn loading(&self) -> Option<String> {
        self.ctx.state().loading.clone()
    }

    /// Switch locale; fetches the pack first when it is a lazy one.
    pub fn set_locale(&self, code: &str) {
        let state = self.ctx.state();
        if is_registered(&state.locales, code) {
            self.ctx.dispatch(I18nAction::SetLocale { locale: code.to_string() });
        } else if self.config.loader(code).is_some() {
            self.ctx.dispatch(I18nAction::LoadStarted { locale: code.to_string() });
        } else {
            self.warn_once(format!("locale:{code}"), || {
                format!(
                    "[i18n] unknown locale \"{code}\" — not registered and no loader configured"
                )
            });
        }
    }

    /// Register a pack at runtime.
    pub fn register_locale(&self, locale: Locale) {
        self.ctx.dispatch(I18nAction::RegisterLocale { locale });
    }
}
