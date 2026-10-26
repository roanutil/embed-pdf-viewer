//! `sync/loaders.ts`: the plugin's only side effect, fetching languages
//! from `loaders`. Driven by state: a load is `state.loading`, set by
//! `set_locale` (a switch to a language still to load) or by the seeded
//! state (a start language still to load), so a load asked for before
//! `connect` ran is picked up here.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};

use crate::controller::I18nContext;
use crate::model::{end_locale_load, register_locale, set_locale};

/// `LoadSettlers.settle`: a load finished, the language shows (`Ok`) or it
/// failed (`Err`).
pub type Settle = Rc<dyn Fn(&str, Result<(), Error>)>;
/// `onFailed(locale, error)`: what the loader rejected with.
pub type OnFailed = Rc<dyn Fn(&str, &Error)>;
/// `settings.get().loaders[code]`, the loader in effect now.
pub type LoaderOf = Rc<dyn Fn(&str) -> Option<crate::contract::LocaleLoader>>;

pub struct Loaders {
    cx: I18nContext,
    loader_of: LoaderOf,
    settle: Settle,
    on_failed: OnFailed,
}

impl Loaders {
    /// `createLoaders(ctx, settings, {settle}, onFailed)`.
    pub fn new(
        cx: I18nContext,
        loader_of: LoaderOf,
        settle: Settle,
        on_failed: OnFailed,
    ) -> Rc<Self> {
        Rc::new(Loaders { cx, loader_of, settle, on_failed })
    }

    /// `load(code)`. A loader that throws synchronously throws out of here.
    fn load(&self, code: &str) -> Result<(), Error> {
        let Some(loader) = (self.loader_of)(code) else {
            // The loader went away after the switch asked for it.
            self.cx.update_state(|state| end_locale_load(&state, code));
            return Ok(());
        };
        let pending = loader()?;
        let cx = self.cx.clone();
        let code = code.to_string();
        let settle = self.settle.clone();
        let on_failed = self.on_failed.clone();
        // `loader().then(onLoaded, onFailed)`.
        epdf_kernel::rt::spawn(async move {
            match pending.await {
                Ok(locale) => {
                    cx.update_state(|state| register_locale(&state, &locale));
                    // Switch only if this load is still the wanted one: the
                    // user may have switched again while the language was on
                    // its way.
                    if cx.state().loading.as_deref() == Some(code.as_str()) {
                        cx.update_state(|state| set_locale(&state, &locale.code));
                        settle(&code, Ok(()));
                    }
                }
                Err(error) => {
                    cx.update_state(|state| end_locale_load(&state, &code));
                    on_failed(&code, &error);
                    settle(
                        &code,
                        Err(plugin_error(
                            code::OPERATION_FAILED,
                            "i18n",
                            format!("locale '{code}' failed to load"),
                            None,
                            Some(to_plugin_error("i18n", &error).plugin_error_info()),
                        )),
                    );
                }
            }
        });
        Ok(())
    }

    /// `connect()`: fetch the language `loading` names now, and each one
    /// asked for after (`ctx.watch` over `loading`). A loader that throws
    /// synchronously throws out of here (failing the start) or out of the
    /// store listener (which the store reports).
    pub fn connect(self: &Rc<Self>) -> Result<(), Error> {
        let load_requested = {
            let this = self.clone();
            move |code: &Option<String>| match code.as_deref() {
                Some(code) if !code.is_empty() => this.load(code),
                _ => Ok(()),
            }
        };
        load_requested(&self.cx.state().loading)?;
        let previous = RefCell::new(self.cx.state().loading.clone());
        let this = self.clone();
        let unsubscribe = self.cx.subscribe(Rc::new(move || {
            let next = this.cx.state().loading.clone();
            if *previous.borrow() == next {
                return Ok(());
            }
            *previous.borrow_mut() = next.clone();
            load_requested(&next)
        }));
        self.cx.listen(unsubscribe);
        Ok(())
    }
}
