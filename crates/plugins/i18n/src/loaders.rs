//! `sync/loaders.ts`: the plugin's one side effect, fetching lazy locale
//! packs. State-driven: a load is `state.loading`, set by `set_locale` (a
//! switch to a lazy pack) or by the initial state (a lazy startup locale),
//! so a load requested before `connect` ran is picked up here and nothing is
//! lost to boot ordering.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::{Controller, Error};

use crate::contract::I18nConfig;
use crate::model::{I18nAction, I18nState};

/// A load finished: the locale is usable (`Ok`) or the pack failed (`Err`).
pub type Settle = Rc<dyn Fn(&str, Result<(), Error>)>;
/// A pack failed to load: the code and what the loader rejected with.
pub type OnFailed = Rc<dyn Fn(&str, &Error)>;

pub struct Loaders {
    cx: Controller<I18nState, I18nAction>,
    config: Rc<I18nConfig>,
    settle: Settle,
    on_failed: OnFailed,
}

impl Loaders {
    /// `createLoaders(ctx, config, {settle}, onFailed)`.
    pub fn new(
        cx: Controller<I18nState, I18nAction>,
        config: Rc<I18nConfig>,
        settle: Settle,
        on_failed: OnFailed,
    ) -> Rc<Self> {
        Rc::new(Loaders { cx, config, settle, on_failed })
    }

    fn load(&self, code: &str) -> Result<(), Error> {
        // Unreachable through the controller; guards bad dispatches. (An
        // `Object.prototype` member never gets here: the controller and the
        // initial state treat those codes as registered, so `loading` never
        // names one.)
        let Some(loader) = self.config.loader(code) else { return Ok(()) };
        let pending = loader()?;
        let cx = self.cx.clone();
        let code = code.to_string();
        let settle = self.settle.clone();
        let on_failed = self.on_failed.clone();
        // `loader().then(onLoaded, onFailed)`.
        epdf_kernel::rt::spawn(async move {
            match pending.await {
                Ok(locale) => {
                    let wanted = locale.code.clone();
                    cx.dispatch(I18nAction::RegisterLocale { locale });
                    // Complete the switch only if this load is still the
                    // wanted one: the user may have switched again meanwhile.
                    if cx.state().loading.as_deref() == Some(code.as_str()) {
                        cx.dispatch(I18nAction::SetLocale { locale: wanted });
                        settle(&code, Ok(()));
                    }
                }
                Err(error) => {
                    cx.dispatch(I18nAction::LoadFailed { locale: code.clone() });
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

    /// Watch `loading` and fetch each newly requested pack. A loader that
    /// throws synchronously throws out of here (failing the start) or out of
    /// the store listener (reported by the store).
    pub fn connect(self: &Rc<Self>) -> Result<(), Error> {
        let last: Rc<RefCell<Option<String>>> = Rc::default();
        let this = self.clone();
        let check: Rc<dyn Fn() -> Result<(), Error>> = Rc::new(move || {
            let code = this.cx.state().loading.clone();
            if *last.borrow() == code {
                return Ok(());
            }
            *last.borrow_mut() = code.clone();
            match code {
                Some(code) if !code.is_empty() => this.load(&code),
                _ => Ok(()),
            }
        });
        check()?;
        let unsubscribe = self.cx.subscribe(check);
        self.cx.cleanup_fn(move || unsubscribe.call());
        Ok(())
    }
}
