//! The i18n plugin through uniffi: typed records, one workspace handle, one
//! event enum, and the lazy-pack loader as a foreign trait.
//!
//! Packs cross flat (`"zoom.in" -> "Acercar"`): uniffi has no recursive
//! records, and lookup splits keys on `.`, so a dotted key reaches what the
//! nested one did.

use std::collections::HashMap;

use epdf_ffi::ViewerError;
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};

use crate::contract::{Dir, Locale, Param, Params, TranslationDictionary, TranslationNode};
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

#[allow(dead_code)] // the handle's `t` reads it (next commit)
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
