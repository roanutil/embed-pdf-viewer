//! `sync/loaders.ts`: the plugin's only side effect, fetching languages
//! from `loaders`. Driven by state: a load is `state.loading`, set by
//! `set_locale` (a switch to a language still to load) or by the seeded
//! state (a start language still to load), and `state.loading_fallback`,
//! the fallback language with a loader that hasn't run. A load asked for
//! before `connect` ran is picked up here. The language on its way shows
//! once it and the fallback language are in.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};

use crate::controller::I18nContext;
use crate::model::{
    arrive_locale, end_fallback_load, end_locale_load, is_ready, is_registered, needs_fetch,
    set_locale,
};

/// `LoadSettlers.settle`: a load finished, the language shows (`Ok`) or it
/// failed (`Err`).
pub type Settle = Rc<dyn Fn(&str, Result<(), Error>)>;
/// `onFailed(locale, error)`: what the loader rejected with.
pub type OnFailed = Rc<dyn Fn(&str, &Error)>;
/// `settings.get().loaders[code]`, the loader in effect now.
pub type LoaderOf = Rc<dyn Fn(&str) -> Option<crate::contract::LocaleLoader>>;
/// `settings.get().loaders[code] !== undefined`, which an
/// `Object.prototype` member also passes.
pub type HasLoader = Rc<dyn Fn(&str) -> bool>;

pub struct Loaders {
    cx: I18nContext,
    loader_of: LoaderOf,
    has_loader: HasLoader,
    settle: Settle,
    on_failed: OnFailed,
    /// The languages whose loader is running: one fetch serves the switch
    /// and the fallback.
    fetching: RefCell<HashSet<String>>,
}

impl Loaders {
    /// `createLoaders(ctx, settings, {settle}, onFailed)`.
    pub fn new(
        cx: I18nContext,
        loader_of: LoaderOf,
        has_loader: HasLoader,
        settle: Settle,
        on_failed: OnFailed,
    ) -> Rc<Self> {
        Rc::new(Loaders {
            cx,
            loader_of,
            has_loader,
            settle,
            on_failed,
            fetching: RefCell::default(),
        })
    }

    /// `showWhenReady()`: show the language on its way once it can: the
    /// user may have switched again meanwhile.
    fn show_when_ready(&self) {
        let state = self.cx.state();
        let Some(code) = state.loading.clone() else { return };
        if !is_ready(&state, &*self.has_loader, &code) {
            return;
        }
        self.cx.update_state(|state| set_locale(&state, &code));
        (self.settle)(&code, Ok(()));
    }

    /// `load(code)`. A loader that throws synchronously throws out of here.
    fn load(self: &Rc<Self>, code: &str) -> Result<(), Error> {
        if self.fetching.borrow().contains(code) {
            return Ok(());
        }
        let loader = (self.loader_of)(code);
        let wanted = needs_fetch(&self.cx.state(), &*self.has_loader, code);
        let Some(loader) = loader.filter(|_| wanted) else {
            // Nothing to fetch: the language is in, or its loader went away
            // after it was asked for.
            if !is_registered(&self.cx.state().locales, code) {
                self.cx.update_state(|state| end_locale_load(&state, code));
                self.cx.update_state(|state| end_fallback_load(&state, code));
            }
            self.show_when_ready();
            return Ok(());
        };
        self.fetching.borrow_mut().insert(code.to_string());
        // A loader that throws synchronously leaves its code fetching, as
        // the TypeScript's `fetching.add(code)` before `loader()` does.
        let pending = loader()?;
        let this = self.clone();
        let code = code.to_string();
        // `loader().then(onLoaded, onFailed)`.
        epdf_kernel::rt::spawn(async move {
            let outcome = pending.await;
            this.fetching.borrow_mut().remove(&code);
            match outcome {
                Ok(locale) => {
                    this.cx.update_state(|state| arrive_locale(&state, &code, &locale));
                    this.show_when_ready();
                }
                Err(error) => {
                    this.cx.update_state(|state| end_locale_load(&state, &code));
                    this.cx.update_state(|state| end_fallback_load(&state, &code));
                    (this.on_failed)(&code, &error);
                    (this.settle)(
                        &code,
                        Err(plugin_error(
                            code::OPERATION_FAILED,
                            "i18n",
                            format!("locale '{code}' failed to load"),
                            None,
                            Some(to_plugin_error("i18n", &error).plugin_error_info()),
                        )),
                    );
                    // Without its fallback language, the language on its way
                    // shows with what it has.
                    this.show_when_ready();
                }
            }
        });
        Ok(())
    }

    /// `connect()`: fetch the languages the state asks for at connect, and
    /// each one it asks for later (`ctx.watch` over `loadingFallback` and
    /// `loading`). A loader that throws synchronously throws out of here
    /// (failing the start) or out of the store listener (which the store
    /// reports).
    pub fn connect(self: &Rc<Self>) -> Result<(), Error> {
        let load_requested = {
            let this = self.clone();
            Rc::new(move |code: &Option<String>| match code.as_deref() {
                Some(code) if !code.is_empty() => this.load(code),
                _ => {
                    this.show_when_ready();
                    Ok(())
                }
            })
        };
        load_requested(&self.cx.state().loading_fallback)?;
        load_requested(&self.cx.state().loading)?;
        let (this, requested) = (self.clone(), load_requested.clone());
        self.cx.watch(
            move || Ok(this.cx.state().loading_fallback.clone()),
            move |next, _| requested(next),
            |a, b| a == b,
        )?;
        let this = self.clone();
        self.cx.watch(
            move || Ok(this.cx.state().loading.clone()),
            move |next, _| load_requested(next),
            |a, b| a == b,
        )?;
        Ok(())
    }
}
