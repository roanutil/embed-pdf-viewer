//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, and the function
//! table the function suites call.
//!
//! A vector's config is JSON, so two members arrive as data: each loader is
//! a `$fn` spec ([`Harness::callback`]), and the warn sink logs every warning
//! as the callback `console.warn`, which is where the generator records the
//! TypeScript's `console.warn`.
//!
//! The runner reads a vector with `serde_json`, whose objects are sorted by
//! key, so a config's `loaders` arrive in key order rather than as written.
//! The generator refuses a vector whose loaders are not written in key order,
//! so the two agree (integer-like keys enumerate first either way).

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Error, is_undefined};
use plugin_conformance::{Answer, FunctionTable, Harness, HarnessPlugin, Outcome, scrub};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::js::JsRecord;
use crate::types::{Loader, Param, Params, TranslateOptions};
use crate::{
    I18N, I18nAction, I18nCapability, I18nConfig, I18nState, Locale, i18n_plugin, i18n_reducer,
    initial_i18n_state, interpolate, negotiate_locale, translate,
};

pub struct I18n;

/// The TypeScript object literal's member order.
const MEMBERS: &[&str] =
    &["t", "locale", "dir", "locales", "loading", "setLocale", "registerLocale"];

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
    let Some(value) = value else { return Ok(None) };
    let params = match value.get("params").filter(|p| !is_undefined(p)) {
        Some(p) => Some(params(p)?),
        None => None,
    };
    let fallback = value.get("fallback").and_then(Value::as_str).map(str::to_string);
    Ok(Some(TranslateOptions { params, fallback }))
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

fn config(raw: &Value, h: &Harness) -> Result<I18nConfig, Error> {
    let warn = h.clone();
    let mut config = I18nConfig {
        warn: Some(Rc::new(move |message: &str| warn.log("console.warn", vec![json!(message)]))),
        ..Default::default()
    };
    if is_undefined(raw) {
        return Ok(config);
    }
    config.locale = raw.get("locale").and_then(Value::as_str).map(str::to_string);
    config.fallback_locale = raw.get("fallbackLocale").and_then(Value::as_str).map(str::to_string);
    if let Some(locales) = raw.get("locales") {
        config.locales = Some(decode("locales", locales)?);
    }
    if let Some(loaders) = raw.get("loaders").and_then(Value::as_object) {
        let mut out: JsRecord<Loader> = JsRecord::new();
        for (code, spec) in loaders {
            let callback = h.callback(spec);
            out.insert(
                code.clone(),
                Rc::new(move || match callback.call(Vec::new()) {
                    Answer::Throws(error) => Err(error),
                    Answer::Promise(promise) => Ok(locale_promise(promise)),
                    Answer::Returns(value) => Ok(locale_promise(Promise::resolved(value))),
                }),
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
        Ok(i18n_plugin(config(raw, h)?))
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![I18N.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        _h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<I18nCapability>()?;
        let text = |i: usize| arg(&args, i).and_then(Value::as_str).unwrap_or_default().to_string();
        Some(match method {
            "t" => match options(arg(&args, 1)) {
                Ok(options) => Outcome::Returned(json!(cap.t(&text(0), options.as_ref()))),
                Err(error) => Outcome::Threw(error),
            },
            "locale" => Outcome::Returned(json!(cap.locale())),
            "dir" => Outcome::Returned(json!(cap.dir().as_str())),
            "locales" => {
                Outcome::Returned(serde_json::to_value(cap.locales()).unwrap_or(Value::Null))
            }
            "loading" => Outcome::Returned(json!(cap.loading())),
            "setLocale" => {
                cap.set_locale(&text(0));
                Outcome::undefined()
            }
            "registerLocale" => {
                match decode::<Locale>("locale", arg(&args, 0).unwrap_or(&Value::Null)) {
                    Ok(locale) => {
                        cap.register_locale(locale);
                        Outcome::undefined()
                    }
                    Err(error) => Outcome::Threw(error),
                }
            }
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

fn f_translate(args: &[Value]) -> Result<Value, Error> {
    let state: I18nState = decode("state", arg(args, 0).unwrap_or(&Value::Null))?;
    let key = arg(args, 1).and_then(Value::as_str).unwrap_or_default();
    let options = options(arg(args, 2))?;
    to_json(&translate(&state, key, options.as_ref()))
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

/// `initialI18nState` reads only whether a loader exists, so the vector's
/// loaders are placeholders: any member that is not `undefined`.
fn f_initial(args: &[Value]) -> Result<Value, Error> {
    let raw = arg(args, 0).cloned().unwrap_or_else(|| json!({}));
    let mut config = I18nConfig {
        locale: raw.get("locale").and_then(Value::as_str).map(str::to_string),
        fallback_locale: raw.get("fallbackLocale").and_then(Value::as_str).map(str::to_string),
        ..Default::default()
    };
    if let Some(locales) = raw.get("locales") {
        config.locales = Some(decode("locales", locales)?);
    }
    if let Some(loaders) = raw.get("loaders").and_then(Value::as_object) {
        let placeholder: Loader = Rc::new(|| Err(Error::new("placeholder")));
        config.loaders = Some(
            loaders
                .iter()
                .filter(|(_, v)| !is_undefined(v))
                .map(|(k, _)| (k.clone(), placeholder.clone()))
                .collect(),
        );
    }
    to_json(&initial_i18n_state(&config))
}

/// The reducer with its reference identity made visible. An action no
/// variant decodes is the TypeScript's `default: return state`.
fn f_reducer(args: &[Value]) -> Result<Value, Error> {
    let state: I18nState = decode("state", arg(args, 0).unwrap_or(&Value::Null))?;
    let action: Option<I18nAction> =
        arg(args, 1).and_then(|a| serde_json::from_value(scrub(a)).ok());
    let next = action.and_then(|action| i18n_reducer(&state, &action));
    Ok(json!({ "same": next.is_none(), "next": to_json(next.as_ref().unwrap_or(&state))? }))
}

/// `translate` over a pack whose branch names every CLDR category.
fn f_plurals(args: &[Value]) -> Result<Value, Error> {
    let locale = arg(args, 0).and_then(Value::as_str).unwrap_or_default();
    let dictionary = json!({ "k": { "zero": "zero", "one": "one", "two": "two", "few": "few", "many": "many", "other": "other" } });
    let pack = Locale {
        code: locale.into(),
        name: locale.into(),
        dir: None,
        translations: decode("pack", &dictionary)?,
    };
    let state = I18nState {
        locale: locale.into(),
        fallback_locale: locale.into(),
        locales: [(locale.to_string(), pack)].into_iter().collect(),
        loading: None,
    };
    let counts = arg(args, 1).and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for count in &counts {
        let n = number(count).ok_or_else(|| bad("count", count))?;
        let options = TranslateOptions {
            params: Some([("count".to_string(), Param::Number(n))].into_iter().collect()),
            fallback: None,
        };
        out.push(json!(translate(&state, "k", Some(&options)).text));
    }
    Ok(Value::Array(out))
}

pub fn functions() -> FunctionTable {
    vec![
        ("translate", f_translate),
        ("interpolate", f_interpolate),
        ("negotiateLocale", f_negotiate),
        ("initialI18nState", f_initial),
        ("i18nReducer", f_reducer),
        ("plurals", f_plurals),
    ]
}
