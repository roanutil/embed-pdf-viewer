//! `effects.ts`: the plugin's one side effect, fetching lazy locale packs.
//! State-driven: a load is `state.loading`, set by the capability or by the
//! initial state, so a load requested before effects ran is picked up here.

use std::rc::Rc;

use epdf_kernel::{EffectCtx, Error};

use crate::types::{I18nAction, I18nConfig, I18nState};

pub fn register_i18n_effects(
    config: Rc<I18nConfig>,
) -> impl Fn(EffectCtx<I18nState, I18nAction>) -> Result<(), Error> + 'static {
    move |ctx: EffectCtx<I18nState, I18nAction>| {
        let load = {
            let config = config.clone();
            let ctx = ctx.ctx.clone();
            Rc::new(move |code: &str| -> Result<(), Error> {
                // Unreachable through the capability; guards bad dispatches.
                // (An `Object.prototype` member never gets here: the
                // capability and the initial state treat those codes as
                // registered, so `loading` never names one.)
                let Some(loader) = config.loader(code) else { return Ok(()) };
                let pending = loader()?;
                let ctx = ctx.clone();
                let code = code.to_string();
                // `loader().then(onLoaded, onFailed)`.
                epdf_kernel::rt::spawn(async move {
                    match pending.await {
                        Ok(locale) => {
                            let wanted = locale.code.clone();
                            ctx.dispatch(I18nAction::RegisterLocale { locale });
                            // Complete the switch only if this load is still
                            // the wanted one. A slice that is gone (the kernel
                            // was destroyed) is the TypeError that ends the
                            // TypeScript callback here.
                            let Some(state) = ctx.try_state() else { return };
                            if state.loading.as_deref() == Some(code.as_str()) {
                                ctx.dispatch(I18nAction::SetLocale { locale: wanted });
                            }
                        }
                        Err(_) => ctx.dispatch(I18nAction::LoadFailed { locale: code }),
                    }
                });
                Ok(())
            })
        };

        let pending = ctx.state().loading.clone();
        if let Some(code) = pending.filter(|c| !c.is_empty()) {
            load(&code)?;
        }
        let select = ctx.ctx.clone();
        ctx.watch(
            move || select.try_state().and_then(|s| s.loading.clone()),
            |a, b| a == b,
            move |code, _| match code {
                Some(code) if !code.is_empty() => load(code),
                _ => Ok(()),
            },
        );
        Ok(())
    }
}
