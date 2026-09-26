//! `controller.ts`: reads over the slice, the locale intents, and the lazy
//! loader wired at `connect`. Built when the kernel is created, so `t()`
//! works before the engine exists, including inside the shell's loading UI.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Resolver;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, Error, Promise};

use crate::contract::{
    Dir, I18nConfig, Locale, LocaleChangedEvent, LocaleInfo, LocaleLoadFailedEvent,
    TranslateOptions, TranslationDictionary,
};
use crate::loaders::Loaders;
use crate::model::{I18nAction, I18nState, is_registered};
use crate::translate::translate;

struct Inner {
    cx: Controller<I18nState, I18nAction>,
    config: Rc<I18nConfig>,
    locale_changed: EventHookSource<LocaleChangedEvent>,
    locale_load_failed: EventHookSource<LocaleLoadFailedEvent>,
    /// Dev signal, once per offender.
    warned: RefCell<HashSet<String>>,
    /// One pending promise per lazy load, in insertion order; a newer
    /// request supersedes it.
    pending: RefCell<Vec<(String, Resolver<()>)>>,
    /// The locale `onLocaleChanged` last announced.
    announced: RefCell<String>,
    loaders: RefCell<Option<Rc<Loaders>>>,
}

/// `I18nCapability` (and `I18nHostCapability`, which equals it today).
#[derive(Clone)]
pub struct I18nCapability(Rc<Inner>);

impl I18nCapability {
    fn warn_once(&self, id: String, message: impl FnOnce() -> String) {
        if self.0.warned.borrow_mut().insert(id) {
            self.0.config.warn(&message());
        }
    }

    fn settle(&self, code: &str, outcome: Result<(), Error>) {
        let waiter = {
            let mut pending = self.0.pending.borrow_mut();
            let Some(index) = pending.iter().position(|(c, _)| c == code) else { return };
            pending.remove(index).1
        };
        waiter.settle(outcome);
    }

    fn supersede(&self) {
        let waiters = std::mem::take(&mut *self.0.pending.borrow_mut());
        for (code, waiter) in waiters {
            waiter.reject(plugin_error(
                code::OPERATION_CANCELLED,
                "i18n",
                format!("switching to '{code}' was superseded"),
                None,
                None,
            ));
        }
    }

    /// A load completing switches the locale from inside the loader, so the
    /// change is announced here, once the locale is usable.
    fn announce_if_changed(&self) {
        let state = self.0.cx.state();
        if *self.0.announced.borrow() == state.locale || state.loading.is_some() {
            return;
        }
        let previous_locale = self.0.announced.replace(state.locale.clone());
        self.0
            .locale_changed
            .emit(&LocaleChangedEvent { locale: state.locale.clone(), previous_locale });
    }

    /// Translate a key: current locale, fallback locale, `options.fallback`,
    /// then the key itself. A miss with no fallback warns once per key.
    pub fn t(&self, key: &str, options: Option<&TranslateOptions>) -> String {
        let state = self.0.cx.state();
        let result = translate(&state, key, options);
        if !result.found && options.and_then(|o| o.fallback.as_ref()).is_none() {
            self.warn_once(format!("key:{key}"), || {
                format!("[i18n] missing translation \"{key}\" (locale: {})", state.locale)
            });
        }
        result.text
    }

    /// Is a key translated in the current or fallback locale? Never warns.
    pub fn has_key(&self, key: &str) -> bool {
        translate(&self.0.cx.state(), key, None).found
    }

    pub fn get_locale(&self) -> String {
        self.0.cx.state().locale.clone()
    }

    /// Text direction of the current locale.
    pub fn get_direction(&self) -> Dir {
        let state = self.0.cx.state();
        state.locales.get(&state.locale).and_then(|l| l.dir).unwrap_or(Dir::Ltr)
    }

    /// Every known locale: registered packs in registration order, then lazy
    /// ones not yet loaded (the code as the name).
    pub fn list_locales(&self) -> Vec<LocaleInfo> {
        let state = self.0.cx.state();
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
        for code in self.0.config.loaders.iter().flat_map(|l| l.keys()) {
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
    pub fn get_loading_locale(&self) -> Option<String> {
        self.0.cx.state().loading.clone()
    }

    /// Switch locale. Resolves once the locale is usable (a lazy pack is
    /// fetched first); rejects `not-found` for an unknown code,
    /// `operation-failed` when the pack fails to load, and
    /// `operation-cancelled` when a newer call supersedes this one.
    pub fn set_locale(&self, code: &str) -> Promise<()> {
        let state = self.0.cx.state();
        if is_registered(&state.locales, code) {
            self.supersede();
            self.0.cx.dispatch(I18nAction::SetLocale { locale: code.to_string() });
            self.announce_if_changed();
            return Promise::resolved(());
        }
        if self.0.config.loader(code).is_some() {
            self.supersede();
            let (promise, resolver) = Promise::pending();
            {
                let mut pending = self.0.pending.borrow_mut();
                match pending.iter_mut().find(|(c, _)| c == code) {
                    Some(slot) => slot.1 = resolver,
                    None => pending.push((code.to_string(), resolver)),
                }
            }
            self.0.cx.dispatch(I18nAction::LoadStarted { locale: code.to_string() });
            return promise;
        }
        Promise::rejected(plugin_error(
            code::NOT_FOUND,
            "i18n",
            format!("unknown locale '{code}' — not registered and no loader configured"),
            None,
            None,
        ))
    }

    /// Register a pack at runtime. The remover drops it again; every call
    /// of it dispatches `UNREGISTER_LOCALE`, as the TypeScript's does.
    pub fn register_locale(&self, locale: Locale) -> Rc<dyn Fn()> {
        let code = locale.code.clone();
        self.0.cx.dispatch(I18nAction::RegisterLocale { locale });
        let cx = self.0.cx.clone();
        Rc::new(move || cx.dispatch(I18nAction::UnregisterLocale { locale: code.clone() }))
    }

    /// Merge keys into a registered pack (later keys win). Errs `not-found`
    /// for an unknown code.
    pub fn add_translations(
        &self,
        code: &str,
        dictionary: TranslationDictionary,
    ) -> Result<(), Error> {
        if !is_registered(&self.0.cx.state().locales, code) {
            return Err(plugin_error(
                code::NOT_FOUND,
                "i18n",
                format!("unknown locale '{code}'"),
                None,
                None,
            ));
        }
        self.0.cx.dispatch(I18nAction::AddTranslations {
            locale: code.to_string(),
            translations: dictionary,
        });
        Ok(())
    }

    /// `onLocaleChanged`: the current locale changed and is usable.
    pub fn on_locale_changed(
        &self,
        listener: impl Fn(&LocaleChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.locale_changed.on(listener)
    }

    /// `onLocaleLoadFailed`: a lazy pack failed to load.
    pub fn on_locale_load_failed(
        &self,
        listener: impl Fn(&LocaleLoadFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.locale_load_failed.on(listener)
    }

    fn connect(&self) -> Result<(), Error> {
        let loaders = self.0.loaders.borrow().clone();
        if let Some(loaders) = loaders {
            loaders.connect()?;
        }
        let this = self.clone();
        let unsubscribe = self.0.cx.subscribe(Rc::new(move || {
            this.announce_if_changed();
            Ok(())
        }));
        self.0.cx.cleanup_fn(move || unsubscribe.call());
        Ok(())
    }
}

/// `createI18nController(ctx, config)`: the api and its `connect`.
pub fn create_i18n_controller(
    cx: Controller<I18nState, I18nAction>,
    config: Rc<I18nConfig>,
) -> (I18nCapability, ConnectFn) {
    // The TypeScript reports a failing listener to `console.error`.
    let report: Rc<dyn Fn(&Error)> = Rc::new(|error| eprintln!("[i18n] listener failed: {error}"));
    let locale_changed = EventHookSource::new(Some(report.clone()));
    let locale_load_failed = EventHookSource::new(Some(report));
    {
        let (changed, failed) = (locale_changed.clone(), locale_load_failed.clone());
        cx.cleanup_fn(move || {
            changed.dispose();
            failed.dispose();
        });
    }
    let announced = cx.state().locale.clone();
    let api = I18nCapability(Rc::new(Inner {
        cx: cx.clone(),
        config: config.clone(),
        locale_changed,
        locale_load_failed,
        warned: RefCell::default(),
        pending: RefCell::default(),
        announced: RefCell::new(announced),
        loaders: RefCell::new(None),
    }));
    let weak = Rc::downgrade(&api.0);
    let settle = Rc::new(move |code: &str, outcome: Result<(), Error>| {
        if let Some(inner) = weak.upgrade() {
            I18nCapability(inner).settle(code, outcome);
        }
    });
    let failed = api.0.locale_load_failed.clone();
    let on_failed = Rc::new(move |locale: &str, error: &Error| {
        failed.emit(&LocaleLoadFailedEvent {
            locale: locale.to_string(),
            error: to_plugin_error("i18n", error).plugin_error_info(),
        });
    });
    *api.0.loaders.borrow_mut() = Some(Loaders::new(cx, config, settle, on_failed));
    let connecting = api.clone();
    (api, Rc::new(move || connecting.connect()))
}

/// `createI18nCapability(ctx, config)`: the capability alone, connected at
/// once, the shape unit tests build.
pub fn create_i18n_capability(
    cx: Controller<I18nState, I18nAction>,
    config: Rc<I18nConfig>,
) -> Result<I18nCapability, Error> {
    let (api, connect) = create_i18n_controller(cx, config);
    connect()?;
    Ok(api)
}
