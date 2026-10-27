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
    "download",
    "downloadLayer",
    "preparePrint",
    "print",
    "getPrintQuality",
    "canDownload",
    "canPrint",
    "listPages",
    "getPage",
    "getPageIndex",
    "getRevision",
    "onOpened",
    "onOpenFailed",
    "onLocked",
    "onClosed",
    "onActiveChanged",
    "onPagesChanged",
    "onUnsavedChangesChanged",
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
                docs.get_document(str_arg(&args, 0))
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
            "download" => {
                let mode = arg(&args, 1).and_then(|o| o.get("mode")).filter(|m| !is_undefined(m));
                Outcome::Promise(docs.save(str_arg(&args, 0), mode.cloned()))
            }
            "downloadLayer" => Outcome::Promise(docs.save_layer(str_arg(&args, 0))),
            "listPages" => Outcome::Returned(json!(docs.list_pages(str_arg(&args, 0)))),
            "getPage" if arg(&args, 0).is_some_and(Value::is_number) => Outcome::Returned(
                docs.get_page_at(
                    arg(&args, 0).and_then(Value::as_i64).unwrap_or(-1),
                    str_arg(&args, 1),
                )
                .unwrap_or(Value::Null),
            ),
            // The replayed vectors pass the kernel no printer, so these
            // answer as TS does without one (kernel.ts:736-752, 887-897).
            "canPrint" => Outcome::Returned(json!(docs.can_print(str_arg(&args, 0)))),
            "getPrintQuality" => Outcome::Returned(
                docs.get_print_quality(str_arg(&args, 0))
                    .map_or(Value::Null, |quality| quality.to_json()),
            ),
            "preparePrint" | "print" => {
                let options = print_options(arg(&args, 1));
                if method == "print" {
                    Outcome::Promise(
                        docs.print(str_arg(&args, 0), options).convert(|_| Ok(undefined())),
                    )
                } else {
                    Outcome::Promise(docs.prepare_print(str_arg(&args, 0), options).convert(
                        |prepared| {
                            Ok(json!({
                                "quality": prepared.quality().to_json(),
                                "pageCount": prepared.page_count(),
                                "needsTap": prepared.needs_tap(),
                            }))
                        },
                    ))
                }
            }
            "canDownload" => Outcome::Returned(json!(docs.can_download(str_arg(&args, 0)))),
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
            "onOpened"
            | "onOpenFailed"
            | "onLocked"
            | "onClosed"
            | "onActiveChanged"
            | "onPagesChanged"
            | "onUnsavedChangesChanged" => {
                let Some(events) = docs.events() else { return Some(Outcome::undefined()) };
                let hook = match method {
                    "onOpened" => events.opened,
                    "onOpenFailed" => events.open_failed,
                    "onLocked" => events.locked,
                    "onClosed" => events.closed,
                    "onActiveChanged" => events.active_changed,
                    "onPagesChanged" => events.pages_changed,
                    _ => events.unsaved_changes_changed,
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

/// `PrintOptions` as a step spells them: `pages` and `includeAnnotations`.
fn print_options(value: Option<&Value>) -> epdf_kernel::print::PrintOptions {
    let pages = value
        .and_then(|o| o.get("pages"))
        .and_then(Value::as_array)
        .map(|refs| refs.iter().filter_map(epdf_kernel::geometry::PageRef::from_json).collect());
    let include_annotations =
        value.and_then(|o| o.get("includeAnnotations")).and_then(Value::as_bool);
    epdf_kernel::print::PrintOptions { pages, include_annotations, ..Default::default() }
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

/// A returned capability, printed; a throwing getter makes the call throw.
fn described(h: &Harness, capability: Rc<dyn Any>) -> Outcome {
    match h.try_describe(capability) {
        Some(Ok(value)) => Outcome::Returned(value),
        Some(Err(error)) => Outcome::Threw(error),
        None => Outcome::Returned(json!({})),
    }
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
                Ok(capability) => described(h, capability),
                Err(error) => Outcome::Threw(error),
            }
        }
        "tryCapability" => {
            let token = token_arg(args.first(), h);
            match kernel.try_capability_any(&token, str_arg(args, 1)) {
                Ok(Some(capability)) => described(h, capability),
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
        .initial_state(undefined)
        .effects(move |fx| {
            let docs = fx.get(&*DOCUMENTS)?;
            if let Some(events) = docs.events() {
                for (name, hook) in [
                    ("opened", events.opened),
                    ("openFailed", events.open_failed),
                    ("locked", events.locked),
                    ("closed", events.closed),
                    ("activeChanged", events.active_changed),
                    ("pagesChanged", events.pages_changed),
                    ("unsavedChangesChanged", events.unsaved_changes_changed),
                ] {
                    let h = h.clone();
                    let off = hook.on(move |value| {
                        h.0.entry
                            .borrow_mut()
                            .document_events
                            .push(json!({"event": name, "value": scrub(value)}));
                        Ok(())
                    });
                    fx.cleanup_fn(move || off.call());
                }
            }
            Ok(())
        })
        .build()
}

fn instrument(plugin: AnyPlugin, h: &Harness) -> AnyPlugin {
    let Some(create) = plugin.create.clone() else { return plugin };
    let mut wrapper = epdf_kernel::plugin::PluginDef {
        id: plugin.id.clone(),
        token: plugin.token.clone(),
        scope: plugin.scope,
        requires: plugin.requires.clone(),
        optional: plugin.optional.clone(),
        initial_state: plugin.initial_state.clone(),
        reducer: plugin.reducer.clone(),
        to_json: plugin.to_json.clone(),
        to_text: plugin.to_text.clone(),
        settings: plugin.settings.clone(),
        capability: plugin.capability.clone(),
        init: plugin.init.clone(),
        effects: plugin.effects.clone(),
        create: None,
        resolves_any_capability: plugin.resolves_any_capability,
        in_scope: plugin.in_scope.clone(),
    };
    let h = h.clone();
    // Printed through the slice's own JSON, as the TypeScript records the
    // state object: a typed slice is not a `Value`.
    let to_json = plugin.to_json.clone();
    wrapper.create = Some(Rc::new(move |cx| {
        let slice = cx.slice_key().to_string();
        let h = h.clone();
        let to_json = to_json.clone();
        let off = cx.lease().on_change(move |change| {
            let show = |state: &epdf_kernel::store::SliceState| (to_json)(&**state);
            h.0.entry.borrow_mut().state_changes.push(json!({"slice": slice, "previous": show(&change.previous), "next": show(&change.next)}));
            Ok(())
        });
        cx.listen(off);
        create(cx)
    }));
    Rc::new(wrapper)
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
    out.insert("stateChanges".into(), Value::Array(entry.state_changes));
    out.insert("documentEvents".into(), Value::Array(entry.document_events));
    out.insert("callbacks".into(), Value::Array(entry.callbacks));
    out.insert("reports".into(), Value::Array(entry.reports));
    out.extend(extra);
    Value::Object(out)
}

/// The scenario host schedules real runtime timers, including 16 ms frames.
pub struct ScenarioClock {
    pub frames: bool,
}
impl epdf_kernel::clock::HostClock for ScenarioClock {
    fn now(&self) -> f64 {
        rt::now()
    }
    fn after(&self, ms: f64, run: Box<dyn FnOnce()>) -> epdf_kernel::Unsubscribe {
        let timer = rt::set_timeout(ms, run);
        epdf_kernel::Unsubscribe::new(move || rt::clear_timer(timer))
    }
    fn has_frames(&self) -> bool {
        self.frames
    }
    fn next_frame(&self, run: Box<dyn FnOnce(f64)>) -> epdf_kernel::Unsubscribe {
        self.after(16.0, Box::new(move || run(rt::now())))
    }
}

thread_local! {
    static CASE_ENTROPY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Restores the UUID source on drop; a nested install holds none, so the
/// outer case's sequence continues, as `installCaseEntropy` returns early
/// when one is installed (case-entropy.ts:11).
pub struct CaseEntropy(Option<epdf_kernel::identity::UuidSourceGuard>);

impl Drop for CaseEntropy {
    fn drop(&mut self) {
        if let Some(guard) = self.0.take() {
            drop(guard);
            CASE_ENTROPY.with(|installed| installed.set(false));
        }
    }
}

/// One sequence shared by factories and kernel changes, restored on every exit.
pub fn install_case_entropy() -> CaseEntropy {
    if CASE_ENTROPY.with(|installed| installed.replace(true)) {
        return CaseEntropy(None);
    }
    let sequence = std::cell::Cell::new(0_u32);
    CaseEntropy(Some(epdf_kernel::identity::with_uuid_source(
        Rc::new(move |bytes| {
            let next = sequence.get().wrapping_add(1);
            sequence.set(next);
            bytes.fill(0);
            bytes[12..].copy_from_slice(&next.to_be_bytes());
        }),
        Rc::new(rt::now),
    )))
}

/// Replay one case.
pub fn run_case(case: &Value, registry: &Registry) -> Run {
    rt::reset();
    let _entropy = install_case_entropy();
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
    let mut wrapped: Vec<(AnyPlugin, AnyPlugin)> = Vec::new();
    for spec in case.get("plugins").and_then(Value::as_array).into_iter().flatten() {
        let factory = spec["factory"].as_str().unwrap_or_default();
        let config = spec.get("config").cloned().unwrap_or_else(undefined);
        let adapter = registry.adapters.iter().find(|a| a.factories().contains(&factory));
        match adapter {
            Some(adapter) => match adapter.build(factory, &config, &h) {
                Ok(plugin) => {
                    let wrapper = wrapped
                        .iter()
                        .find(|(original, _)| Rc::ptr_eq(original, &plugin))
                        .map(|(_, wrapper)| wrapper.clone())
                        .unwrap_or_else(|| {
                            let wrapper = instrument(plugin.clone(), &h);
                            wrapped.push((plugin, wrapper.clone()));
                            wrapper
                        });
                    plugins.push(wrapper);
                }
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
        printer: None,
    };
    let kernel = match Kernel::new_with_clock(
        Rc::new(crate::fake_engine::AsEngine(fake.clone())),
        plugins,
        options,
        Rc::new(ScenarioClock { frames: case["clock"]["frames"].as_bool() == Some(true) }),
    ) {
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

    // Whatever the plugins logged while the kernel was being built stays in
    // the first step's entry, as the TypeScript's first entry holds it.
    let snapshot_keys = case.get("snapshot");
    let mut trace = Vec::new();
    let mut logged = 0;
    for step in case.get("steps").and_then(Value::as_array).into_iter().flatten() {
        run_step(step, &kernel, &fake, &h);
        if let Some(turns) = step.get("meter").and_then(Value::as_u64) {
            assert!(step["op"] != "advance", "[scenario] an advance step cannot be metered");
            start_meter(&h, turns);
        }
        rt::run_until_stalled();
        h.0.metering.set(false);
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

/// The turn meter: `@@turn [i]` for each of `turns` turns, starting now.
fn start_meter(h: &Harness, turns: u64) {
    h.0.metering.set(true);
    let h = h.clone();
    rt::spawn(async move {
        for i in 0..turns {
            h.log("@@turn", vec![json!(i)]);
            rt::tick().await;
        }
    });
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
