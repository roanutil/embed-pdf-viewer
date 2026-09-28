//! The scenario runner: `tooling/plugin-conformance/src/scenario.ts` in Rust.

use std::any::Any;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::kernel::{InitialDocument, OpenSource};
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::*;
use epdf_kernel::{
    AnyPlugin, AnyToken, DOCUMENTS, Documents, Error, Kernel, KernelOptions, OpenOptions, define,
    is_undefined, undefined,
};
use serde_json::{Value, json};

use crate::fake_engine::FakeEngine;
use crate::harness::{Answer, Harness, HarnessPlugin, Outcome};
use crate::scrub;

const CORE_ACTIONS: [&str; 8] = [
    CORE_DOCUMENT_ADDED,
    CORE_DOCUMENT_REMOVED,
    CORE_ACTIVE_CHANGED,
    CORE_ORDER_CHANGED,
    CORE_DOCUMENT_PAGES_UPDATED,
    CORE_DOCUMENT_OPENING,
    CORE_DOCUMENT_LOCKED,
    CORE_DOCUMENT_OPEN_FAILED,
];

/// The adapters a run may use. Plugin crates add theirs.
#[derive(Clone, Default)]
pub struct Registry {
    pub adapters: Vec<Rc<dyn HarnessPlugin>>,
}

impl Registry {
    pub fn new() -> Self {
        let mut registry = Registry::default();
        registry.add(Rc::new(DocumentsAdapter));
        registry.add(Rc::new(crate::probe::Probe));
        registry
    }

    pub fn add(&mut self, adapter: Rc<dyn HarnessPlugin>) -> &mut Self {
        self.adapters.push(adapter);
        self
    }

    pub fn with(mut self, adapter: impl HarnessPlugin + 'static) -> Self {
        self.add(Rc::new(adapter));
        self
    }
}

fn arg(args: &[Value], i: usize) -> Option<&Value> {
    args.get(i).filter(|v| !is_undefined(v))
}

fn str_arg(args: &[Value], i: usize) -> Option<&str> {
    arg(args, i).and_then(Value::as_str)
}

/// `documents` as the harness drives it.
struct DocumentsAdapter;

/// `documents`, in the order the TypeScript object literal defines it.
const DOCUMENTS_MEMBERS: &[&str] = &[
    "open",
    "retry",
    "rename",
    "openAll",
    "unlock",
    "close",
    "closeAll",
    "setActive",
    "getActiveId",
    "getActive",
    "list",
    "get",
    "has",
    "getCount",
    "getOrder",
    "setOrder",
    "move",
    "swap",
    "save",
    "saveLayer",
    "listPages",
    "getPage",
    "getPageAt",
    "getPageIndex",
    "getRevision",
    "allows",
    "onOpened",
    "onOpenFailed",
    "onLocked",
    "onClosed",
    "onActiveChanged",
    "onPagesChanged",
];

/// Subscribe a scripted `$fn` listener to a lifecycle event; the outcome is
/// the unsubscribe, as a ref.
fn subscribe_listener(
    hook: &epdf_kernel::event_hook::EventHookSource<Value>,
    spec: &Value,
    h: &Harness,
) -> Outcome {
    let callback = h.callback(spec);
    let off = hook.on(move |event| match callback.call(vec![event.clone()]) {
        Answer::Throws(error) => Err(error),
        _ => Ok(()),
    });
    Outcome::Returned(h.register_fn(Rc::new(move |_| {
        off.call();
        Outcome::undefined()
    })))
}

fn page_ref(value: Option<&Value>) -> Option<epdf_kernel::geometry::PageRef> {
    value.and_then(epdf_kernel::geometry::PageRef::from_json)
}

fn open_source(input: &Value, h: &Harness) -> OpenSource {
    if input.get("$fn").is_some() {
        let callback = h.callback(input);
        OpenSource::Thunk(Rc::new(move |signal: AbortSignal| {
            match callback.call(vec![json!({ "$signal": { "aborted": signal.aborted() } })]) {
                Answer::Returns(value) => Ok(Promise::resolved(value)),
                Answer::Throws(error) => Err(error),
                Answer::Promise(promise) => Ok(promise),
            }
        }))
    } else {
        OpenSource::Input(input.clone())
    }
}

impl HarnessPlugin for DocumentsAdapter {
    fn factories(&self) -> Vec<&'static str> {
        Vec::new()
    }

    fn build(&self, _: &str, _: &Value, _: &Harness) -> Result<AnyPlugin, Error> {
        Err(Error::new("documents is built in"))
    }

    fn call(
        &self,
        capability: &Rc<dyn Any>,
        method: &str,
        args: Vec<Value>,
        h: &Harness,
    ) -> Option<Outcome> {
        let docs = capability.downcast_ref::<Documents>()?;
        let json_of = |v: &dyn erased::Ser| v.json();
        let _ = json_of;
        Some(match method {
            "open" => {
                let source = open_source(arg(&args, 0).unwrap_or(&Value::Null), h);
                let options = arg(&args, 1).map(OpenOptions::from_json).unwrap_or_default();
                Outcome::Promise(docs.open(source, options))
            }
            "openAll" => {
                let list = arg(&args, 0).and_then(Value::as_array).cloned().unwrap_or_default();
                let initial = list
                    .iter()
                    .map(|entry| {
                        let mut options = entry.clone();
                        let object = options.as_object_mut();
                        let (source, active) = match object {
                            Some(o) => {
                                (o.remove("source").unwrap_or(Value::Null), o.remove("active"))
                            }
                            None => (Value::Null, None),
                        };
                        InitialDocument {
                            source: open_source(&source, h),
                            active: active.and_then(|a| a.as_bool()).unwrap_or(false),
                            options: OpenOptions::from_json(&options),
                        }
                    })
                    .collect();
                match docs.open_all(initial) {
                    Ok(ids) => Outcome::Returned(json!(ids)),
                    Err(error) => Outcome::Threw(error),
                }
            }
            "retry" => Outcome::Promise(docs.retry(str_arg(&args, 0).unwrap_or_default())),
            "rename" => {
                docs.rename(
                    str_arg(&args, 0).unwrap_or_default(),
                    str_arg(&args, 1).unwrap_or_default(),
                );
                Outcome::undefined()
            }
            "unlock" => {
                let password = arg(&args, 1)
                    .and_then(|o| o.get("password"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                Outcome::promise_unit(docs.unlock(str_arg(&args, 0).unwrap_or_default(), password))
            }
            "close" => Outcome::promise_unit(docs.close(str_arg(&args, 0).unwrap_or_default())),
            "closeAll" => Outcome::promise_unit(docs.close_all()),
            "setActive" => {
                docs.set_active(str_arg(&args, 0).unwrap_or_default());
                Outcome::undefined()
            }
            "getActiveId" => {
                Outcome::Returned(docs.get_active_id().map_or(Value::Null, Value::String))
            }
            "getActive" => Outcome::Returned(
                docs.get_active().map_or(Value::Null, |d| serde_json::to_value(d).unwrap()),
            ),
            "list" => Outcome::Returned(serde_json::to_value(docs.list()).unwrap()),
            "get" => Outcome::Returned(
                docs.get(str_arg(&args, 0).unwrap_or_default())
                    .map_or(Value::Null, |d| serde_json::to_value(d).unwrap()),
            ),
            "has" => Outcome::Returned(json!(docs.has(str_arg(&args, 0).unwrap_or_default()))),
            "getCount" => Outcome::Returned(json!(docs.get_count())),
            "getOrder" => Outcome::Returned(json!(docs.get_order())),
            "setOrder" => {
                let ids: Vec<String> = arg(&args, 0)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                    .unwrap_or_default();
                match docs.set_order(&ids) {
                    Ok(()) => Outcome::undefined(),
                    Err(error) => Outcome::Threw(error),
                }
            }
            "move" => {
                docs.move_to(
                    str_arg(&args, 0).unwrap_or_default(),
                    arg(&args, 1).and_then(Value::as_i64).unwrap_or(0),
                );
                Outcome::undefined()
            }
            "swap" => {
                docs.swap(
                    str_arg(&args, 0).unwrap_or_default(),
                    str_arg(&args, 1).unwrap_or_default(),
                );
                Outcome::undefined()
            }
            "save" => {
                let mode = arg(&args, 1).and_then(|o| o.get("mode")).filter(|m| !is_undefined(m));
                Outcome::Promise(docs.save(str_arg(&args, 0), mode.cloned()))
            }
            "saveLayer" => Outcome::Promise(docs.save_layer(str_arg(&args, 0))),
            "listPages" => Outcome::Returned(json!(docs.list_pages(str_arg(&args, 0)))),
            "getPage" => Outcome::Returned(
                page_ref(arg(&args, 0))
                    .and_then(|r| docs.get_page(r, str_arg(&args, 1)))
                    .unwrap_or(Value::Null),
            ),
            "getPageAt" => Outcome::Returned(
                docs.get_page_at(
                    arg(&args, 0).and_then(Value::as_i64).unwrap_or(-1),
                    str_arg(&args, 1),
                )
                .unwrap_or(Value::Null),
            ),
            "getPageIndex" => Outcome::Returned(json!(match page_ref(arg(&args, 0)) {
                Some(r) => docs.get_page_index(r, str_arg(&args, 1)),
                None => -1,
            })),
            "getRevision" => Outcome::Returned(json!(docs.get_revision(str_arg(&args, 0)))),
            "allows" => Outcome::Returned(json!(
                docs.allows(str_arg(&args, 0).unwrap_or_default(), str_arg(&args, 1))
            )),
            "onOpened" | "onOpenFailed" | "onLocked" | "onClosed" | "onActiveChanged"
            | "onPagesChanged" => {
                let Some(events) = docs.events() else { return Some(Outcome::undefined()) };
                let hook = match method {
                    "onOpened" => events.opened,
                    "onOpenFailed" => events.open_failed,
                    "onLocked" => events.locked,
                    "onClosed" => events.closed,
                    "onActiveChanged" => events.active_changed,
                    _ => events.pages_changed,
                };
                subscribe_listener(&hook, arg(&args, 0).unwrap_or(&Value::Null), h)
            }
            other => {
                Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function")))
            }
        })
    }

    fn members(&self, capability: &Rc<dyn Any>) -> Option<Vec<&'static str>> {
        capability.downcast_ref::<Documents>()?;
        Some(DOCUMENTS_MEMBERS.to_vec())
    }
}

mod erased {
    pub trait Ser {
        fn json(&self) -> serde_json::Value;
    }
}

/// One replayed case: the trace, and any engine call nothing answered.
pub struct Run {
    pub trace: Vec<Value>,
    pub unmatched: Vec<String>,
}

fn token_arg(value: Option<&Value>, h: &Harness) -> AnyToken {
    let name = value.and_then(|v| v.get("$token")).and_then(Value::as_str).unwrap_or_default();
    h.token(name, None)
}

fn kernel_call(kernel: &Kernel, method: &str, args: &[Value], h: &Harness) -> Outcome {
    match method {
        "start" => Outcome::promise_unit(kernel.start()),
        "destroy" => Outcome::promise_unit(kernel.destroy()),
        "status" => Outcome::Returned(json!(kernel.status().as_str())),
        "getState" => Outcome::Returned(kernel.state_json()),
        "scopeOf" => Outcome::Returned(json!(match kernel.scope_of(&token_arg(args.first(), h)) {
            PluginScope::Document => "document",
            PluginScope::Workspace => "workspace",
        })),
        "capability" => {
            let token = token_arg(args.first(), h);
            match kernel.capability_any(&token, str_arg(args, 1)) {
                Ok(capability) => {
                    Outcome::Returned(h.describe(capability).unwrap_or_else(|| json!({})))
                }
                Err(error) => Outcome::Threw(error),
            }
        }
        "tryCapability" => {
            let token = token_arg(args.first(), h);
            match kernel.try_capability_any(&token, str_arg(args, 1)) {
                Ok(Some(capability)) => {
                    Outcome::Returned(h.describe(capability).unwrap_or_else(|| json!({})))
                }
                Ok(None) => Outcome::Returned(Value::Null),
                Err(error) => Outcome::Threw(error),
            }
        }
        other => Outcome::Threw(Error::new(format!("[scenario] \"{other}\" is not a function"))),
    }
}

fn spy(h: &Harness) -> AnyPlugin {
    let h = h.clone();
    define::<Value, Value>("@@conformance/spy")
        .effects(move |fx| {
            for kind in CORE_ACTIONS {
                let h = h.clone();
                fx.on_action(kind, move |action| {
                    h.0.entry
                        .borrow_mut()
                        .actions
                        .push(json!({ "slice": "@@core", "action": scrub(&action.to_json()) }));
                    Ok(())
                });
            }
            Ok(())
        })
        .build()
}

fn snapshot(kernel: &Kernel, keys: Option<&Vec<Value>>) -> Value {
    let state = kernel.state_json();
    let Some(keys) = keys else { return scrub(&state) };
    let mut plugins = serde_json::Map::new();
    let names: Vec<&str> = keys.iter().filter_map(Value::as_str).collect();
    for key in &names {
        if *key == "@@core" {
            continue;
        }
        if let Some(slice) = state["plugins"].get(*key) {
            plugins.insert(key.to_string(), slice.clone());
        }
    }
    let out = if names.contains(&"@@core") {
        json!({ "core": state["core"], "plugins": plugins })
    } else {
        json!({ "plugins": plugins })
    };
    scrub(&out)
}

fn finish_entry(h: &Harness, extra: serde_json::Map<String, Value>) -> Value {
    let entry = std::mem::take(&mut *h.0.entry.borrow_mut());
    let mut out = serde_json::Map::new();
    if let Some(outcome) = entry.outcome {
        out.insert("outcome".into(), outcome);
    }
    out.insert("settled".into(), Value::Array(entry.settled));
    out.insert("actions".into(), Value::Array(entry.actions));
    out.insert("callbacks".into(), Value::Array(entry.callbacks));
    out.insert("reports".into(), Value::Array(entry.reports));
    out.extend(extra);
    Value::Object(out)
}

/// Replay one case.
pub fn run_case(case: &Value, registry: &Registry) -> Run {
    rt::reset();
    let mut tokens: HashMap<String, AnyToken> = HashMap::new();
    tokens.insert(DOCUMENTS.name().to_string(), DOCUMENTS.any().clone());
    for adapter in &registry.adapters {
        for token in adapter.tokens() {
            tokens.insert(token.name().to_string(), token);
        }
    }
    let h = Harness::new(registry.adapters.clone(), tokens);
    let fake = FakeEngine::new(case.get("engine").cloned().unwrap_or_else(|| json!({})));
    *h.0.fake.borrow_mut() = Some(fake.clone());

    let mut plugins = vec![spy(&h)];
    let mut build_error = None;
    for spec in case.get("plugins").and_then(Value::as_array).into_iter().flatten() {
        let factory = spec["factory"].as_str().unwrap_or_default();
        let config = spec.get("config").cloned().unwrap_or_else(undefined);
        let adapter = registry.adapters.iter().find(|a| a.factories().contains(&factory));
        match adapter {
            Some(adapter) => match adapter.build(factory, &config, &h) {
                Ok(plugin) => plugins.push(plugin),
                Err(error) => {
                    build_error = Some(error);
                    break;
                }
            },
            None => panic!("no adapter provides factory \"{factory}\""),
        }
    }
    if let Some(error) = build_error {
        panic!("building plugins failed: {error}");
    }

    let report_h = h.clone();
    let dispatch_h = h.clone();
    let options = KernelOptions {
        report: Some(Rc::new(move |error: &Error| report_h.report(error))),
        on_dispatch: Some(Rc::new(move |slice: &str, action: &dyn epdf_kernel::Action| {
            dispatch_h
                .0
                .entry
                .borrow_mut()
                .actions
                .push(json!({ "slice": slice, "action": scrub(&action.to_json()) }));
        })),
        release_closed_sessions: false,
    };
    let kernel =
        match Kernel::new(Rc::new(crate::fake_engine::AsEngine(fake.clone())), plugins, options) {
            Ok(kernel) => kernel,
            Err(error) => {
                h.0.entry.borrow_mut().outcome = Some(json!({ "threw": error.to_canonical() }));
                let mut extra = serde_json::Map::new();
                extra.insert("engine".into(), json!([]));
                extra.insert("status".into(), json!("unconstructed"));
                extra.insert("engineState".into(), json!({}));
                return Run { trace: vec![finish_entry(&h, extra)], unmatched: fake.unmatched() };
            }
        };

    // The TypeScript starts every step with a blank entry, so whatever the
    // plugins logged while the kernel was being built is not in any step.
    *h.0.entry.borrow_mut() = Default::default();
    let snapshot_keys = case.get("snapshot");
    let mut trace = Vec::new();
    let mut logged = 0;
    for step in case.get("steps").and_then(Value::as_array).into_iter().flatten() {
        run_step(step, &kernel, &fake, &h);
        rt::run_until_stalled();
        let mut extra = serde_json::Map::new();
        extra.insert("engine".into(), Value::Array(fake.log_since(logged)));
        logged = fake.log_len();
        extra.insert("status".into(), json!(kernel.status().as_str()));
        extra.insert("engineState".into(), fake.snapshot());
        match snapshot_keys {
            Some(Value::String(s)) if s == "none" => {}
            Some(Value::Array(keys)) => {
                extra.insert("state".into(), snapshot(&kernel, Some(keys)));
            }
            _ => {
                extra.insert("state".into(), snapshot(&kernel, None));
            }
        }
        trace.push(finish_entry(&h, extra));
    }
    let run = Run { trace, unmatched: fake.unmatched() };
    drop(kernel);
    rt::reset();
    run
}

fn run_step(step: &Value, kernel: &Kernel, fake: &Rc<FakeEngine>, h: &Harness) {
    let op = step["op"].as_str().unwrap_or_default();
    match op {
        "call" => {
            let outcome = invoke(step, kernel, h);
            let json = h.outcome_json(outcome);
            h.0.entry.borrow_mut().outcome = Some(json);
        }
        "emit" => {
            if let Err(error) = fake.emit(step["doc"].as_str().unwrap_or_default(), &step["event"])
            {
                panic!("{error}");
            }
        }
        "settle" => {
            let label = step["label"].as_str().unwrap_or_default();
            let outcome = match step.get("error") {
                Some(error) => Err(error),
                None => Ok(step.get("value").cloned().unwrap_or_else(undefined)),
            };
            if !fake.settle(label, outcome) {
                panic!("[scenario] nothing pending as \"{label}\"");
            }
        }
        "advance" => rt::advance(step["ms"].as_f64().unwrap_or(0.0)),
        "abort" => {
            let id = step["promise"].as_u64().unwrap_or(0) as usize;
            let promise = h.promise(id).unwrap_or_else(|| panic!("[scenario] no promise {id}"));
            promise.abort(step.get("reason").cloned());
        }
        "drain" => {}
        other => panic!("[scenario] unknown op {other}"),
    }
}

fn invoke(step: &Value, kernel: &Kernel, h: &Harness) -> Outcome {
    let args = step.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
    let method = step.get("method").and_then(Value::as_str);
    let target = &step["target"];
    match (target.as_str(), method) {
        (Some("kernel"), Some(method)) => kernel_call(kernel, method, &args, h),
        (Some("documents"), Some(method)) => {
            let docs: Rc<dyn Any> = Rc::new(kernel.documents());
            h.call(&docs, method, args)
        }
        _ => {
            if let Some(id) = target.get("ref").and_then(Value::as_u64) {
                if method.is_some() {
                    return Outcome::Threw(Error::new(
                        "[scenario] a ref is a function; call it without a method",
                    ));
                }
                return match h.ref_fn(id as usize) {
                    Some(f) => f(args),
                    None => Outcome::Threw(Error::new(format!("[scenario] no ref {id}"))),
                };
            }
            let Some(method) = method else {
                return Outcome::Threw(Error::new("[scenario] a call on an object needs a method"));
            };
            let token = h.token(target["token"].as_str().unwrap_or_default(), None);
            match kernel.capability_any(&token, target.get("doc").and_then(Value::as_str)) {
                Ok(capability) => h.call(&capability, method, args),
                Err(error) => Outcome::Threw(error),
            }
        }
    }
}
