//! `packages/plugin/i18n` in Rust: translation as pure data over pure
//! state. Locale packs live in the store, `t()` is a pure read, and the
//! plugin is workspace-scoped with no dependencies, so its capability exists
//! from the moment the kernel is created and never touches the engine.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], [`effects`], [`translate`], [`negotiate`], and the wiring
//! in [`i18n_plugin`]. Two modules have no TypeScript file of their own
//! because they port what the TypeScript gets from its runtime: [`plural`]
//! is `Intl.PluralRules`, and [`js`] is a plain object used as a record and
//! `String(number)`.

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};

pub mod capability;
pub mod effects;
pub mod js;
pub mod negotiate;
pub mod plural;
mod plural_data;
pub mod reducer;
pub mod translate;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::I18nCapability;
pub use js::JsRecord;
pub use negotiate::negotiate_locale;
pub use reducer::{i18n_reducer, initial_i18n_state};
pub use translate::{TranslateResult, interpolate, translate};
pub use types::{
    Dir, I18N, I18nAction, I18nConfig, I18nState, Loader, Locale, LocaleInfo, Param, Params,
    TranslateOptions, TranslationDictionary, TranslationNode, WarnSink,
};

/// `i18nPlugin(config)`: workspace-scoped, no dependencies.
pub fn i18n_plugin(config: I18nConfig) -> AnyPlugin {
    let config = Rc::new(config);
    let initial = config.clone();
    let capability = config.clone();
    define::<I18nState, I18nAction>("i18n")
        .initial_state(move || initial_i18n_state(&initial))
        .reduce(i18n_reducer)
        .capability(&I18N, move |ctx| Ok(I18nCapability::new(ctx, capability.clone())))
        .effects(effects::register_i18n_effects(config))
        .build()
}
