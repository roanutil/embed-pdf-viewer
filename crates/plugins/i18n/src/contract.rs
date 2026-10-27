//! `contract.ts`: translations and the current language.
//!
//! Languages are data: the packs live in the plugin's state, so `t()` is a
//! pure read and every reader follows the kernel's one change stream. The
//! plugin is workspace-scoped and needs no engine or document, so it
//! translates a loading screen and a password prompt too. It ships
//! EmbedPDF's own strings (the standard commands' labels) in eight
//! languages, under the app's ([`crate::strings`]).

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Token};
use serde::{Deserialize, Serialize};

use crate::controller::I18nCapability;
use crate::js::JsRecord;

/// A dictionary member: a leaf string (which may hold `{name}` slots) or a
/// nested dictionary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TranslationNode {
    Text(String),
    Branch(TranslationDictionary),
}

/// `TranslationDictionary`: a tree of strings. A dotted key is the same as a
/// nested one: `{ "review.reject": "Reject" }` is
/// `{ "review": { "reject": "Reject" } }`.
pub type TranslationDictionary = JsRecord<TranslationNode>;

/// `TextDirection`: how a language reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextDirection {
    Ltr,
    Rtl,
}

impl TextDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            TextDirection::Ltr => "ltr",
            TextDirection::Rtl => "rtl",
        }
    }
}

/// `Locale`: a language and its strings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    /// Its language tag (RFC 5646): `en`, `nl`, `ar`, `zh-CN`.
    pub code: String,
    /// Its name in itself, as a language picker shows it: `Nederlands`.
    pub name: String,
    /// How it reads; `ltr` when left out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<TextDirection>,
    pub translations: TranslationDictionary,
}

/// `LocaleLoader`: loads a language the first time someone switches to it.
/// `Err` is a loader that throws synchronously.
pub type LocaleLoader = Rc<dyn Fn() -> Result<Promise<Locale>, Error>>;

/// Where the plugin's `console.warn` goes. A Rust host picks the sink;
/// [`crate::i18n_plugin`] writes to stderr.
pub type WarnSink = Rc<dyn Fn(&str)>;

/// `I18nSettings`. `i18nPlugin(config)` registers them over
/// [`i18n_defaults`], and `update_settings()` changes them while the app
/// runs: a new `locale` switches to it, new `locales` are registered, and
/// `fallback_locale` and `loaders` apply to the next lookup and the next
/// switch.
#[derive(Clone)]
pub struct I18nSettings {
    /// The language to start with; `None` starts with `fallback_locale`. A
    /// language in `loaders` shows the fallback language until it has
    /// loaded.
    pub locale: Option<String>,
    /// The language to use for a string another language doesn't have.
    pub fallback_locale: String,
    /// Languages loaded from the start. The same `Rc` until they change.
    pub locales: Rc<Vec<Locale>>,
    /// Languages loaded the first time someone switches to them, by code.
    /// Until then a language picker shows the code as the name. The same
    /// `Rc` until they change.
    pub loaders: Rc<JsRecord<LocaleLoader>>,
}

/// `I18N_DEFAULTS`: what the settings are when the app registers none.
pub fn i18n_defaults() -> I18nSettings {
    I18nSettings {
        locale: None,
        fallback_locale: "en".to_string(),
        locales: Rc::default(),
        loaders: Rc::default(),
    }
}

/// `I18nConfig = DeepPartial<I18nSettings>`: what `i18nPlugin(config)` and
/// `updateSettings(changes)` take. A member left `None` leaves the setting
/// as it is; `locale: Some(None)` is `locale: null`. `loaders` merge code by
/// code; `locales` replace the list.
#[derive(Clone, Default)]
pub struct I18nConfig {
    pub locale: Option<Option<String>>,
    pub fallback_locale: Option<String>,
    pub locales: Option<Vec<Locale>>,
    pub loaders: Option<JsRecord<LocaleLoader>>,
}

/// `{name}` slot values.
#[derive(Clone, Debug, PartialEq)]
pub enum Param {
    Text(String),
    Number(f64),
}

impl From<&str> for Param {
    fn from(text: &str) -> Self {
        Param::Text(text.to_string())
    }
}

impl From<String> for Param {
    fn from(text: String) -> Self {
        Param::Text(text)
    }
}

impl From<f64> for Param {
    fn from(n: f64) -> Self {
        Param::Number(n)
    }
}

pub type Params = JsRecord<Param>;

/// `TranslateOptions`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslateOptions {
    /// The values of the string's `{name}` slots. A number `count` also
    /// picks the plural: a key whose string is `{ one, other }` gives the
    /// form for the language's plural rule, and `other` when the rule's form
    /// is missing.
    pub params: Option<Params>,
    /// Another key, read in each language that has no string for the key,
    /// before the fallback language is: a specific key in front of a
    /// general one.
    pub fallback_key: Option<String>,
    /// What to show, with its slots filled, when no language has the key;
    /// the key itself otherwise.
    pub fallback: Option<String>,
}

/// `OperationOptions`: the caller's cancellation.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `LocaleInfo`: a language as a picker shows it, the loaded ones and the
/// ones in `loaders`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocaleInfo {
    pub code: String,
    /// Its name in itself; the code until a language in `loaders` has
    /// loaded.
    pub name: String,
    pub direction: TextDirection,
    pub loaded: bool,
}

/// `LocaleChangedEvent`: the current language changed and shows.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
#[serde(rename_all = "camelCase")]
pub struct LocaleChangedEvent {
    pub locale: String,
    pub previous_locale: String,
}

/// `LocaleLoadFailedEvent`: a language in `loaders` couldn't be loaded; the
/// current language stays. `error` is `PluginErrorInfo`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocaleLoadFailedEvent {
    pub locale: String,
    pub error: serde_json::Value,
}

/// `SettingsChangedEvent<I18nSettings>`: the settings after a change, and
/// the top-level settings whose value changed.
#[derive(Clone)]
pub struct I18nSettingsChanged {
    pub settings: Rc<I18nSettings>,
    pub changed: Vec<String>,
}

/// `getTranslator()`'s function: `t` bound to the strings in effect when it
/// was made.
pub type Translator = Rc<dyn Fn(&str, Option<&TranslateOptions>) -> String>;

/// `I18nToken` (`token.ts`).
pub static I18N: LazyLock<Token<I18nCapability>> = LazyLock::new(|| {
    Token::with_hint("i18n", "add i18nPlugin() from '@embedpdf/plugin-i18n' to your plugins list")
});
