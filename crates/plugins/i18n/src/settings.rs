//! The i18n settings in the kernel's settings store.
//!
//! The kernel keeps a registration's settings as one JSON value
//! (`epdf_kernel::settings`), merged as `settings.ts` merges them: plain
//! objects key by key, arrays and other values whole. `locale`,
//! `fallbackLocale` and `locales` are plain data. A loader is a function, so
//! each is stored as a handle, `{"$i18nLoader": n}`, into a table the
//! registration owns (one per `i18n_plugin(config)`, shared by its
//! instances as the store is). The same loader reads back as the same
//! value, so giving a code the loader it has changes nothing, as the
//! TypeScript's `Object.is` merge decides.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::Error;
use serde_json::{Map, Value, json};

use crate::contract::{I18nConfig, I18nSettings, Locale, LocaleLoader};
use crate::js::JsRecord;

const LOADER_HANDLE: &str = "$i18nLoader";

/// The registration's loaders, by handle.
#[derive(Default)]
pub struct LoaderTable {
    loaders: RefCell<Vec<(u64, LocaleLoader)>>,
    next: Cell<u64>,
}

impl LoaderTable {
    fn loader_json(&self, loader: &LocaleLoader) -> Value {
        let found =
            self.loaders.borrow().iter().find(|(_, l)| Rc::ptr_eq(l, loader)).map(|(n, _)| *n);
        let n = found.unwrap_or_else(|| {
            let n = self.next.get();
            self.next.set(n + 1);
            self.loaders.borrow_mut().push((n, loader.clone()));
            n
        });
        json!({ LOADER_HANDLE: n })
    }

    fn loader_of(&self, value: &Value) -> Option<LocaleLoader> {
        let n = value.get(LOADER_HANDLE).and_then(Value::as_u64)?;
        self.loaders.borrow().iter().find(|(m, _)| *m == n).map(|(_, l)| l.clone())
    }

    /// The JSON a `DeepPartial<I18nSettings>` change is in the store: only
    /// the members it names.
    pub fn encode(&self, config: &I18nConfig) -> Value {
        let mut out = Map::new();
        if let Some(locale) = &config.locale {
            out.insert("locale".into(), json!(locale));
        }
        if let Some(fallback) = &config.fallback_locale {
            out.insert("fallbackLocale".into(), json!(fallback));
        }
        if let Some(locales) = &config.locales {
            out.insert("locales".into(), serde_json::to_value(locales).unwrap_or(Value::Null));
        }
        if let Some(loaders) = &config.loaders {
            let loaders: Map<String, Value> =
                loaders.iter().map(|(code, l)| (code.to_string(), self.loader_json(l))).collect();
            out.insert("loaders".into(), Value::Object(loaders));
        }
        Value::Object(out)
    }

    /// The settings a stored JSON value holds. `previous` is the last value
    /// decoded with what it decoded to: a list or a loader record whose JSON
    /// did not change keeps its `Rc`, as the merge keeps the object it did
    /// not change.
    ///
    /// Every write goes through [`encode`](Self::encode), so a member that
    /// doesn't decode means the store was written some other way. The
    /// TypeScript would carry on with whatever the member holds; this typed
    /// boundary can't, so it hands the failure to `report` and reads the
    /// member as empty (`locales`), absent (a loader) or `""`
    /// (`fallbackLocale`).
    pub fn decode(
        &self,
        value: &Value,
        previous: Option<(&Value, &I18nSettings)>,
        report: &dyn Fn(Error),
    ) -> I18nSettings {
        let refuse = |what: &str, found: &Value| {
            report(Error::new(format!("[i18n] settings.{what} does not decode: {found}")))
        };
        let unchanged = |member: &str| {
            previous.filter(|(json, _)| json.get(member) == value.get(member)).map(|(_, s)| s)
        };
        let locales = match unchanged("locales") {
            Some(settings) => settings.locales.clone(),
            None => {
                let raw = value.get("locales").cloned().unwrap_or(Value::Null);
                Rc::new(serde_json::from_value::<Vec<Locale>>(raw.clone()).unwrap_or_else(|_| {
                    refuse("locales", &raw);
                    Vec::new()
                }))
            }
        };
        let loaders = match unchanged("loaders") {
            Some(settings) => settings.loaders.clone(),
            None => {
                let mut record = JsRecord::new();
                match value.get("loaders").and_then(Value::as_object) {
                    Some(object) => {
                        for (code, handle) in object {
                            match self.loader_of(handle) {
                                Some(loader) => record.insert(code.clone(), loader),
                                None => refuse(&format!("loaders.{code}"), handle),
                            }
                        }
                    }
                    None => refuse("loaders", value.get("loaders").unwrap_or(&Value::Null)),
                }
                Rc::new(record)
            }
        };
        let locale = match value.get("locale") {
            Some(Value::String(locale)) => Some(locale.clone()),
            Some(Value::Null) => None,
            other => {
                refuse("locale", other.unwrap_or(&Value::Null));
                None
            }
        };
        let fallback_locale = match value.get("fallbackLocale") {
            Some(Value::String(fallback)) => fallback.clone(),
            other => {
                refuse("fallbackLocale", other.unwrap_or(&Value::Null));
                String::new()
            }
        };
        I18nSettings { locale, fallback_locale, locales, loaders }
    }
}

/// `I18N_DEFAULTS` in the store.
pub fn defaults_json() -> Value {
    json!({ "locale": null, "fallbackLocale": "en", "locales": [], "loaders": {} })
}

/// The registration's table and its registered settings.
pub fn register(config: &I18nConfig) -> (Rc<LoaderTable>, Value) {
    let table = Rc::new(LoaderTable::default());
    let registered = table.encode(config);
    (table, registered)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A store value no `encode` wrote: each member that doesn't decode is
    /// reported, never dropped quietly.
    #[test]
    fn a_member_that_does_not_decode_is_reported() {
        let table = LoaderTable::default();
        let reports: RefCell<Vec<String>> = RefCell::default();
        let settings = table.decode(
            &json!({ "locale": 7, "locales": [{ "code": "en" }], "loaders": { "ar": { "$i18nLoader": 9 } } }),
            None,
            &|error| reports.borrow_mut().push(error.message),
        );
        assert!(settings.locales.is_empty() && settings.loaders.keys().next().is_none());
        assert_eq!((settings.locale, settings.fallback_locale.as_str()), (None, ""));
        assert_eq!(
            *reports.borrow(),
            [
                "[i18n] settings.locales does not decode: [{\"code\":\"en\"}]",
                "[i18n] settings.loaders.ar does not decode: {\"$i18nLoader\":9}",
                "[i18n] settings.locale does not decode: 7",
                "[i18n] settings.fallbackLocale does not decode: null",
            ]
        );
        // What `encode` writes decodes without a report.
        reports.borrow_mut().clear();
        let mut stored = defaults_json();
        stored["loaders"] = table.encode(&I18nConfig {
            loaders: Some(
                [("ar".to_string(), Rc::new(|| Err(Error::new("x"))) as LocaleLoader)]
                    .into_iter()
                    .collect(),
            ),
            ..I18nConfig::default()
        })["loaders"]
            .clone();
        let settings =
            table.decode(&stored, None, &|error| reports.borrow_mut().push(error.message));
        assert!(reports.borrow().is_empty());
        assert!(settings.loaders.contains_key("ar"));
    }
}
