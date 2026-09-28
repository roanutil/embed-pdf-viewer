//! What JavaScript sees: [`WasmKernel`], a kernel built from plugin specs
//! through the conformance adapters and driven by name.
//!
//! Values cross as canonical JSON text (`{"$undefined": true}`,
//! `{"$number": "NaN"}`, `{"$bytes": ...}` intact, never scrubbed). A
//! capability reads as the harness describes it: member name to
//! `{"$ref": n}` or a plain value. A ref is called with [`WasmKernel::invoke_ref`],
//! whose outcome is `{"returned": v}`, `{"threw": e}` or `{"promise": n}`,
//! and a promise id becomes a JavaScript promise with [`WasmKernel::promise`].
//! The harness keeps every ref and promise forever; for a session of manual
//! testing that is fine.
//!
//! A JavaScript function in a plugin config or a call's arguments arrives as
//! `{"$fn": "js:<id>"}`. Calling it calls `call_js(id, argsJson)` with the
//! `<id>` text; its return value, throw or promise is the callback's answer.
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
//! `Rc`, which the cache holds so its address is never reused.
//!
//! Every export's body runs as a [`runtime::entry`], so the kernel's queue
//! drains and its timers stay armed once the outermost one returns.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Once;

use epdf_kernel::types::PluginScope;
use epdf_kernel::{
    AnyToken, DOCUMENTS, Error, Kernel, KernelOptions, Promise, Unsubscribe, undefined,
};
use plugin_conformance::{Answer, Harness, LiveFn, Outcome};
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
type Described = HashMap<*const (), (Rc<dyn Any>, String)>;

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
    described: RefCell<Described>,
    /// `kernel.documents()`, made once so its description memoizes too.
    documents: Rc<dyn Any>,
}

#[wasm_bindgen]
impl WasmKernel {
    /// `plugins_json` is `[{"factory": ..., "config": ...}, ...]`; `engine`
    /// is the object `createEngineBridge` returns. Throws the canonical
    /// error if a plugin does not build or the kernel refuses the list.
    #[wasm_bindgen(constructor)]
    pub fn new(
        plugins_json: &str,
        engine: JsValue,
        call_js: js_sys::Function,
    ) -> Result<WasmKernel, JsValue> {
        report_panics();
        // Before anything can schedule a timer.
        runtime::start_clock();
        entry(|| WasmKernel::build(plugins_json, engine, call_js).map_err(|e| thrown(&e)))
    }

    fn build(
        plugins_json: &str,
        engine: JsValue,
        call_js: js_sys::Function,
    ) -> Result<WasmKernel, Error> {
        let adapters = adapters();
        let mut tokens: HashMap<String, AnyToken> = HashMap::new();
        tokens.insert(DOCUMENTS.name().to_string(), DOCUMENTS.any().clone());
        for adapter in &adapters {
            for token in adapter.tokens() {
                tokens.insert(token.name().to_string(), token);
            }
        }
        let harness = Harness::live(adapters.clone(), tokens, live(call_js));

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
        };
        let bridge: EngineBridge = engine.unchecked_into();
        let kernel = Kernel::new(Rc::new(JsEngine::new(bridge)), plugins, options)?;
        let documents: Rc<dyn Any> = Rc::new(kernel.documents());
        Ok(WasmKernel {
            kernel,
            harness,
            subscriptions: RefCell::new(HashMap::new()),
            next_subscription: Cell::new(0),
            described: RefCell::new(HashMap::new()),
            documents,
        })
    }

    pub fn start(&self) -> js_sys::Promise {
        entry(|| runtime::to_js(self.kernel.start().map(|_| Ok(undefined()))))
    }

    pub fn destroy(&self) -> js_sys::Promise {
        entry(|| runtime::to_js(self.kernel.destroy().map(|_| Ok(undefined()))))
    }

    /// `kernel.capability(token, documentId?)`, described. Throws the
    /// canonical error the kernel refuses with.
    pub fn capability(
        &self,
        token_name: String,
        doc_id: Option<String>,
    ) -> Result<String, JsValue> {
        entry(|| {
            let token = self.harness.token(&token_name, None);
            self.kernel
                .capability_any(&token, doc_id.as_deref())
                .map(|capability| self.describe(capability))
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
            let token = self.harness.token(&token_name, None);
            self.kernel
                .try_capability_any(&token, doc_id.as_deref())
                .map(|capability| capability.map(|c| self.describe(c)))
                .map_err(|e| thrown(&e))
        })
    }

    /// `kernel.scopeOf(token)`: `"document"` or `"workspace"`.
    pub fn scope_of(&self, token_name: String) -> String {
        entry(|| {
            let token = self.harness.token(&token_name, None);
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
        entry(|| self.describe(self.documents.clone()))
    }

    /// Call a ref with canonical JSON arguments. Throws only if the
    /// arguments are not a JSON array.
    pub fn invoke_ref(&self, id: u32, args_json: String) -> Result<String, JsValue> {
        entry(|| {
            let args = parse_args(&args_json).map_err(|e| thrown(&e))?;
            let outcome = match self.harness.ref_fn(id as usize) {
                Some(f) => f(args),
                None => Outcome::Threw(Error::new(format!("[epdf-web] no ref {id}"))),
            };
            Ok(self.harness.outcome_json_raw(outcome).to_string())
        })
    }

    /// A promise id as a JavaScript promise. It resolves with the value's
    /// canonical form as a JavaScript value (a top-level `undefined` as
    /// `undefined`) and rejects with the canonical `{"$error": ...}`.
    pub fn promise(&self, id: u32) -> js_sys::Promise {
        entry(|| {
            let promise = self.harness.promise(id as usize).unwrap_or_else(|| {
                Promise::rejected(Error::new(format!("[epdf-web] no promise {id}")))
            });
            runtime::to_js(promise)
        })
    }

    /// Abort a promise id, as its `AbortablePromise.abort(reason)` would.
    /// `reason_json` is the reason's canonical JSON; an unknown id does
    /// nothing. Throws a `TypeError` if `reason_json` is not JSON.
    pub fn abort_promise(&self, id: u32, reason_json: Option<String>) -> Result<(), JsValue> {
        entry(|| {
            let reason = match reason_json {
                Some(text) => Some(serde_json::from_str(&text).map_err(|e| {
                    thrown(&Error::named("TypeError", format!("malformed abort reason: {e}")))
                })?),
                None => None,
            };
            if let Some(promise) = self.harness.promise(id as usize) {
                promise.abort(reason);
            }
            Ok(())
        })
    }

    pub fn state_json(&self) -> String {
        entry(|| self.kernel.state_json().to_string())
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

    pub fn unsubscribe(&self, id: u32) {
        entry(|| {
            let off = self.subscriptions.borrow_mut().remove(&id);
            if let Some(off) = off {
                off.call();
            }
        })
    }
}

impl WasmKernel {
    /// The harness's description of `capability`, made once per capability.
    fn describe(&self, capability: Rc<dyn Any>) -> String {
        let key = Rc::as_ptr(&capability) as *const ();
        if let Some((_, text)) = self.described.borrow().get(&key) {
            return text.clone();
        }
        let text =
            self.harness.describe(capability.clone()).unwrap_or_else(|| json!({})).to_string();
        self.described.borrow_mut().insert(key, (capability, text.clone()));
        text
    }
}
