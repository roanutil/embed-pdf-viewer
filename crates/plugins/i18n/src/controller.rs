//! `controller.ts`: reads over the state, the language verbs, the
//! `onLocaleChanged` derivation, and the loader wired at `connect`. Built
//! synchronously when the kernel is created, so `t()` works before the
//! engine exists.

use std::cell::{OnceCell, RefCell};
use std::collections::HashSet;
use std::rc::{Rc, Weak};

use epdf_kernel::cancellable::cancellable;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::Resolver;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, Error, Promise};
use serde_json::Value;

use crate::contract::{
    I18nConfig, I18nSettings, I18nSettingsChanged, Locale, LocaleChangedEvent, LocaleInfo,
    LocaleLoadFailedEvent, LocaleLoader, OperationOptions, TextDirection, TranslateOptions,
    TranslationDictionary, Translator, WarnSink,
};
use crate::js::{Inherited, JsRecord};
use crate::loaders::Loaders;
use crate::model::{self, I18nState, is_registered};
use crate::settings::LoaderTable;
use crate::translate::{TranslationSources, translate};

/// The context the controller receives, typed to the i18n state.
pub type I18nContext = Controller<I18nState, Value>;

const CAPABILITY: &str = "i18n";

fn unknown_locale(code: &str) -> Error {
    plugin_error(
        code::NOT_FOUND,
        CAPABILITY,
        format!("unknown locale '{code}': not loaded and not in loaders"),
        None,
        None,
    )
}

/// `getTranslator`'s memo inputs: the locale, the loaded languages (by
/// identity) and the fallback locale. The cache holds the inputs, as the
/// TypeScript's `memo` does, so an address is never reused while compared.
type TranslatorInputs = (String, Rc<JsRecord<Rc<Locale>>>, String);
/// `listLocales`' memo inputs: the loaded languages and the loaders, both by
/// identity, held by the cache.
type ListInputs = (Rc<JsRecord<Rc<Locale>>>, Rc<JsRecord<LocaleLoader>>);

struct Inner {
    cx: I18nContext,
    /// `ctx.settings()`: the instance's view of the registration's store.
    settings: InstanceSettings,
    /// The registration's loaders, which the store's JSON names by handle.
    table: Rc<LoaderTable>,
    /// The last settings JSON decoded, and what it decoded to.
    decoded: RefCell<Option<(Rc<Value>, Rc<I18nSettings>)>>,
    warn: WarnSink,
    locale_changed: EventHookSource<LocaleChangedEvent>,
    locale_load_failed: EventHookSource<LocaleLoadFailedEvent>,
    /// A missing key would otherwise go unnoticed, since `t()` always
    /// returns something to show: warn once per key.
    warned: RefCell<HashSet<String>>,
    /// The caller waiting for a language to load, by code; a newer switch
    /// replaces it.
    pending: RefCell<Vec<(String, Resolver<()>)>>,
    /// The locale `onLocaleChanged` announced last.
    announced: RefCell<String>,
    translator: RefCell<Option<(TranslatorInputs, Translator)>>,
    listed: RefCell<Option<(ListInputs, Rc<Vec<LocaleInfo>>)>>,
    loaders: OnceCell<Rc<Loaders>>,
}

/// `I18nCapability`. Cheap to clone; clones share state.
#[derive(Clone)]
pub struct I18nCapability(Rc<Inner>);

impl I18nCapability {
    fn state(&self) -> Rc<I18nState> {
        self.0.cx.state()
    }

    /// `ctx.state.update(transition, ...args)`.
    fn update(&self, transition: impl FnOnce(&Rc<I18nState>) -> Rc<I18nState>) {
        self.0.cx.update_state(|state| transition(&state));
    }

    /// `settings.get()`, decoded once per settings value.
    fn current_settings(&self) -> Rc<I18nSettings> {
        self.settings_of(self.0.settings.get())
    }

    /// A settings value the store held, decoded; the last one decoded is
    /// kept, so reading the same value again costs nothing.
    fn settings_of(&self, json: Rc<Value>) -> Rc<I18nSettings> {
        let mut decoded = self.0.decoded.borrow_mut();
        if let Some((cached, settings)) = &*decoded
            && Rc::ptr_eq(cached, &json)
        {
            return settings.clone();
        }
        let previous = decoded.as_ref().map(|(json, settings)| (json.as_ref(), settings.as_ref()));
        let cx = self.0.cx.clone();
        let settings =
            Rc::new(self.0.table.decode(&json, previous, &move |error| cx.report_error(&error)));
        *decoded = Some((json, settings.clone()));
        settings
    }

    /// `settings.get().loaders[code]`: an own loader, or an `Object.prototype`
    /// member, which a JavaScript record lookup also finds (and which is
    /// truthy).
    fn has_loader(&self, code: &str) -> bool {
        self.current_settings().loaders.contains_key(code) || Inherited::of(code).is_some()
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
                CAPABILITY,
                format!("switching to '{code}' was replaced"),
                None,
                None,
            ));
        }
    }

    /// `translateKey(key, options)`, the `t` every reader calls.
    fn translate_key(&self, key: &str, options: Option<&TranslateOptions>) -> String {
        let state = self.state();
        let settings = self.current_settings();
        let sources = TranslationSources {
            locale: &state.locale,
            fallback_locale: &settings.fallback_locale,
            locales: &state.locales,
        };
        let result = translate(&sources, key, options);
        let missing = match options.and_then(|o| o.fallback_key.as_deref()) {
            None => format!("\"{key}\""),
            Some(fallback_key) => format!("\"{key}\" or \"{fallback_key}\""),
        };
        if !result.found
            && options.and_then(|o| o.fallback.as_ref()).is_none()
            && self.0.warned.borrow_mut().insert(missing.clone())
        {
            (self.0.warn)(&format!(
                "[i18n] missing translation {missing} (locale: {})",
                state.locale
            ));
        }
        result.text
    }

    // ── settings (`SettingsApi<I18nSettings>`) ──

    /// `getSettings()`: the settings in effect, the same `Rc` until a
    /// setting changes.
    pub fn get_settings(&self) -> Rc<I18nSettings> {
        self.current_settings()
    }

    /// `updateSettings(changes)`: merge `changes` into the settings. A new
    /// `locale` switches to it, new `locales` are registered, and
    /// `fallback_locale` and `loaders` apply to the next lookup and the next
    /// switch. Fires `on_settings_changed` once when a setting changed.
    pub fn update_settings(&self, changes: &I18nConfig) {
        self.0.settings.store.update_settings(&self.0.table.encode(changes));
        self.0.table.release_unnamed(&self.0.settings.store);
    }

    /// How many loaders the registration's table holds (M37's guard).
    #[doc(hidden)]
    pub fn retained_loaders(&self) -> usize {
        self.0.table.retained()
    }

    /// `resetSettings()`: back to what the app registered.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// `onSettingsChanged(listener)`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&I18nSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let this = Rc::downgrade(&self.0);
        self.0.settings.events.on(move |event| {
            let Some(inner) = this.upgrade() else { return Ok(()) };
            listener(&I18nSettingsChanged {
                settings: I18nCapability(inner).settings_of(event.settings.clone()),
                changed: event.changed.clone(),
            })
        })
    }

    // ── reads ──

    /// `t(key, options)`: a string in the current language, with `params`
    /// filled in: the app's strings for the language, then EmbedPDF's own
    /// (for the key, then `options.fallback_key`), then the same for the
    /// fallback language, then `options.fallback`, then the key. A key no
    /// language has warns once.
    pub fn t(&self, key: &str, options: Option<&TranslateOptions>) -> String {
        self.translate_key(key, options)
    }

    /// `getTranslator()`: a `t` function that stays the same `Rc` until the
    /// language or the strings change, so a memoized component holding it
    /// renders again exactly when its strings may differ.
    pub fn get_translator(&self) -> Translator {
        let state = self.state();
        let inputs: TranslatorInputs = (
            state.locale.clone(),
            state.locales.clone(),
            self.current_settings().fallback_locale.clone(),
        );
        if let Some((cached, translator)) = &*self.0.translator.borrow()
            && cached.0 == inputs.0
            && Rc::ptr_eq(&cached.1, &inputs.1)
            && cached.2 == inputs.2
        {
            return translator.clone();
        }
        let this: Weak<Inner> = Rc::downgrade(&self.0);
        let translator: Translator = Rc::new(move |key, options| match this.upgrade() {
            Some(inner) => I18nCapability(inner).translate_key(key, options),
            // The plugin is gone: nothing to read but the key.
            None => key.to_string(),
        });
        *self.0.translator.borrow_mut() = Some((inputs, translator.clone()));
        translator
    }

    /// `hasKey(key)`: whether a key has a string in the current or the
    /// fallback language. Never warns.
    pub fn has_key(&self, key: &str) -> bool {
        let state = self.state();
        let settings = self.current_settings();
        let sources = TranslationSources {
            locale: &state.locale,
            fallback_locale: &settings.fallback_locale,
            locales: &state.locales,
        };
        translate(&sources, key, None).found
    }

    /// `getLocale()`: the current language's code.
    pub fn get_locale(&self) -> String {
        self.state().locale.clone()
    }

    /// `getDirection()`: how the current language reads.
    pub fn get_direction(&self) -> TextDirection {
        let state = self.state();
        state.locales.get(&state.locale).and_then(|l| l.direction).unwrap_or(TextDirection::Ltr)
    }

    /// `listLocales()`: every language you can switch to, loaded or not, in
    /// the order they were added. The same `Rc` until one changes.
    pub fn list_locales(&self) -> Rc<Vec<LocaleInfo>> {
        let state = self.state();
        let loaders = self.current_settings().loaders.clone();
        let inputs: ListInputs = (state.locales.clone(), loaders.clone());
        if let Some((cached, listed)) = &*self.0.listed.borrow()
            && Rc::ptr_eq(&cached.0, &inputs.0)
            && Rc::ptr_eq(&cached.1, &inputs.1)
        {
            return listed.clone();
        }
        let mut known: Vec<LocaleInfo> = state
            .locales
            .values()
            .map(|locale| LocaleInfo {
                code: locale.code.clone(),
                name: locale.name.clone(),
                direction: locale.direction.unwrap_or(TextDirection::Ltr),
                loaded: true,
            })
            .collect();
        for code in loaders.keys() {
            if !is_registered(&state.locales, code) {
                known.push(LocaleInfo {
                    code: code.to_string(),
                    name: code.to_string(),
                    direction: TextDirection::Ltr,
                    loaded: false,
                });
            }
        }
        let listed = Rc::new(known);
        *self.0.listed.borrow_mut() = Some((inputs, listed.clone()));
        listed
    }

    /// `getLoadingLocale()`: the code of the language being loaded.
    pub fn get_loading_locale(&self) -> Option<String> {
        self.state().loading.clone()
    }

    // ── verbs ──

    /// `setLocale(code, options)`: switch the language, loading it first
    /// when it's in `loaders`. Resolves once it shows, and fires
    /// `on_locale_changed`. Rejects `not-found` for a code that is neither
    /// loaded nor in `loaders`, `operation-failed` when it can't load (and
    /// fires `on_locale_load_failed`), and `operation-cancelled` when the
    /// signal fires or a newer call replaces it. Cancelling stops the
    /// waiting, not the fetch: the language still registers when it arrives,
    /// but doesn't become current.
    pub fn set_locale(&self, code: &str, options: OperationOptions) -> Promise<()> {
        let signal = options.signal;
        if signal.as_ref().is_some_and(|s| s.aborted()) {
            return Promise::rejected(plugin_error(
                code::OPERATION_CANCELLED,
                CAPABILITY,
                "operation cancelled",
                None,
                None,
            ));
        }
        if is_registered(&self.state().locales, code) {
            self.supersede();
            self.update(|state| model::set_locale(state, code));
            return Promise::resolved(());
        }
        if !self.has_loader(code) {
            return Promise::rejected(unknown_locale(code));
        }
        self.supersede();
        let (loaded, resolver) = Promise::pending();
        self.0.pending.borrow_mut().push((code.to_string(), resolver));
        self.update(|state| model::start_locale_load(state, code));
        if let Some(signal) = &signal {
            let this = Rc::downgrade(&self.0);
            let code = code.to_string();
            let fired = std::cell::Cell::new(false);
            signal.on_abort(Rc::new(move |_| {
                // `{ once: true }`
                if fired.replace(true) {
                    return;
                }
                let Some(inner) = this.upgrade() else { return };
                let removed = {
                    let mut pending = inner.pending.borrow_mut();
                    let before = pending.len();
                    pending.retain(|(c, _)| *c != code);
                    pending.len() != before
                };
                if removed {
                    I18nCapability(inner).update(|state| model::end_locale_load(state, &code));
                }
            }));
        }
        cancellable(CAPABILITY, signal, loaded)
    }

    /// `registerLocale(locale)`: add a whole language, and get a function
    /// that removes it again. Every call of the remover removes the code,
    /// as the TypeScript's closure does; after the instance closed, the
    /// write is dropped and reported `instance-closed` once.
    pub fn register_locale(&self, locale: Locale) -> Rc<dyn Fn()> {
        let code = locale.code.clone();
        self.update(|state| model::register_locale(state, &locale));
        // The context, not the capability: it holds no reference back to the
        // capability, so the remover keeps nothing alive but the lease.
        let cx = self.0.cx.clone();
        Rc::new(move || {
            cx.update_state(|state| model::unregister_locale(&state, &code));
        })
    }

    /// `addTranslations(code, translations)`: add strings to a language,
    /// loaded or not, replacing the ones with the same key; dotted keys
    /// work. A language in `loaders` gets them when it loads, on top of its
    /// own. Errs `not-found` for a code that is neither loaded nor in
    /// `loaders`.
    pub fn add_translations(
        &self,
        code: &str,
        translations: TranslationDictionary,
    ) -> Result<(), Error> {
        if !is_registered(&self.state().locales, code) && !self.has_loader(code) {
            return Err(unknown_locale(code));
        }
        self.update(|state| model::add_translations(state, code, &translations));
        Ok(())
    }

    // ── events ──

    /// `onLocaleChanged`: the current language changed and shows; never
    /// while a language is still loading.
    pub fn on_locale_changed(
        &self,
        listener: impl Fn(&LocaleChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.locale_changed.on(listener)
    }

    /// `onLocaleLoadFailed`: a language in `loaders` couldn't be loaded; the
    /// current language stays.
    pub fn on_locale_load_failed(
        &self,
        listener: impl Fn(&LocaleLoadFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.locale_load_failed.on(listener)
    }

    /// `followSettings()`: follow a settings change. New languages register,
    /// and a new `locale` becomes current.
    fn follow_settings(&self) {
        let previous = RefCell::new(self.current_settings());
        let this = Rc::downgrade(&self.0);
        let unsubscribe = self.0.settings.events.on(move |event| {
            let Some(inner) = this.upgrade() else { return Ok(()) };
            let api = I18nCapability(inner);
            let next = api.settings_of(event.settings.clone());
            let prior = previous.replace(next.clone());
            if event.changed.iter().any(|k| k == "locales") {
                for locale in prior.locales.iter() {
                    if !next.locales.iter().any(|other| other.code == locale.code) {
                        api.update(|state| model::unregister_locale(state, &locale.code));
                    }
                }
                // `previous.locales.includes(locale)`: the settings hold
                // plain data, so a locale is included when an equal one is
                // (README, "What is not ported").
                for locale in next.locales.iter() {
                    if !prior.locales.contains(locale) {
                        api.update(|state| model::register_locale(state, locale));
                    }
                }
            }
            if event.changed.iter().any(|k| k == "locale") {
                let code = next.locale.clone().unwrap_or_else(|| next.fallback_locale.clone());
                // A failed load fires onLocaleLoadFailed; only a code nothing
                // knows needs saying here.
                let switched = api.set_locale(&code, OperationOptions::default());
                let warn = api.0.warn.clone();
                epdf_kernel::rt::spawn(async move {
                    if let Err(error) = switched.await
                        && error.is_plugin_error(Some(code::NOT_FOUND))
                    {
                        warn(&format!("[i18n] {}", error.message));
                    }
                });
            }
            Ok(())
        });
        self.0.cx.listen(unsubscribe);
    }

    fn connect(&self) -> Result<(), Error> {
        if let Some(loaders) = self.0.loaders.get() {
            loaders.connect()?;
        }
        self.follow_settings();
        Ok(())
    }
}

/// `createI18nController(ctx)`: the api and its `connect`, which wires the
/// loader and follows the settings. `table` is the registration's loader
/// table, which the store's JSON names loaders by; `warn` is the plugin's
/// `console.warn`.
pub fn create_i18n_controller(
    cx: I18nContext,
    table: Rc<LoaderTable>,
    warn: WarnSink,
) -> Result<(I18nCapability, ConnectFn), Error> {
    let settings = cx.settings()?;
    let locale_changed = cx.event_source::<LocaleChangedEvent>();
    let locale_load_failed = cx.event_source::<LocaleLoadFailedEvent>();
    let api = I18nCapability(Rc::new(Inner {
        cx: cx.clone(),
        settings,
        table,
        decoded: RefCell::new(None),
        warn,
        locale_changed,
        locale_load_failed,
        warned: RefCell::default(),
        pending: RefCell::default(),
        announced: RefCell::new(String::new()),
        translator: RefCell::new(None),
        listed: RefCell::new(None),
        loaders: OnceCell::new(),
    }));

    let seed = api.current_settings();
    api.update(|state| {
        model::seed_from_settings(
            state,
            seed.locale.as_deref(),
            &seed.fallback_locale,
            &seed.locales,
            |code| seed.loaders.contains_key(code) || Inherited::of(code).is_some(),
        )
    });

    // A language change is announced once it shows: never while a language
    // is still loading, and always against the language announced last.
    *api.0.announced.borrow_mut() = api.state().locale.clone();
    {
        let this = Rc::downgrade(&api.0);
        cx.on_state_change(move |next, _previous| {
            let Some(inner) = this.upgrade() else { return Ok(()) };
            if next.loading.is_some() || *inner.announced.borrow() == next.locale {
                return Ok(());
            }
            let previous_locale = inner.announced.replace(next.locale.clone());
            inner
                .locale_changed
                .emit(&LocaleChangedEvent { locale: next.locale.clone(), previous_locale });
            Ok(())
        });
    }

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
            error: to_plugin_error(CAPABILITY, error).plugin_error_info(),
        });
    });
    let weak = Rc::downgrade(&api.0);
    let loader_of = Rc::new(move |code: &str| -> Option<LocaleLoader> {
        let inner = weak.upgrade()?;
        I18nCapability(inner).current_settings().loaders.get(code).cloned()
    });
    let _ = api.0.loaders.set(Loaders::new(cx, loader_of, settle, on_failed));

    let connecting = api.clone();
    Ok((api, Rc::new(move || connecting.connect())))
}
