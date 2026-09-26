//! The conformance adapter: how `crates/plugin-conformance` builds this
//! plugin from a vector and calls its capability by name, plus the function
//! table. It mirrors the shims in
//! `tooling/plugin-conformance/src/suites/stamp-fixtures.ts`:
//!
//! - every case starts the id counter at zero, as the generator's fresh
//!   import of `capability.ts` per case does;
//! - a `console.error` from this plugin logs as the callback `console.error`
//!   (`StampHost::console_error`);
//! - `config.assetEngine` names an engine built over the kernel's:
//!   `'instance'`, `'factory'`, `'async-factory'`, `{throws}` or `{rejects}`,
//!   each factory call logging `assetEngine`;
//! - `config.scripting` is `StampScriptingOptions` with the scripted sandbox
//!   and scripted observers; `sandbox: null` is a host with no sandbox, which
//!   has no scripting options.

use std::any::Any;
use std::rc::Rc;

use epdf_kernel::engine::Engine;
use epdf_kernel::js::{from_value, to_value};
use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, AnyToken, Ctx, Error, define, is_undefined};
use epdf_plugin_form::harness::standalone_options;
use plugin_conformance::{Answer, Callback, FunctionTable, Harness, HarnessPlugin, Outcome};
use serde::Serialize;
use serde_json::{Value, json};

use crate::capability::{StampCapability, create_stamp_capability, reset_id_sequence};
use crate::types::{
    AddAssetInput, ArmAssetOptions, AssetEngine, AssetEngineRef, ImportLibraryOptions, Observer,
    STAMP, StampAction, StampAssetPreview, StampConfig, StampHost, StampScriptingOptions,
    StampState,
};
use crate::{initial_stamp_state, stamp_plugin, stamp_reducer};

/// The adapter.
pub struct Stamp;

/// The TypeScript object literal's member order.
pub const MEMBERS: &[&str] = &[
    "libraries",
    "library",
    "assets",
    "asset",
    "assetPreview",
    "assetBytes",
    "libraryBytes",
    "importLibraryPdf",
    "addAsset",
    "removeAsset",
    "removeLibrary",
    "armAsset",
    "disarm",
];

fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null() && !is_undefined(v))
}

/// An id argument as the property key JavaScript would look up.
fn key(args: &[Value], index: usize) -> String {
    match args.get(index) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => epdf_geometry::js::number_to_string(n.as_f64().unwrap_or(0.0)),
        _ => "undefined".to_string(),
    }
}

fn string(value: Option<&Value>) -> Option<String> {
    present(value).and_then(Value::as_str).map(str::to_string)
}

fn decoded<T: serde::de::DeserializeOwned>(value: Option<&Value>) -> Result<Option<T>, Error> {
    present(value).map(from_value).transpose()
}

fn out<T: Serialize>(value: Option<T>) -> Outcome {
    Outcome::Returned(value.map_or(Value::Null, |v| to_value(&v)))
}

fn bytes(value: Option<Vec<u8>>) -> Value {
    value.map_or(Value::Null, |b| epdf_kernel::bytes::Bytes(b).to_json())
}

fn preview(value: Option<StampAssetPreview>) -> Value {
    let Some(preview) = value else { return Value::Null };
    let mut out = serde_json::Map::new();
    if let Some(bytes) = preview.bytes {
        out.insert("bytes".into(), epdf_kernel::bytes::Bytes(bytes).to_json());
    }
    if let Some(mime_type) = preview.mime_type {
        out.insert("mimeType".into(), json!(mime_type));
    }
    Value::Object(out)
}

fn import_options(value: Option<&Value>, h: &Harness) -> Result<ImportLibraryOptions, Error> {
    let Some(opts) = present(value) else { return Ok(ImportLibraryOptions::default()) };
    let asset_name = present(opts.get("assetName")).map(|spec| {
        let callback = h.callback(spec);
        Rc::new(move |index: f64| match callback.call(vec![json!(index)]) {
            Answer::Returns(value) => value.as_str().map(str::to_string),
            _ => None,
        }) as Rc<dyn Fn(f64) -> Option<String>>
    });
    Ok(ImportLibraryOptions {
        name: string(opts.get("name")),
        categories: decoded(opts.get("categories"))?,
        kind: decoded(opts.get("kind"))?,
        asset_name,
    })
}

fn add_asset_input(value: Option<&Value>) -> Result<AddAssetInput, Error> {
    let input = value.cloned().unwrap_or(Value::Null);
    let member = |k: &str| input.get(k);
    Ok(AddAssetInput {
        library_id: string(member("libraryId")),
        name: string(member("name")).unwrap_or_default(),
        kind: decoded(member("kind"))?,
        subject: string(member("subject")),
        categories: decoded(member("categories"))?,
        source: member("source").cloned().unwrap_or_else(epdf_kernel::undefined),
        preview: member("preview").filter(|v| !is_undefined(v)).cloned(),
        size: decoded(member("size"))?,
    })
}

fn arm_options(value: Option<&Value>) -> ArmAssetOptions {
    ArmAssetOptions {
        target_width: present(value).and_then(|o| o.get("targetWidth")).and_then(Value::as_f64),
    }
}

/// `config.assetEngine`, over the kernel's engine.
fn asset_engine(spec: &Value, engine: Rc<dyn Engine>, h: &Harness) -> AssetEngine {
    if spec.as_str() == Some("instance") {
        return AssetEngine::Instance(engine);
    }
    let (spec, h) = (spec.clone(), h.clone());
    AssetEngine::Factory(Rc::new(move || {
        h.log("assetEngine", Vec::new());
        match spec.as_str() {
            Some("factory") => Ok(AssetEngineRef::Ready(engine.clone())),
            Some("async-factory") => Ok(AssetEngineRef::Pending(Promise::resolved(engine.clone()))),
            _ => match (spec.get("throws"), spec.get("rejects")) {
                (Some(error), _) => Err(Error::from_spec(error)),
                (None, Some(error)) => {
                    Ok(AssetEngineRef::Pending(Promise::rejected(Error::from_spec(error))))
                }
                _ => Err(Error::new("[stamp harness] unknown assetEngine spec")),
            },
        }
    }))
}

fn observer<T: Serialize + 'static>(spec: Option<&Value>, h: &Harness) -> Option<Observer<T>> {
    let callback: Callback = h.callback(present(spec)?);
    Some(Rc::new(move |value: &T| match callback.call(vec![to_value(value)]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    }))
}

/// `config.scripting`, or `None` for a host with no sandbox or no config.
fn scripting(spec: Option<&Value>, h: &Harness) -> Result<Option<StampScriptingOptions>, Error> {
    let Some(spec) = present(spec) else { return Ok(None) };
    let enabled = spec.get("enabled").and_then(Value::as_bool).unwrap_or(false);
    match spec.get("sandbox") {
        Some(Value::Null) => return Ok(None),
        None if enabled => {
            return Err(Error::new(
                "[stamp suite] scripting.enabled needs a scripted sandbox: the QuickJS default does not port",
            ));
        }
        None => return Ok(None),
        Some(_) => {}
    }
    Ok(Some(StampScriptingOptions {
        enabled,
        form: standalone_options(spec, h)?,
        on_ui_effect: observer(spec.get("onUiEffect"), h),
        on_diagnostic: observer(spec.get("onDiagnostic"), h),
        on_error: observer(spec.get("onError"), h),
    }))
}

fn host(h: &Harness) -> StampHost {
    let log = h.clone();
    StampHost {
        console_error: Some(Rc::new(move |message: &str, error: &Error| {
            log.log("console.error", vec![json!(message), error.to_canonical()]);
        })),
    }
}

fn config_of(spec: &Value, engine: Rc<dyn Engine>, h: &Harness) -> Result<StampConfig, Error> {
    Ok(StampConfig {
        asset_engine: present(spec.get("assetEngine")).map(|s| asset_engine(s, engine, h)),
        preview_width: spec.get("previewWidth").and_then(Value::as_f64),
        scripting: scripting(spec.get("scripting"), h)?,
        host: host(h),
    })
}

fn unit(result: Result<(), Error>) -> Outcome {
    match result {
        Ok(()) => Outcome::undefined(),
        Err(error) => Outcome::Threw(error),
    }
}

impl HarnessPlugin for Stamp {
    fn factories(&self) -> Vec<&'static str> {
        vec!["stamp", "stamp-plugin"]
    }

    fn build(&self, factory: &str, config: &Value, h: &Harness) -> Result<AnyPlugin, Error> {
        reset_id_sequence();
        if factory == "stamp-plugin" {
            return Ok(stamp_plugin(StampConfig { host: host(h), ..StampConfig::default() }));
        }
        let (spec, h) = (config.clone(), h.clone());
        // `stamp.plugin.ts`, with the config built over the kernel's engine.
        Ok(define::<StampState, StampAction>("stamp")
            .initial_state(initial_stamp_state)
            .reduce(stamp_reducer)
            .capability(&STAMP, move |ctx: Ctx<StampState, StampAction>| {
                let engine = ctx.engine().ok_or_else(|| Error::new("[stamp harness] no engine"))?;
                let config = config_of(&spec, engine, &h)?;
                Ok(create_stamp_capability(ctx, config))
            })
            .build())
    }

    fn tokens(&self) -> Vec<AnyToken> {
        vec![STAMP.any().clone()]
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let cap = capability.downcast_ref::<StampCapability>()?;
        Some(match method {
            "libraries" => Outcome::Returned(to_value(&cap.libraries())),
            "library" => out(cap.library(&key(&args, 0))),
            "assets" => {
                let library_id = present(args.first()).map(|_| key(&args, 0));
                Outcome::Returned(to_value(&cap.assets(library_id.as_deref())))
            }
            "asset" => out(cap.asset(&key(&args, 0))),
            "assetPreview" => Outcome::Returned(preview(cap.asset_preview(&key(&args, 0)))),
            "assetBytes" => Outcome::Returned(bytes(cap.asset_bytes(&key(&args, 0)))),
            "libraryBytes" => Outcome::Returned(bytes(cap.library_bytes(&key(&args, 0)))),
            "importLibraryPdf" => match import_options(args.get(1), h) {
                Ok(opts) => {
                    let source = args.first().cloned().unwrap_or_else(epdf_kernel::undefined);
                    Outcome::promise(cap.import_library_pdf(source, opts))
                }
                Err(error) => Outcome::Threw(error),
            },
            "addAsset" => match add_asset_input(args.first()) {
                Ok(input) => Outcome::promise(cap.add_asset(input)),
                Err(error) => Outcome::Threw(error),
            },
            "removeAsset" => Outcome::promise_unit(cap.remove_asset(&key(&args, 0))),
            "removeLibrary" => Outcome::promise_unit(cap.remove_library(&key(&args, 0))),
            "armAsset" => Outcome::promise_unit(cap.arm_asset(
                &key(&args, 0),
                &key(&args, 1),
                arm_options(args.get(2)),
            )),
            "disarm" => unit(cap.disarm(&key(&args, 0))),
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<StampCapability>()?;
        Some(MEMBERS.to_vec())
    }
}

// ── functions ─────────────────────────────────────────────────────────────────

fn initial_stamp_state_fn(_: &[Value]) -> Result<Value, Error> {
    Ok(to_value(&initial_stamp_state()))
}

/// `{changed: next !== state, state: next}`. An action outside the union is
/// the reducer's `return state`.
fn stamp_reducer_fn(a: &[Value]) -> Result<Value, Error> {
    let state: StampState = from_value(&a[0])?;
    let next = match from_value::<StampAction>(&a[1]) {
        Ok(action) => stamp_reducer(&state, &action),
        Err(_) => None,
    };
    Ok(json!({ "changed": next.is_some(), "state": to_value(next.as_ref().unwrap_or(&state)) }))
}

/// The pure functions, by the names the vectors use.
pub fn functions() -> FunctionTable {
    vec![("initialStampState", initial_stamp_state_fn), ("stampReducer", stamp_reducer_fn)]
}
