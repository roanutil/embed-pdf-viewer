//! `packages/plugin/i18n` in Rust: translations and the current language.
//! The language packs live in the store, `t()` is a pure read, and the
//! plugin is workspace-scoped with no dependencies, so its controller exists
//! from the moment the kernel is created and never touches the engine.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], [`loaders`] (`sync/loaders.ts`), [`translate`],
//! [`negotiate`], [`strings`] (EmbedPDF's own strings), and the wiring in
//! [`i18n_plugin`]. [`settings`] keeps the settings in the kernel's JSON
//! settings store. Three modules port what the TypeScript gets from its
//! runtime: [`plural`] is `Intl.PluralRules`, [`script`] is
//! `Intl.Locale#maximize().script`, and [`js`] is a plain object used as a
//! record and `String(number)`.

use std::rc::Rc;

use epdf_kernel::settings::SettingsDeclaration;
use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub mod contract;
pub mod controller;
pub mod js;
pub mod loaders;
pub mod model;
pub mod negotiate;
pub mod plural;
mod plural_data;
pub mod script;
mod script_data;
pub mod settings;
pub mod strings;
pub mod translate;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{
    I18N, I18nConfig, I18nSettings, I18nSettingsChanged, Locale, LocaleChangedEvent, LocaleInfo,
    LocaleLoadFailedEvent, LocaleLoader, OperationOptions, Param, Params, TextDirection,
    TranslateOptions, TranslationDictionary, TranslationNode, Translator, WarnSink, i18n_defaults,
};
pub use controller::{I18nCapability, I18nContext, create_i18n_controller};
pub use js::JsRecord;
pub use model::{
    I18nState, add_translations, end_locale_load, initial_i18n_state, merge_translations,
    nest_keys, register_locale, seed_from_settings, set_locale, start_locale_load,
    unregister_locale,
};
pub use negotiate::negotiate_locale;
pub use settings::LoaderTable;
pub use strings::embedpdf_strings_for;
pub use translate::{TranslateResult, TranslationSources, interpolate, translate};

/// `i18nPlugin(config)`: workspace-scoped, with no dependencies and no
/// engine calls. The capability exists from `createKernel()`, so the viewer
/// translates while the engine is still loading. `config` is the settings
/// the app registers, over [`i18n_defaults`]. The plugin's `console.warn`
/// (a key no language has, a settings `locale` nothing knows) goes to
/// stderr; [`i18n_plugin_warning_to`] picks the sink.
pub fn i18n_plugin(config: I18nConfig) -> AnyPlugin {
    i18n_plugin_warning_to(config, Rc::new(|message: &str| eprintln!("{message}")))
}

/// [`i18n_plugin`] with the plugin's `console.warn` going to `warn`.
pub fn i18n_plugin_warning_to(config: I18nConfig, warn: WarnSink) -> AnyPlugin {
    let (table, registered) = settings::register(&config);
    define::<I18nState, Value>("i18n")
        .initial_state(initial_i18n_state)
        .settings(SettingsDeclaration {
            defaults: settings::defaults_json(),
            registered: Some(registered),
            whole: Vec::new(),
        })
        .create(&I18N, move |cx| {
            let (api, connect) = create_i18n_controller(cx, table.clone(), warn.clone())?;
            Ok((api, Some(connect)))
        })
        .build()
}
