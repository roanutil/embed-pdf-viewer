//! The i18n plugin through uniffi: typed records, one workspace handle, one
//! event enum, and the lazy-pack loader as a foreign trait.
//!
//! Packs cross flat (`"zoom.in" -> "Acercar"`): uniffi has no recursive
//! records, and lookup splits keys on `.`, so a dotted key reaches what the
//! nested one did.

use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    ForeignSettle, KernelBound, KernelPost, PluginErrorInfo, RegistrationHandle, Subscription,
    ViewerError,
};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::rt::Promise;
use epdf_kernel::store::Unsubscribe;
use serde_json::Value;

use crate::contract::{
    Dir, Loader, Locale, LocaleChangedEvent, LocaleLoadFailedEvent, Param, Params,
    TranslateOptions, TranslationDictionary, TranslationNode,
};
use crate::controller::I18nCapability;
use crate::js::JsRecord;

fn invalid(message: String) -> Error {
    plugin_error(code::INVALID_INPUT, "i18n", message, None, None)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TextDirection {
    Ltr,
    Rtl,
}

impl From<Dir> for TextDirection {
    fn from(dir: Dir) -> Self {
        match dir {
            Dir::Ltr => TextDirection::Ltr,
            Dir::Rtl => TextDirection::Rtl,
        }
    }
}

impl From<TextDirection> for Dir {
    fn from(dir: TextDirection) -> Self {
        match dir {
            TextDirection::Ltr => Dir::Ltr,
            TextDirection::Rtl => Dir::Rtl,
        }
    }
}

/// A locale pack. `translations` is flat: a branch's members are spelled
/// `branch.member`, and plural forms `key.one`, `key.other` and so on.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LocalePack {
    /// BCP-47 code: `en`, `es`, `ar`, `zh-Hans`.
    pub code: String,
    /// Native display name.
    pub name: String,
    /// `None` reads as left to right.
    #[uniffi(default = None)]
    pub dir: Option<TextDirection>,
    pub translations: HashMap<String, String>,
}

fn flatten(dict: &TranslationDictionary, prefix: &str, out: &mut HashMap<String, String>) {
    for (key, node) in dict.iter() {
        let path = if prefix.is_empty() { key.to_string() } else { format!("{prefix}.{key}") };
        match node {
            TranslationNode::Text(text) => {
                out.insert(path, text.clone());
            }
            TranslationNode::Branch(branch) => flatten(branch, &path, out),
        }
    }
}

fn insert_path(
    dict: &mut TranslationDictionary,
    full: &str,
    key: &str,
    text: String,
) -> Result<(), Error> {
    let (head, rest) = match key.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (key, None),
    };
    if head.is_empty() {
        return Err(invalid(format!("translation key \"{full}\" has an empty segment")));
    }
    let conflict =
        || invalid(format!("translation key \"{full}\" is both a string and a group of strings"));
    match rest {
        None => match dict.get(head) {
            Some(TranslationNode::Branch(_)) => Err(conflict()),
            _ => {
                dict.insert(head, TranslationNode::Text(text));
                Ok(())
            }
        },
        Some(rest) => {
            let mut child = match dict.get(head) {
                None => TranslationDictionary::new(),
                Some(TranslationNode::Branch(branch)) => branch.clone(),
                Some(TranslationNode::Text(_)) => return Err(conflict()),
            };
            insert_path(&mut child, full, rest, text)?;
            dict.insert(head, TranslationNode::Branch(child));
            Ok(())
        }
    }
}

/// The nested dictionary the plugin reads. Keys are inserted in sorted
/// order, so the result doesn't depend on `HashMap` iteration.
pub fn unflatten(flat: HashMap<String, String>) -> Result<TranslationDictionary, Error> {
    let mut entries: Vec<(String, String)> = flat.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut root = TranslationDictionary::new();
    for (key, text) in entries {
        insert_path(&mut root, &key, &key, text)?;
    }
    Ok(root)
}

impl TryFrom<LocalePack> for Locale {
    type Error = Error;

    fn try_from(pack: LocalePack) -> Result<Self, Error> {
        Ok(Locale {
            code: pack.code,
            name: pack.name,
            dir: pack.dir.map(Dir::from),
            translations: unflatten(pack.translations)?,
        })
    }
}

impl From<&Locale> for LocalePack {
    fn from(locale: &Locale) -> Self {
        let mut translations = HashMap::new();
        flatten(&locale.translations, "", &mut translations);
        LocalePack {
            code: locale.code.clone(),
            name: locale.name.clone(),
            dir: locale.dir.map(TextDirection::from),
            translations,
        }
    }
}

/// Parse a locale file in the web's shape (`{code, name, dir?,
/// translations}`, nested) into a flat pack.
#[uniffi::export]
pub fn locale_pack_from_json(json: String) -> Result<LocalePack, ViewerError> {
    let locale: Locale =
        serde_json::from_str(&json).map_err(|e| invalid(format!("not a locale pack: {e}")))?;
    Ok(LocalePack::from(&locale))
}

/// A locale as a switcher sees it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LocaleInfo {
    pub code: String,
    /// The bare code until a lazy pack has loaded.
    pub name: String,
    pub direction: TextDirection,
    pub loaded: bool,
}

impl From<crate::LocaleInfo> for LocaleInfo {
    fn from(info: crate::LocaleInfo) -> Self {
        LocaleInfo {
            code: info.code,
            name: info.name,
            direction: info.dir.into(),
            loaded: info.loaded,
        }
    }
}

/// A `{slot}` value. A `Number` named `count` picks a plural form.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum I18nParam {
    Text { value: String },
    Number { value: f64 },
}

impl From<I18nParam> for Param {
    fn from(param: I18nParam) -> Self {
        match param {
            I18nParam::Text { value } => Param::Text(value),
            I18nParam::Number { value } => Param::Number(value),
        }
    }
}

fn params(map: HashMap<String, I18nParam>) -> Option<Params> {
    if map.is_empty() {
        return None;
    }
    let mut entries: Vec<(String, I18nParam)> = map.into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut params = JsRecord::new();
    for (key, value) in entries {
        params.insert(key, value.into());
    }
    Some(params)
}

/// Serves lazy packs. Called on the kernel thread: return at once, do the
/// work elsewhere, and answer through `completion` from any thread. A
/// completion dropped unanswered fails the switch with `operation-failed`.
#[uniffi::export(with_foreign)]
pub trait LocaleLoader: Send + Sync {
    fn load(&self, code: String, completion: Arc<LocaleLoadCompletion>);
}

/// The answer to one `LocaleLoader::load`. The first call wins.
#[derive(uniffi::Object)]
pub struct LocaleLoadCompletion {
    settle: ForeignSettle<Locale>,
}

#[uniffi::export]
impl LocaleLoadCompletion {
    /// A pack whose keys don't form a tree (see `LocalePack`) fails the
    /// switch with `invalid-input`.
    pub fn finish(&self, pack: LocalePack) {
        self.settle.settle(move || Locale::try_from(pack));
    }

    pub fn fail(&self, detail: String) {
        self.settle.settle(move || Err(Error::new(detail).with("code", code::OPERATION_FAILED)));
    }
}

/// `i18nPlugin(config)` as Swift configures it.
///
/// Exports `Eq` and `Hash` because `ViewerConfig` holds it: uniffi 0.32's
/// Swift generator gives a record `Equatable` and `Hashable` unless it sees
/// an object inside, and it doesn't look inside a record from another crate.
/// Without these, `ViewerConfig` wouldn't compile in Swift.
#[derive(uniffi::Record)]
#[uniffi::export(Eq, Hash)]
pub struct I18nConfig {
    /// Startup locale. May name a lazy pack. Defaults to `fallback_locale`.
    #[uniffi(default = None)]
    pub locale: Option<String>,
    /// Tried when a key misses the current locale. Defaults to `en`.
    #[uniffi(default = None)]
    pub fallback_locale: Option<String>,
    /// Packs available at once.
    #[uniffi(default = [])]
    pub locales: Vec<LocalePack>,
    /// Codes `loader` serves on demand.
    #[uniffi(default = [])]
    pub lazy_locales: Vec<String>,
    #[uniffi(default = None)]
    pub loader: Option<Arc<dyn LocaleLoader>>,
}

impl fmt::Debug for I18nConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("I18nConfig")
            .field("locale", &self.locale)
            .field("fallback_locale", &self.fallback_locale)
            .field("locales", &self.locales.iter().map(|l| &l.code).collect::<Vec<_>>())
            .field("lazy_locales", &self.lazy_locales)
            .field("loader", &self.loader.is_some())
            .finish()
    }
}

/// The loader compares by identity: two configs are equal when they hold
/// the same loader object, or neither has one.
impl PartialEq for I18nConfig {
    fn eq(&self, other: &Self) -> bool {
        let same_loader = match (&self.loader, &other.loader) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        same_loader
            && self.locale == other.locale
            && self.fallback_locale == other.fallback_locale
            && self.locales == other.locales
            && self.lazy_locales == other.lazy_locales
    }
}

impl Eq for I18nConfig {}

/// Hashes the fields that hash cheaply; equal configs still hash equal.
impl Hash for I18nConfig {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.locale.hash(state);
        self.fallback_locale.hash(state);
        self.lazy_locales.hash(state);
        self.loader.as_ref().map(|l| Arc::as_ptr(l) as *const () as usize).hash(state);
    }
}

impl I18nConfig {
    /// On the kernel thread: each lazy code becomes a `Loader` that asks the
    /// foreign loader and settles through `post`.
    pub fn into_plugin_config(self, post: KernelPost) -> Result<crate::I18nConfig, Error> {
        let locales =
            self.locales.into_iter().map(Locale::try_from).collect::<Result<Vec<_>, _>>()?;
        let loaders = match (self.lazy_locales.is_empty(), self.loader) {
            (true, _) => None,
            (false, None) => {
                return Err(invalid(format!("lazy locales {:?} need a loader", self.lazy_locales)));
            }
            (false, Some(foreign)) => {
                let mut loaders: JsRecord<Loader> = JsRecord::new();
                for code in self.lazy_locales {
                    let (foreign, post, asked) = (foreign.clone(), post.clone(), code.clone());
                    let loader: Loader = Rc::new(move || {
                        let (promise, resolver) = Promise::pending();
                        let completion = Arc::new(LocaleLoadCompletion {
                            settle: ForeignSettle::new(
                                resolver,
                                post.clone(),
                                "a LocaleLoader completion",
                            ),
                        });
                        foreign.load(asked.clone(), completion);
                        Ok(promise)
                    });
                    loaders.insert(code, loader);
                }
                Some(loaders)
            }
        };
        Ok(crate::I18nConfig {
            locale: self.locale,
            fallback_locale: self.fallback_locale,
            locales: Some(locales),
            loaders,
            warn: None,
        })
    }
}

/// `LocaleLoadFailedEvent`, with its error as the record every plugin uses.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LocaleLoadFailure {
    pub locale: String,
    pub error: PluginErrorInfo,
}

impl TryFrom<&LocaleLoadFailedEvent> for LocaleLoadFailure {
    type Error = Error;

    fn try_from(event: &LocaleLoadFailedEvent) -> Result<Self, Error> {
        let field = |key: &str| {
            event.error.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| {
                Error::named("InvalidState", format!("[i18n] load failure without \"{key}\""))
                    .with("code", "invalid-state")
            })
        };
        Ok(LocaleLoadFailure {
            locale: event.locale.clone(),
            error: PluginErrorInfo {
                code: field("code")?,
                message: field("message")?,
                capability: field("capability")?,
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum I18nEvent {
    LocaleChanged { event: LocaleChangedEvent },
    LocaleLoadFailed { event: LocaleLoadFailure },
}

/// Workspace events have no document to end with: the stream lasts as long
/// as the viewer.
#[uniffi::export(with_foreign)]
pub trait I18nEventListener: Send + Sync {
    fn on_event(&self, event: I18nEvent);
}

#[derive(uniffi::Object)]
pub struct I18nHandle {
    cap: KernelBound<I18nCapability>,
    post: KernelPost,
}

impl I18nHandle {
    /// On the kernel thread.
    pub fn new(cap: I18nCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(I18nHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl I18nHandle {
    /// Current locale, then the fallback, then `fallback`, then the key.
    pub async fn t(
        &self,
        key: String,
        params: HashMap<String, I18nParam>,
        fallback: Option<String>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call(move |c| {
                let options = TranslateOptions { params: self::params(params), fallback };
                Ok(c.t(&key, Some(&options)))
            })
            .await
    }

    pub async fn has_key(&self, key: String) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(c.has_key(&key))).await
    }

    pub async fn locale(&self) -> Result<String, ViewerError> {
        self.cap.call(|c| Ok(c.get_locale())).await
    }

    pub async fn direction(&self) -> Result<TextDirection, ViewerError> {
        self.cap.call(|c| Ok(c.get_direction().into())).await
    }

    pub async fn locales(&self) -> Result<Vec<LocaleInfo>, ViewerError> {
        self.cap.call(|c| Ok(c.list_locales().into_iter().map(LocaleInfo::from).collect())).await
    }

    pub async fn loading_locale(&self) -> Result<Option<String>, ViewerError> {
        self.cap.call(|c| Ok(c.get_loading_locale())).await
    }

    /// Resolves once the locale is usable. Rejects `not-found` for an
    /// unknown code, `operation-failed` when the pack fails to load, and
    /// `operation-cancelled` when a newer call supersedes this one.
    pub async fn set_locale(&self, code: String) -> Result<(), ViewerError> {
        self.cap.call_promise(move |c| Ok(c.set_locale(&code)), |()| ()).await
    }

    pub async fn register_locale(
        &self,
        pack: LocalePack,
    ) -> Result<Arc<RegistrationHandle>, ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let remove = c.register_locale(Locale::try_from(pack)?);
                Ok(RegistrationHandle::new(post, move || remove()))
            })
            .await
    }

    /// Merge flat keys into a registered pack; later keys win.
    pub async fn add_translations(
        &self,
        code: String,
        translations: HashMap<String, String>,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| c.add_translations(&code, unflatten(translations)?)).await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect.
    pub fn subscribe(&self, listener: Arc<dyn I18nEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let changed = {
                let listener = listener.clone();
                c.on_locale_changed(move |event| {
                    listener.on_event(I18nEvent::LocaleChanged { event: event.clone() });
                    Ok(())
                })
            };
            let failed = c.on_locale_load_failed(move |event| {
                let event = LocaleLoadFailure::try_from(event)?;
                listener.on_event(I18nEvent::LocaleLoadFailed { event });
                Ok(())
            });
            slot.fill(Unsubscribe::new(move || {
                changed.call();
                failed.call();
            }));
        });
        subscription
    }
}
