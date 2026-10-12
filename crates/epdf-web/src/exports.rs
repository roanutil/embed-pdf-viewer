//! What JavaScript sees: [`WasmKernel`], a kernel built from plugin specs
//! through the conformance adapters and driven by name.
//!
//! Values cross as canonical JSON text (`{"$undefined": true}`,
//! `{"$number": "NaN"}`, `{"$bytes": ...}` intact, never scrubbed). A
//! capability reads as the harness describes it: member name to
//! `{"$ref": n}` or a plain value. A ref is called with [`WasmKernel::invoke_ref`],
//! whose outcome is `{"returned": v}`, `{"threw": e}` or `{"promise": n}`,
//! and a promise id becomes a JavaScript promise with [`WasmKernel::promise`],
//! taken from the harness's table exactly once: a second call throws. A ref
//! lives until [`WasmKernel::release_ref`], which the shim calls once the
//! function it built from the ref is garbage. A capability's own refs are
//! pinned, never released while its document is open: its description
//! prints each as `{"$ref": n, "pinned": true}`.
//!
//! A JavaScript function in a plugin config or a call's arguments arrives as
//! `{"$fn": "js:<id>"}`. Calling it calls `call_js(id, argsJson)` with the
//! `<id>` text; its return value, throw or promise is the callback's answer.
//! Once no callback made from `js:<id>` is left (the same function can back
//! several at once), the kernel calls `release_js(id)`, at the end of the
//! outermost export or event it happened in: a callback dropped and made
//! again within one call keeps its function. JavaScript may then forget
//! the function; the same function passed again crosses under a new id.
//! On an idle kernel, a release queued outside any entry waits for the
//! next outermost entry: nothing else runs the sweep.
//!
//! A command function's context arrives with markers where the TypeScript
//! has functions and a live object: `ctx.core`, `ctx.get` and `ctx.tryGet`
//! are each `{"$kernel": "core" | "get" | "tryGet", "doc": <documentId or
//! null>}` (`epdf_plugin_commands::harness`, `ctx_json`). The host resolves
//! `get` to [`WasmKernel::capability`], `tryGet` to
//! [`WasmKernel::try_capability`] and `core` to `state_json().core`, each
//! with that `doc`.
//!
//! The same capability object always describes to the same text, with the
//! same refs: [`WasmKernel`] memoizes each description by the capability's
//! `Rc`, which the cache holds so its address is never reused. That is why
//! its refs are pinned: the memo hands the same ids out again. Once a
//! document closes, its capabilities leave the memo and their refs are
//! unpinned, so they release like any other; the texts they described to
//! wait in [`WasmKernel::take_evicted`] for the shim to drop its proxies.
//!
//! Every export's body runs as a [`runtime::entry`], so the kernel's queue
//! drains and its timers stay armed once the outermost one returns.
//!
//! `free()` (wasm-bindgen's, once [`destroy`](WasmKernel::destroy) has
//! settled) drops the memo and every ref and promise the harness still
//! holds: a plugin's store can hold callbacks that hold the harness, whose
//! refs hold the capabilities that hold the store, and only this breaks
//! that loop. Everything else the runtime keeps (its armed timeout, its
//! queue, its exit hooks) belongs to the wasm instance, not to one kernel,
//! and goes on serving any other kernel in it.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Once;

use epdf_kernel::types::PluginScope;
use epdf_kernel::{
    AnyToken, DOCUMENTS, Error, Kernel, KernelOptions, Promise, Unsubscribe, undefined,
};
use plugin_conformance::{Answer, Harness, LiveFn, Outcome, ReleaseFn};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::adapters::adapters;
use crate::engine::{EngineBridge, JsEngine};
use crate::runtime::{self, entry, js_to_error, js_to_value, value_to_js};

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(prefix: &str, error: &JsValue);
}

/// Hand every panic's message and location to `console.error`, once per
/// instance. Without it a panic reaches the page only as the bare
/// `RuntimeError: unreachable` its abort throws.
fn report_panics() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            console_error("[epdf-web] panic:", &js_sys::Error::new(&info.to_string()).into());
            previous(info);
        }));
    });
}

/// An error as JavaScript sees it thrown: the canonical `{"$error": ...}`.
fn thrown(error: &Error) -> JsValue {
    value_to_js(&error.to_canonical())
}

/// The wasm module's own linear memory, in bytes: `wasm_bindgen::memory()`'s
/// buffer's `byteLength`. Dev-only, for the eval build's memory soak test.
/// This is the number `process.memoryUsage().heapUsed` cannot see: the
/// harness's `refs` and `promises` tables (and everything they hold) live
/// here, not in V8's JS heap.
#[wasm_bindgen]
pub fn wasm_memory_bytes() -> f64 {
    let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
    let buffer: js_sys::ArrayBuffer = memory.buffer().unchecked_into();
    buffer.byte_length() as f64
}

/// Install `f` as the one resolver every `{"$hostBytes": n}` value goes
/// through (`epdf_kernel::bytes::Bytes::from_json`), for the life of this
/// wasm instance. Called once, by the shim right after the module loads
/// (`packages/core-wasm/src/browser.ts`'s `initWasm`), with the module-
/// level `hostBytes` from `engine-bridge.ts`: not a `WasmKernel` method,
/// because a `WasmKernel` is not who should own this. A resolver installed
/// per kernel let a second live kernel (two viewers on a page, or a
/// remount) silently overwrite the first kernel's resolver; `f` here
/// already knows how to find any bridge's ids, because every bridge
/// `engine-bridge.ts` builds shares one host table. A call after the first
/// is a no-op: nothing about which resolver is right changes from kernel
/// to kernel.
#[wasm_bindgen]
pub fn set_host_bytes_source(f: js_sys::Function) {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        epdf_kernel::bytes::set_host_bytes_resolver(Some(Rc::new(move |n: u32| {
            match f.call1(&JsValue::NULL, &JsValue::from_f64(f64::from(n))) {
                Ok(v) => v.dyn_into::<js_sys::Uint8Array>().ok().map(|array| array.to_vec()),
                Err(error) => {
                    console_error("[epdf-web] hostBytes threw:", &error);
                    None
                }
            }
        })));
    });
}

/// Install `f` as what `epdf_kernel::bytes::release_host` calls, for the
/// life of this wasm instance: `f(n)` with the value's `$host` id, and
/// nothing for a value without one. Installed next to
/// [`set_host_bytes_source`], and for the same reason once per instance
/// rather than per kernel: `f` is the module-level `releaseHost` from
/// `engine-bridge.ts`, which finds whichever bridge holds `n`. A throw (a
/// double release, which `f` has already reported with `console.error`)
/// cannot cross back into the plugin that released, so it is logged here
/// too. A call after the first is a no-op.
#[wasm_bindgen]
pub fn set_host_release(f: js_sys::Function) {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        epdf_kernel::bytes::set_host_releaser(Some(Rc::new(move |value: &Value| {
            let Some(n) = value.get("$host").and_then(Value::as_u64) else { return };
            if let Err(error) = f.call1(&JsValue::NULL, &JsValue::from_f64(n as f64)) {
                console_error("[epdf-web] releaseHost threw:", &error);
            }
        })));
    });
}

fn parse_args(args_json: &str) -> Result<Vec<Value>, Error> {
    match serde_json::from_str(args_json) {
        Ok(Value::Array(args)) => Ok(args),
        Ok(other) => Err(Error::named("TypeError", format!("arguments are not an array: {other}"))),
        Err(e) => Err(Error::named("TypeError", format!("malformed arguments: {e}"))),
    }
}

/// A `$fn` callback's call as a call to `call_js`.
fn live(call_js: js_sys::Function) -> LiveFn {
    Rc::new(move |spec: &Value, args: Vec<Value>| {
        let id = spec.get("$fn").and_then(Value::as_str).and_then(|s| s.strip_prefix("js:"));
        let Some(id) = id else {
            return Answer::Throws(Error::new(format!(
                "[epdf-web] not a JavaScript function: {spec}"
            )));
        };
        let args = Value::Array(args).to_string();
        let value = match call_js.call2(
            &JsValue::NULL,
            &JsValue::from_str(id),
            &JsValue::from_str(&args),
        ) {
            Ok(value) => value,
            Err(error) => return Answer::Throws(js_to_error(&error)),
        };
        if let Some(promise) = thenable(&value) {
            return Answer::Promise(runtime::from_js(promise));
        }
        match js_to_value(&value) {
            Ok(value) => Answer::Returns(value),
            Err(error) => Answer::Throws(error),
        }
    })
}

/// Call `release_js(id)` for each `js:<id>` name in `released` that no
/// callback holds any more. Run at the end of an outermost entry, so a name
/// whose callbacks were dropped and remade within it is still held.
fn release_fns(harness: &Harness, released: &RefCell<Vec<String>>, release_js: &js_sys::Function) {
    let mut names = std::mem::take(&mut *released.borrow_mut());
    names.sort();
    names.dedup();
    for name in names {
        if harness.holds_fn(&name) {
            continue;
        }
        let Some(id) = name.strip_prefix("js:") else { continue };
        if let Err(error) = release_js.call1(&JsValue::NULL, &JsValue::from_str(id)) {
            console_error("[epdf-web] release_js threw:", &error);
        }
    }
}

/// A promise, or any object with a callable `then`, as a promise.
fn thenable(value: &JsValue) -> Option<js_sys::Promise> {
    if let Some(promise) = value.dyn_ref::<js_sys::Promise>() {
        return Some(promise.clone());
    }
    if !value.is_object() {
        return None;
    }
    let then = js_sys::Reflect::get(value, &JsValue::from_str("then")).ok()?;
    then.is_function().then(|| js_sys::Promise::resolve(value))
}

/// A capability's description, with the capability that owns the address
/// it is keyed by.
struct Described {
    /// Held, so the address is never reused while the entry lives.
    _capability: Rc<dyn Any>,
    /// The document it was resolved for; `None` for a workspace capability.
    doc: Option<String>,
    text: String,
    /// The refs the text pins.
    refs: Vec<u32>,
}

/// [`WasmKernel::describe`]'s memo, by capability address. The documents'
/// closed listener holds it weakly and evicts from it.
struct Memo {
    harness: Harness,
    described: RefCell<HashMap<*const (), Described>>,
    /// The texts of evicted descriptions, until
    /// [`take_evicted`](WasmKernel::take_evicted).
    evicted: RefCell<Vec<String>>,
}

impl Memo {
    /// Drop every description of a capability of document `doc`, and unpin
    /// its refs.
    fn evict(&self, doc: &str) {
        let gone: Vec<Described> = {
            let mut described = self.described.borrow_mut();
            let keys: Vec<*const ()> = described
                .iter()
                .filter(|(_, entry)| entry.doc.as_deref() == Some(doc))
                .map(|(key, _)| *key)
                .collect();
            keys.iter().filter_map(|key| described.remove(key)).collect()
        };
        for entry in &gone {
            entry.refs.iter().for_each(|id| self.harness.unpin_ref(*id));
        }
        self.evicted.borrow_mut().extend(gone.into_iter().map(|entry| entry.text));
    }
}

/// The kernel and every plugin, built from specs and driven by name.
#[wasm_bindgen]
pub struct WasmKernel {
    kernel: Kernel,
    harness: Harness,
    /// `subscribe` registrations by id. Dropping the last clone of an
    /// `Unsubscribe` unsubscribes, so each lives here until `unsubscribe`.
    subscriptions: RefCell<HashMap<u32, Unsubscribe>>,
    next_subscription: Cell<u32>,
    /// Descriptions by capability address.
    memo: Rc<Memo>,
    /// The documents' closed listener that evicts from `memo`.
    on_closed: Option<Unsubscribe>,
    /// `kernel.documents`: the documents capability, held so its
    /// description memoizes too.
    documents: Rc<dyn Any>,
    /// A taken promise's abort handle, by id, kept until it settles:
    /// [`promise`](Self::promise) takes the promise out of the harness's own
    /// table, so [`abort_promise`](Self::abort_promise) needs somewhere else
    /// to find it. `Rc`-wrapped so the `on_settle` closure that clears an
    /// entry can outlive the call that registered it. The entry of a
    /// promise that never settles stays, by design: it is still abortable,
    /// and nothing else can tell it is abandoned.
    aborts: Rc<RefCell<HashMap<u32, Promise<Value>>>>,
    /// [`release_fns`] for this kernel, registered with
    /// [`runtime::at_exit`], which holds it weakly: it runs for as long as
    /// this kernel lives.
    _release_fns: Rc<dyn Fn()>,
}

#[wasm_bindgen]
impl WasmKernel {
    /// `plugins_json` is `[{"factory": ..., "config": ...}, ...]`; `engine`
    /// is the object `createEngineBridge` returns; `call_js(id, argsJson)`
    /// calls a `{"$fn": "js:<id>"}` function and `release_js(id)` says the
    /// kernel holds it no more. Throws the canonical error if a plugin does
    /// not build or the kernel refuses the list.
    #[wasm_bindgen(constructor)]
    pub fn new(
        plugins_json: &str,
        engine: JsValue,
        call_js: js_sys::Function,
        release_js: js_sys::Function,
    ) -> Result<WasmKernel, JsValue> {
        report_panics();
        // Before anything can schedule a timer.
        runtime::start_clock();
        entry(|| {
            WasmKernel::build(plugins_json, engine, call_js, release_js).map_err(|e| thrown(&e))
        })
    }

    fn build(
        plugins_json: &str,
        engine: JsValue,
        call_js: js_sys::Function,
        release_js: js_sys::Function,
    ) -> Result<WasmKernel, Error> {
        let adapters = adapters();
        let mut tokens: HashMap<String, AnyToken> = HashMap::new();
        tokens.insert(DOCUMENTS.name().to_string(), DOCUMENTS.any().clone());
        for adapter in &adapters {
            for token in adapter.tokens() {
                tokens.insert(token.name().to_string(), token);
            }
        }
        let released = Rc::new(RefCell::new(Vec::new()));
        let pending = released.clone();
        let release: ReleaseFn = Rc::new(move |name| pending.borrow_mut().push(name.to_string()));
        let harness = Harness::live_with_release(adapters.clone(), tokens, live(call_js), release);
        let release_fns: Rc<dyn Fn()> = {
            let harness = harness.clone();
            Rc::new(move || release_fns(&harness, &released, &release_js))
        };
        runtime::at_exit(&release_fns);

        let specs: Vec<Value> = serde_json::from_str(plugins_json)
            .map_err(|e| Error::named("TypeError", format!("malformed plugin list: {e}")))?;
        let mut plugins = Vec::new();
        for spec in &specs {
            let factory = spec["factory"].as_str().unwrap_or_default();
            let config = spec.get("config").cloned().unwrap_or_else(undefined);
            let adapter = adapters.iter().find(|a| a.factories().contains(&factory));
            let Some(adapter) = adapter else {
                return Err(Error::new(format!("[epdf-web] no plugin factory \"{factory}\"")));
            };
            plugins.push(adapter.build(factory, &config, &harness)?);
        }

        let options = KernelOptions {
            report: Some(Rc::new(|error: &Error| {
                console_error("[kernel]", &runtime::error_to_js(error))
            })),
            on_dispatch: None,
            release_closed_sessions: true,
        };
        let bridge: EngineBridge = engine.unchecked_into();
        let kernel = Kernel::new(Rc::new(JsEngine::new(bridge)), plugins, options)?;
        // The capability `kernel.capability(DocumentsToken)` answers, so the
        // two describe as one proxy with one set of pinned refs.
        let documents = kernel.capability_any(DOCUMENTS.any(), None)?;
        let memo = Rc::new(Memo {
            harness: harness.clone(),
            described: RefCell::new(HashMap::new()),
            evicted: RefCell::new(Vec::new()),
        });
        // Registered before the shim subscribes its own `onClosed` (which
        // reads `memo.evicted` back out via `take_evicted`), so on a native
        // `EventHookSource` that calls listeners in registration order this
        // one fills the list before that one drains it. `Wire.capability`/
        // `tryCapability` in `packages/core-wasm/src/proxy.ts` also drain
        // `take_evicted`, on the next capability lookup, so nothing on the
        // JS side actually depends on this order holding.
        let on_closed = kernel.documents().events().map(|events| {
            let memo = Rc::downgrade(&memo);
            events.closed.on(move |event: &Value| {
                let doc = event.get("documentId").and_then(Value::as_str);
                if let (Some(memo), Some(doc)) = (memo.upgrade(), doc) {
                    memo.evict(doc);
                }
                Ok(())
            })
        });
        Ok(WasmKernel {
            kernel,
            harness,
            subscriptions: RefCell::new(HashMap::new()),
            next_subscription: Cell::new(0),
            memo,
            on_closed,
            documents,
            aborts: Rc::new(RefCell::new(HashMap::new())),
            _release_fns: release_fns,
        })
    }

    pub fn start(&self) -> js_sys::Promise {
        entry(|| runtime::to_js(self.kernel.start().map(|_| Ok(undefined()))))
    }

    /// `kernel.destroy()`. The shim calls `free()` once this settles, and
    /// nothing on this kernel after that.
    pub fn destroy(&self) -> js_sys::Promise {
        entry(|| runtime::to_js(self.kernel.destroy().map(|_| Ok(undefined()))))
    }

    /// The description texts evicted since the last call, as a JSON array:
    /// the capabilities of documents that have closed. Their refs are no
    /// longer pinned, so the shim drops the proxy it built from each text
    /// and releases the refs once their functions are collected.
    pub fn take_evicted(&self) -> String {
        entry(|| {
            let texts = std::mem::take(&mut *self.memo.evicted.borrow_mut());
            Value::from(texts).to_string()
        })
    }

    /// `kernel.capability(token, documentId?)`, described. Throws the
    /// canonical error the kernel refuses with.
    pub fn capability(
        &self,
        token_name: String,
        doc_id: Option<String>,
    ) -> Result<String, JsValue> {
        entry(|| {
            let token = self.token(&token_name);
            let doc = self.document_of(&token, doc_id.as_deref());
            self.kernel
                .capability_any(&token, doc_id.as_deref())
                .map(|capability| self.describe(capability, doc))
                .map_err(|e| thrown(&e))
        })
    }

    /// `kernel.tryCapability(token, documentId?)`, described, or `null`
    /// while the document is not ready. Throws the canonical error the
    /// kernel refuses with.
    pub fn try_capability(
        &self,
        token_name: String,
        doc_id: Option<String>,
    ) -> Result<Option<String>, JsValue> {
        entry(|| {
            let token = self.token(&token_name);
            let doc = self.document_of(&token, doc_id.as_deref());
            self.kernel
                .try_capability_any(&token, doc_id.as_deref())
                .map(|capability| capability.map(|c| self.describe(c, doc)))
                .map_err(|e| thrown(&e))
        })
    }

    /// `kernel.scopeOf(token)`: `"document"` or `"workspace"`.
    pub fn scope_of(&self, token_name: String) -> String {
        entry(|| {
            let token = self.token(&token_name);
            match self.kernel.scope_of(&token) {
                PluginScope::Document => "document".into(),
                PluginScope::Workspace => "workspace".into(),
            }
        })
    }

    /// `kernel.status()`.
    pub fn status(&self) -> String {
        entry(|| self.kernel.status().as_str().into())
    }

    /// `kernel.documents()`, described as [`capability`](Self::capability)
    /// describes one.
    pub fn documents(&self) -> String {
        entry(|| self.describe(self.documents.clone(), None))
    }

    /// Call a ref with canonical JSON arguments. Throws only if the
    /// arguments are not a JSON array.
    pub fn invoke_ref(&self, id: u32, args_json: String) -> Result<String, JsValue> {
        entry(|| {
            let args = parse_args(&args_json).map_err(|e| thrown(&e))?;
            let outcome = match self.harness.ref_fn(id as usize) {
                Some(f) => f(args),
                None => {
                    let error = if self.harness.ref_was_released(id) {
                        Error::new(format!("[epdf-web] ref {id} was called after its release"))
                    } else {
                        Error::new(format!("[epdf-web] no ref {id}"))
                    };
                    console_error("[epdf-web]", &runtime::error_to_js(&error));
                    Outcome::Threw(error)
                }
            };
            Ok(self.harness.outcome_text_raw(outcome))
        })
    }

    /// Free ref `id`: the shim calls this once the function it built from
    /// the ref has been garbage collected, so nothing can call it again. A
    /// pinned ref (a capability's own member) and an unknown or already
    /// released id are ignored.
    pub fn release_ref(&self, id: u32) {
        entry(|| self.harness.release_ref(id))
    }

    /// A promise id as a JavaScript promise, taken from the harness's live
    /// table: `id` answers exactly once, so a second call throws
    /// `[epdf-web] promise n was already taken` (also thrown for an id this
    /// kernel never tracked). It resolves with the value's canonical form as
    /// a JavaScript value (a top-level `undefined` as `undefined`) and
    /// rejects with the canonical `{"$error": ...}`.
    /// [`abort_promise`](Self::abort_promise) still works after the take:
    /// this kernel keeps the taken promise's abort handle, until it settles,
    /// in its own table.
    pub fn promise(&self, id: u32) -> Result<js_sys::Promise, JsValue> {
        entry(|| {
            let promise = self.harness.take_promise(id).ok_or_else(|| {
                thrown(&Error::new(format!("[epdf-web] promise {id} was already taken")))
            })?;
            self.aborts.borrow_mut().insert(id, promise.clone());
            let aborts = self.aborts.clone();
            promise.on_settle(move |_| {
                aborts.borrow_mut().remove(&id);
            });
            Ok(runtime::to_js(promise))
        })
    }

    /// Abort a promise id, as its `AbortablePromise.abort(reason)` would.
    /// `reason_json` is the reason's canonical JSON; an id
    /// [`promise`](Self::promise) has not been called for, or whose promise
    /// already settled, does nothing. Throws a `TypeError` if `reason_json`
    /// is not JSON.
    pub fn abort_promise(&self, id: u32, reason_json: Option<String>) -> Result<(), JsValue> {
        entry(|| {
            let reason = match reason_json {
                Some(text) => Some(serde_json::from_str(&text).map_err(|e| {
                    thrown(&Error::named("TypeError", format!("malformed abort reason: {e}")))
                })?),
                None => None,
            };
            let promise = self.aborts.borrow().get(&id).cloned();
            if let Some(promise) = promise {
                promise.abort(reason);
            }
            Ok(())
        })
    }

    pub fn state_json(&self) -> String {
        entry(|| self.kernel.state_text().to_string())
    }

    /// Moves whenever [`state_json`](Self::state_json) could answer
    /// differently. A plain read, so it skips [`entry`]: nothing it does can
    /// queue work or reach JavaScript.
    pub fn state_revision(&self) -> f64 {
        self.kernel.state_revision() as f64
    }

    /// Moves whenever the documents, and so the capabilities
    /// [`try_capability`](Self::try_capability) can answer, might have
    /// changed. A plain read, like [`state_revision`](Self::state_revision).
    pub fn core_revision(&self) -> f64 {
        self.kernel.core_revision() as f64
    }

    /// `{"core": n, "plugins": {"<key>": n}}`: which parts of
    /// [`state_json`](Self::state_json) moved since a reader last looked.
    /// Plain reads, like [`state_revision`](Self::state_revision).
    pub fn state_revisions(&self) -> String {
        self.kernel.state_revisions()
    }

    /// `state_json().core`, as text.
    pub fn core_text(&self) -> String {
        self.kernel.core_text().to_string()
    }

    /// `state_json().plugins[key]`, as text; `undefined` for no such slice.
    pub fn slice_text(&self, key: &str) -> Option<String> {
        self.kernel.slice_text(key).map(|text| text.to_string())
    }

    /// Call `listener()` after every store change until
    /// [`unsubscribe`](Self::unsubscribe) with the id this returns. A throw
    /// is reported.
    pub fn subscribe(&self, listener: js_sys::Function) -> u32 {
        entry(|| {
            let off = self.kernel.subscribe(Rc::new(move || {
                listener.call0(&JsValue::NULL).map(|_| ()).map_err(|e| js_to_error(&e))
            }));
            let id = self.next_subscription.get();
            self.next_subscription.set(id + 1);
            self.subscriptions.borrow_mut().insert(id, off);
            id
        })
    }

    /// Whether a live callback made from `{"$fn": "js:<id>"}` holds that
    /// function. The shim forgets a function it handed over once a task has
    /// passed with no callback made from it (`Wire.sweep` in
    /// `packages/core-wasm/src/proxy.ts`): one the kernel ignored, so no
    /// `release_js` would ever come for it.
    pub fn holds_js(&self, id: String) -> bool {
        entry(|| self.harness.holds_fn(&format!("js:{id}")))
    }

    pub fn unsubscribe(&self, id: u32) {
        entry(|| {
            let off = self.subscriptions.borrow_mut().remove(&id);
            if let Some(off) = off {
                off.call();
            }
        })
    }

    /// The harness's live-table counts plus how many capabilities this
    /// kernel has described (`describe`'s memo), how many taken promises
    /// it keeps abortable (`aborts`, which holds a never-settling promise's
    /// entry for good) and how many tokens the harness knows by name:
    /// `{"refs": n, "promises": n, "heldFns": n, "described": n,
    /// "aborts": n, "tokens": n}`. Dev-only, for the eval build's memory
    /// soak test.
    pub fn debug_counts(&self) -> String {
        entry(|| {
            let mut counts = self.harness.counts();
            counts["described"] = json!(self.memo.described.borrow().len());
            counts["aborts"] = json!(self.aborts.borrow().len());
            counts["tokens"] = json!(self.harness.token_count());
            counts.to_string()
        })
    }
}

impl WasmKernel {
    /// The token named `name`. An unknown name gets a token of its own that
    /// is not kept: the kernel refuses it all the same, and a caller asking
    /// for made-up names must not grow the harness's table.
    fn token(&self, name: &str) -> AnyToken {
        self.harness.find_token(name).unwrap_or_else(|| AnyToken::new(name, None))
    }

    /// The document a capability of `token` asked for with `doc_id`
    /// resolves to: `doc_id`, or the active document for a document
    /// plugin's token, and `None` for a workspace plugin's.
    fn document_of(&self, token: &AnyToken, doc_id: Option<&str>) -> Option<String> {
        match self.kernel.scope_of(token) {
            PluginScope::Workspace => None,
            PluginScope::Document => {
                doc_id.map(str::to_string).or_else(|| self.kernel.documents().get_active_id())
            }
        }
    }

    /// The harness's description of `capability` (of document `doc`), made
    /// once per capability, with every ref in it pinned.
    fn describe(&self, capability: Rc<dyn Any>, doc: Option<String>) -> String {
        let key = Rc::as_ptr(&capability) as *const ();
        if let Some(entry) = self.memo.described.borrow().get(&key) {
            return entry.text.clone();
        }
        let mut description =
            self.harness.describe(capability.clone()).unwrap_or_else(|| json!({}));
        let mut refs = Vec::new();
        self.pin_refs(&mut description, &mut refs);
        let text = description.to_string();
        let entry = Described { _capability: capability, doc, text: text.clone(), refs };
        self.memo.described.borrow_mut().insert(key, entry);
        text
    }

    /// Pin every `{"$ref": n}` in `value`, into `refs`, and mark it
    /// `"pinned": true`, so the shim does not ask to release it.
    fn pin_refs(&self, value: &mut Value, refs: &mut Vec<u32>) {
        match value {
            Value::Object(object) => {
                if let Some(id) = object.get("$ref").and_then(Value::as_u64) {
                    self.harness.pin_ref(id as u32);
                    refs.push(id as u32);
                    object.insert("pinned".into(), Value::Bool(true));
                    return;
                }
                object.values_mut().for_each(|member| self.pin_refs(member, refs));
            }
            Value::Array(items) => items.iter_mut().for_each(|item| self.pin_refs(item, refs)),
            _ => {}
        }
    }
}

/// What `free()` runs: the listeners come off, and the memo, the abort
/// table and the harness's refs and promises go, so nothing this kernel
/// built keeps the rest alive (see the module's doc).
impl Drop for WasmKernel {
    fn drop(&mut self) {
        entry(|| {
            if let Some(off) = self.on_closed.take() {
                off.call();
            }
            let subscriptions = std::mem::take(&mut *self.subscriptions.borrow_mut());
            subscriptions.values().for_each(Unsubscribe::call);
            let described = std::mem::take(&mut *self.memo.described.borrow_mut());
            let aborts = std::mem::take(&mut *self.aborts.borrow_mut());
            self.memo.evicted.borrow_mut().clear();
            drop((described, aborts, subscriptions));
            self.harness.clear_live();
        })
    }
}
