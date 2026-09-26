//! `packages/plugin/i18n` in Rust: translation as pure data over pure
//! state. Locale packs live in the store, `t()` is a pure read, and the
//! plugin is workspace-scoped with no dependencies, so its controller exists
//! from the moment the kernel is created and never touches the engine.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], [`loaders`] (`sync/loaders.ts`), [`translate`],
//! [`negotiate`], and the wiring in [`i18n_plugin`]. Two modules have no
//! TypeScript file of their own because they port what the TypeScript gets
//! from its runtime: [`plural`] is `Intl.PluralRules`, and [`js`] is a plain
//! object used as a record and `String(number)`.

use std::rc::Rc;

use epdf_kernel::{AnyPlugin, define};

pub mod contract;
pub mod controller;
pub mod js;
pub mod loaders;
pub mod model;
pub mod negotiate;
pub mod plural;
mod plural_data;
pub mod translate;

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{
    Dir, I18N, I18nConfig, Loader, Locale, LocaleChangedEvent, LocaleInfo, LocaleLoadFailedEvent,
    Param, Params, TranslateOptions, TranslationDictionary, TranslationNode, WarnSink,
};
pub use controller::{I18nCapability, create_i18n_capability, create_i18n_controller};
pub use js::JsRecord;
pub use model::{I18nAction, I18nState, i18n_reducer, initial_i18n_state, merge_translations};
pub use negotiate::negotiate_locale;
pub use translate::{TranslateResult, interpolate, translate};

/// `i18nPlugin(config)`: workspace-scoped (locale is a workspace concern)
/// with no dependencies. Its controller is built when the kernel is created,
/// so the shell translates from the first frame.
pub fn i18n_plugin(config: I18nConfig) -> AnyPlugin {
    let config = Rc::new(config);
    let initial = config.clone();
    define::<I18nState, I18nAction>("i18n")
        .initial_state(move || initial_i18n_state(&initial))
        .reduce(i18n_reducer)
        .create(&I18N, move |cx| {
            let (api, connect) = create_i18n_controller(cx, config.clone());
            Ok((api, Some(connect)))
        })
        .build()
}
