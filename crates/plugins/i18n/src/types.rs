//! `types.ts`: the contract. Locale packs live in the store, so `t()` is a
//! pure read and the whole state serializes.

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, Token};
use serde::{Deserialize, Serialize};

use crate::capability::I18nCapability;
use crate::js::JsRecord;

/// A dictionary member: a leaf string (interpolating `{param}` slots) or a
/// nested dictionary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TranslationNode {
    Text(String),
    Branch(TranslationDictionary),
}

/// `TranslationDictionary`: a nested tree of translation strings.
pub type TranslationDictionary = JsRecord<TranslationNode>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Ltr,
    Rtl,
}

impl Dir {
    pub fn as_str(self) -> &'static str {
        match self {
            Dir::Ltr => "ltr",
            Dir::Rtl => "rtl",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Locale {
    /// BCP-47 code: 'en', 'es', 'ar', 'zh-Hans'.
    pub code: String,
    /// Native display name: what a locale switcher shows.
    pub name: String,
    /// Text direction for viewer chrome; `ltr` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<Dir>,
    pub translations: TranslationDictionary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct I18nState {
    pub locale: String,
    pub fallback_locale: String,
    /// Registered packs by code: the lookup table `t()` reads.
    pub locales: JsRecord<Locale>,
    /// Code of a lazy pack being fetched.
    pub loading: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum I18nAction {
    /// Switch to a registered locale; the reducer ignores unregistered codes.
    #[serde(rename = "I18N/SET_LOCALE")]
    SetLocale { locale: String },
    #[serde(rename = "I18N/REGISTER_LOCALE")]
    RegisterLocale { locale: Locale },
    #[serde(rename = "I18N/LOAD_STARTED")]
    LoadStarted { locale: String },
    #[serde(rename = "I18N/LOAD_FAILED")]
    LoadFailed { locale: String },
}

/// A lazy pack: `() => Promise<Locale>`. `Err` is a loader that throws
/// synchronously.
pub type Loader = Rc<dyn Fn() -> Result<Promise<Locale>, Error>>;

/// Where the capability's warn-once signal goes. The TypeScript writes to
/// `console.warn`; a Rust host picks the sink.
pub type WarnSink = Rc<dyn Fn(&str)>;

#[derive(Clone, Default)]
pub struct I18nConfig {
    /// Startup locale. May name a `loaders` pack: the fallback chain shows
    /// until the pack arrives. Defaults to `fallback_locale`.
    pub locale: Option<String>,
    /// The pack tried when a key misses the current locale. Default 'en'.
    pub fallback_locale: Option<String>,
    /// Eagerly available packs.
    pub locales: Option<Vec<Locale>>,
    /// Lazy packs: code → loader.
    pub loaders: Option<JsRecord<Loader>>,
    /// The `console.warn` of this port. `None` writes to stderr.
    pub warn: Option<WarnSink>,
}

impl I18nConfig {
    /// `config.loaders?.[code]`, own keys only. (An inherited
    /// `Object.prototype` member is never reached: see [`crate::effects`].)
    pub fn loader(&self, code: &str) -> Option<&Loader> {
        self.loaders.as_ref()?.get(code)
    }

    pub(crate) fn warn(&self, message: &str) {
        match &self.warn {
            Some(sink) => sink(message),
            None => eprintln!("{message}"),
        }
    }
}

/// A `{slot}` interpolation value.
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslateOptions {
    /// `{slot}` values. A number `count` picks a plural branch.
    pub params: Option<Params>,
    /// Returned (interpolated) when the key misses every pack.
    pub fallback: Option<String>,
}

/// A locale as a switcher sees it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LocaleInfo {
    pub code: String,
    /// Native name; the bare code until a lazy pack has loaded.
    pub name: String,
    pub dir: Dir,
    pub loaded: bool,
}

pub static I18N: LazyLock<Token<I18nCapability>> = LazyLock::new(|| Token::new("i18n"));
