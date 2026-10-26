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
    pub fn decode(&self, value: &Value, previous: Option<(&Value, &I18nSettings)>) -> I18nSettings {
        let unchanged = |member: &str| {
            previous.filter(|(json, _)| json.get(member) == value.get(member)).map(|(_, s)| s)
        };
        let locales = match unchanged("locales") {
            Some(settings) => settings.locales.clone(),
            None => Rc::new(
                value
                    .get("locales")
                    .and_then(|v| serde_json::from_value::<Vec<Locale>>(v.clone()).ok())
                    .unwrap_or_default(),
            ),
        };
        let loaders = match unchanged("loaders") {
            Some(settings) => settings.loaders.clone(),
            None => {
                let mut record = JsRecord::new();
                for (code, handle) in
                    value.get("loaders").and_then(Value::as_object).into_iter().flatten()
                {
                    if let Some(loader) = self.loader_of(handle) {
                        record.insert(code.clone(), loader);
                    }
                }
                Rc::new(record)
            }
        };
        I18nSettings {
            locale: value.get("locale").and_then(Value::as_str).map(str::to_string),
            fallback_locale: value
                .get("fallbackLocale")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            locales,
            loaders,
        }
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
