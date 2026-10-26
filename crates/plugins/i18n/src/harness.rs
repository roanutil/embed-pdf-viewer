//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the function suites call.
//!
//! A vector's config is JSON, so two members arrive as data: each loader is
//! a `$fn` spec ([`Harness::callback`]), and the plugin's `console.warn` is
//! logged as the callback `console.warn`, which is where the generator
//! records the TypeScript's `console.warn`.
//!
//! The runner reads a vector with `serde_json` and `preserve_order` (this
//! crate declares it), so a config's `loaders` arrive as written, which is
//! the TypeScript's enumeration order (integer-like keys first). The
//! generator also refuses loaders not written in key order, a guard from
//! when objects were read sorted.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined, undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::contract::{LocaleLoader, OperationOptions, Param, Params, TranslateOptions};
use crate::js::{Inherited, JsRecord};
use crate::model::{self, I18nState};
use crate::{
    I18N, I18nCapability, I18nConfig, Locale, TranslationDictionary, TranslationSources,
    i18n_plugin_warning_to, initial_i18n_state, interpolate, negotiate_locale, translate,
};

pub struct I18n;

/// The TypeScript object literal's member order: the settings calls spread
/// first, then the plugin's own.
const MEMBERS: &[&str] = &[
    "getSettings",
    "updateSettings",
    "resetSettings",
    "onSettingsChanged",
    "t",
    "getTranslator",
    "hasKey",
    "getLocale",
    "getDirection",
    "listLocales",
    "getLoadingLocale",
    "setLocale",
    "registerLocale",
    "addTranslations",
    "onLocaleChanged",
    "onLocaleLoadFailed",
];

/// Subscribe a scripted `$fn` listener; the outcome is the unsubscribe.
fn listen<E: serde::Serialize>(
    h: &Harness,
    spec: Option<&Value>,
    subscribe: impl FnOnce(Box<dyn Fn(&E) -> Result<(), Error>>) -> epdf_kernel::Unsubscribe,
) -> Outcome {
    let callback = h.callback(spec.unwrap_or(&Value::Null));
    let off = subscribe(Box::new(move |event: &E| {
        match callback.call(vec![serde_json::to_value(event).unwrap_or(Value::Null)]) {
            Answer::Throws(error) => Err(error),
            _ => Ok(()),
        }
    }));
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn bad(what: &str, e: impl std::fmt::Display) -> Error {
    Error::new(format!("[i18n harness] {what}: {e}"))
}

fn decode<T: DeserializeOwned>(what: &str, value: &Value) -> Result<T, Error> {
    serde_json::from_value(scrub(value)).map_err(|e| bad(what, e))
}

/// An argument, with an explicit `undefined` read as absent.
fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn number(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    match value.get("$number").and_then(Value::as_str)? {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

fn params(value: &Value) -> Result<Params, Error> {
    let object = value.as_object().ok_or_else(|| bad("params", "not an object"))?;
    let mut out = JsRecord::new();
    for (key, member) in object {
        if is_undefined(member) {
            continue;
        }
        let param = match member {
            Value::String(text) => Param::Text(text.clone()),
            other => Param::Number(number(other).ok_or_else(|| bad("param", other))?),
        };
        out.insert(key.clone(), param);
    }
    Ok(out)
}

fn options(value: Option<&Value>) -> Result<Option<TranslateOptions>, Error> {
    let Some(value) = value.filter(|v| !v.is_null()) else { return Ok(None) };
    let params = match value.get("params").filter(|p| !is_undefined(p)) {
        Some(p) => Some(params(p)?),
        None => None,
    };
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
    Ok(Some(TranslateOptions {
        params,
        fallback_key: text("fallbackKey"),
        fallback: text("fallback"),
    }))
}

/// A `Promise<Value>` read as a `Promise<Locale>` at the moment it settles,
/// so the conversion costs no turn.
fn locale_promise(source: Promise<Value>) -> Promise<Locale> {
    let (promise, resolver) = Promise::pending();
    source.on_settle(move |result| {
        resolver.settle(result.clone().and_then(|v| decode("loaded pack", &v)));
    });
    promise
}

/// A vector's `I18nConfig`: the settings members it names, each loader a
/// `$fn` spec.
fn config(raw: &Value, h: &Harness) -> Result<I18nConfig, Error> {
    let mut config = I18nConfig::default();
    if is_undefined(raw) || raw.is_null() {
        return Ok(config);
    }
    match raw.get("locale") {
        Some(Value::String(locale)) => config.locale = Some(Some(locale.clone())),
        Some(Value::Null) => config.locale = Some(None),
        _ => {}
    }
    config.fallback_locale = raw.get("fallbackLocale").and_then(Value::as_str).map(str::to_string);
    if let Some(locales) = raw.get("locales").filter(|v| !is_undefined(v)) {
        config.locales = Some(decode("locales", locales)?);
    }
    if let Some(loaders) = raw.get("loaders").and_then(Value::as_object) {
        let mut out: JsRecord<LocaleLoader> = JsRecord::new();
        for (code, spec) in loaders {
            let callback = h.callback(spec);
            out.insert(
                code.clone(),
                Rc::new(move || match callback.call(Vec::new()) {
                    Answer::Throws(error) => Err(error),
                    Answer::Promise(promise) => Ok(locale_promise(promise)),
                    Answer::Returns(value) => Ok(locale_promise(Promise::resolved(value))),
                }) as LocaleLoader,
            );
        }
        config.loaders = Some(out);
    }
    Ok(config)
}

impl HarnessPlugin for I18n {
    fn factories(&self) -> Vec<&'static str> {
        vec!["i18n"]
    }

    fn build(&self, _factory: &str, raw: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        let warn = h.clone();
        Ok(i18n_plugin_warning_to(
            config(raw, h)?,
            Rc::new(move |message: &str| warn.log("console.warn", vec![json!(message)])),
        ))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![I18N.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<I18nCapability>()?;
        let text = |i: usize| arg(&args, i).and_then(Value::as_str).unwrap_or_default().to_string();
        Some(match method {
            "t" => match options(arg(&args, 1)) {
                Ok(options) => Outcome::Returned(json!(cap.t(&text(0), options.as_ref()))),
                Err(error) => Outcome::Threw(error),
            },
            "hasKey" => Outcome::Returned(json!(cap.has_key(&text(0)))),
            "getLocale" => Outcome::Returned(json!(cap.get_locale())),
            "getDirection" => Outcome::Returned(json!(cap.get_direction().as_str())),
            "listLocales" => {
                Outcome::Returned(serde_json::to_value(&*cap.list_locales()).unwrap_or(Value::Null))
            }
            "getLoadingLocale" => Outcome::Returned(json!(cap.get_loading_locale())),
            // No vector passes a signal.
            "setLocale" => {
                Outcome::promise_unit(cap.set_locale(&text(0), OperationOptions::default()))
            }
            "registerLocale" => {
                match decode::<Locale>("locale", arg(&args, 0).unwrap_or(&Value::Null)) {
                    Ok(locale) => {
                        let remove = cap.register_locale(locale);
                        Outcome::Returned(h.register_fn(Rc::new(move |_| {
                            remove();
                            Outcome::undefined()
                        })))
                    }
                    Err(error) => Outcome::Threw(error),
                }
            }
            "addTranslations" => {
                match decode::<TranslationDictionary>(
                    "dictionary",
                    arg(&args, 1).unwrap_or(&Value::Null),
                ) {
                    Ok(dictionary) => Outcome::from_result(
                        cap.add_translations(&text(0), dictionary).map(|()| undefined()),
                    ),
                    Err(error) => Outcome::Threw(error),
                }
            }
            "updateSettings" => match config(arg(&args, 0).unwrap_or(&undefined()), h) {
                Ok(changes) => {
                    cap.update_settings(&changes);
                    Outcome::undefined()
                }
                Err(error) => Outcome::Threw(error),
            },
            "resetSettings" => {
                cap.reset_settings();
                Outcome::undefined()
            }
            "onLocaleChanged" => listen(h, arg(&args, 0), |f| cap.on_locale_changed(f)),
            "onLocaleLoadFailed" => listen(h, arg(&args, 0), |f| cap.on_locale_load_failed(f)),
            // Their answers hold functions (the loaders, the translator),
            // which no vector reads.
            "getSettings" | "onSettingsChanged" | "getTranslator" => Outcome::Threw(bad(
                method,
                "returns functions the replay does not print; no vector calls it",
            )),
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<I18nCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── the function table ────────────────────────────────────────────────────────

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|e| bad("serialize", e))
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

fn state_arg(args: &[Value]) -> Result<Rc<I18nState>, Error> {
    decode("state", arg(args, 0).unwrap_or(&Value::Null)).map(Rc::new)
}

/// `TranslationSources`, as the vector writes it.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Sources {
    locale: String,
    fallback_locale: String,
    locales: JsRecord<Rc<Locale>>,
}

fn f_translate(args: &[Value]) -> Result<Value, Error> {
    let sources: Sources = decode("sources", arg(args, 0).unwrap_or(&Value::Null))?;
    let key = arg(args, 1).and_then(Value::as_str).unwrap_or_default();
    let options = options(arg(args, 2))?;
    let sources = TranslationSources {
        locale: &sources.locale,
        fallback_locale: &sources.fallback_locale,
        locales: &sources.locales,
    };
    to_json(&translate(&sources, key, options.as_ref()))
}

fn f_interpolate(args: &[Value]) -> Result<Value, Error> {
    let text = arg(args, 0).and_then(Value::as_str).unwrap_or_default();
    let params = match arg(args, 1) {
        Some(p) => Some(params(p)?),
        None => None,
    };
    Ok(json!(interpolate(text, params.as_ref())))
}

fn f_negotiate(args: &[Value]) -> Result<Value, Error> {
    let available = string_list(arg(args, 0));
    let requested = string_list(arg(args, 1));
    Ok(json!(negotiate_locale(&available, &requested)))
}

fn f_initial(_args: &[Value]) -> Result<Value, Error> {
    to_json(&initial_i18n_state())
}

/// `seedFromSettings(state, settings)` reads only whether a loader is there
/// (`!== undefined`), so the vector's loaders are placeholders.
fn f_seed(args: &[Value]) -> Result<Value, Error> {
    let state = state_arg(args)?;
    let settings = arg(args, 1).cloned().unwrap_or_else(|| json!({}));
    let locales: Vec<Locale> = match settings.get("locales") {
        Some(locales) => decode("locales", locales)?,
        None => Vec::new(),
    };
    let fallback = settings.get("fallbackLocale").and_then(Value::as_str).unwrap_or_default();
    let loaders = settings.get("loaders").and_then(Value::as_object);
    let has_loader = |code: &str| {
        loaders.and_then(|l| l.get(code)).is_some_and(|v| !is_undefined(v))
            || Inherited::of(code).is_some()
    };
    let locale = settings.get("locale").and_then(Value::as_str);
    to_json(&*model::seed_from_settings(&state, locale, fallback, &locales, has_loader))
}

fn f_merge(args: &[Value]) -> Result<Value, Error> {
    let base: TranslationDictionary = decode("base", arg(args, 0).unwrap_or(&Value::Null))?;
    let patch: TranslationDictionary = decode("patch", arg(args, 1).unwrap_or(&Value::Null))?;
    to_json(&model::merge_translations(&base, &patch))
}

fn f_nest(args: &[Value]) -> Result<Value, Error> {
    let dictionary: TranslationDictionary =
        decode("dictionary", arg(args, 0).unwrap_or(&Value::Null))?;
    to_json(&model::nest_keys(&dictionary))
}

fn code_arg(args: &[Value]) -> &str {
    arg(args, 1).and_then(Value::as_str).unwrap_or_default()
}

fn f_set_locale(args: &[Value]) -> Result<Value, Error> {
    to_json(&*model::set_locale(&state_arg(args)?, code_arg(args)))
}

fn f_register(args: &[Value]) -> Result<Value, Error> {
    let locale: Locale = decode("locale", arg(args, 1).unwrap_or(&Value::Null))?;
    to_json(&*model::register_locale(&state_arg(args)?, &locale))
}

fn f_unregister(args: &[Value]) -> Result<Value, Error> {
    to_json(&*model::unregister_locale(&state_arg(args)?, code_arg(args)))
}

fn f_add(args: &[Value]) -> Result<Value, Error> {
    let dictionary: TranslationDictionary =
        decode("dictionary", arg(args, 2).unwrap_or(&Value::Null))?;
    to_json(&*model::add_translations(&state_arg(args)?, code_arg(args), &dictionary))
}

fn f_start(args: &[Value]) -> Result<Value, Error> {
    to_json(&*model::start_locale_load(&state_arg(args)?, code_arg(args)))
}

fn f_end(args: &[Value]) -> Result<Value, Error> {
    to_json(&*model::end_locale_load(&state_arg(args)?, code_arg(args)))
}

/// `translate` over a pack whose branch names every CLDR category.
fn f_plurals(args: &[Value]) -> Result<Value, Error> {
    let locale = arg(args, 0).and_then(Value::as_str).unwrap_or_default();
    let dictionary = json!({ "k": { "zero": "zero", "one": "one", "two": "two", "few": "few", "many": "many", "other": "other" } });
    let pack = Locale {
        code: locale.into(),
        name: locale.into(),
        direction: None,
        translations: decode("pack", &dictionary)?,
    };
    let locales: JsRecord<Rc<Locale>> = [(locale.to_string(), Rc::new(pack))].into_iter().collect();
    let sources = TranslationSources { locale, fallback_locale: locale, locales: &locales };
    let counts = arg(args, 1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for count in &counts {
        let n = number(count).ok_or_else(|| bad("count", count))?;
        let options = TranslateOptions {
            params: Some([("count".to_string(), Param::Number(n))].into_iter().collect()),
            ..Default::default()
        };
        out.push(json!(translate(&sources, "k", Some(&options)).text));
    }
    Ok(Value::Array(out))
}

pub fn functions() -> FunctionTable {
    vec![
        ("translate", f_translate),
        ("interpolate", f_interpolate),
        ("negotiateLocale", f_negotiate),
        ("initialI18nState", f_initial),
        ("seedFromSettings", f_seed),
        ("mergeTranslations", f_merge),
        ("nestKeys", f_nest),
        ("setLocale", f_set_locale),
        ("registerLocale", f_register),
        ("unregisterLocale", f_unregister),
        ("addTranslations", f_add),
        ("startLocaleLoad", f_start),
        ("endLocaleLoad", f_end),
        ("plurals", f_plurals),
    ]
}
