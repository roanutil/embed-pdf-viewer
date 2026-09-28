//! The kernel's `Engine` and `DocumentHandle` over a JavaScript engine
//! bridge: `createEngineBridge(engine)` from
//! `packages/core-wasm/src/engine-bridge.ts`, which forwards to
//! `@embedpdf/engine`.
//!
//! The envelope is the one `epdf_kernel::engine` defines, and the bridge
//! speaks it in canonical JSON text both ways, so this side only moves
//! strings: arguments out, results and events back. A rejection arrives as
//! the canonical `{"$error": ...}`, which [`runtime::from_js`] reads back to
//! an `Error`; one named `PermissionDenied` gets its class back the way the
//! conformance fake gives it (`fake_engine::engine_error`), since the kernel
//! maps engine refusals by class.
//!
//! Synchronous reads (`capabilities`, the security selectors, `page(ref)`)
//! call the bridge each time rather than caching: `security.current` changes
//! after an unlock. A read that throws (the document is gone) answers the
//! refusing default: nothing allowed, no identity, no prompt.

use std::rc::Rc;

use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Error, Promise};
use plugin_conformance::fake_engine::engine_error;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::runtime::{self, js_to_error, value_to_js};

#[wasm_bindgen]
extern "C" {
    /// The object `createEngineBridge` returns.
    #[derive(Clone)]
    pub type EngineBridge;

    #[wasm_bindgen(method, catch)]
    fn open(
        this: &EngineBridge,
        input: &str,
        options: Option<String>,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn call(
        this: &EngineBridge,
        doc: &str,
        path: &str,
        pon: Option<u32>,
        args: &str,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn has(this: &EngineBridge, doc: &str, member: &str) -> Result<bool, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn capabilities(this: &EngineBridge, doc: &str) -> Result<String, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn security(
        this: &EngineBridge,
        doc: &str,
        member: &str,
        args: &str,
    ) -> Result<String, JsValue>;
    #[wasm_bindgen(method, catch, js_name = pageIndex)]
    fn page_index(this: &EngineBridge, doc: &str, pon: u32) -> Result<f64, JsValue>;
    #[wasm_bindgen(method, catch, js_name = lastServerId)]
    fn last_server_id(this: &EngineBridge, doc: &str) -> Result<Option<String>, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn subscribe(
        this: &EngineBridge,
        doc: &str,
        listener: &Closure<dyn FnMut(String)>,
    ) -> Result<js_sys::Function, JsValue>;
    #[wasm_bindgen(method, catch)]
    fn destroy(this: &EngineBridge) -> Result<js_sys::Promise, JsValue>;
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn console_error(message: &str);
}

/// What `page_index` answers for a page the engine does not index. The
/// local engine's `page(ref)` never throws and reports `pageIndex: -1`
/// (display order lives on `pages.list()`); every plugin caller only asks
/// whether the call succeeded.
pub const UNINDEXED: u32 = u32::MAX;

fn refused(error: Error) -> Error {
    engine_error(&error.to_canonical())
}

fn args_json(args: &[Value]) -> String {
    Value::Array(args.to_vec()).to_string()
}

/// A bridge call as an engine call: the answer's JSON text parsed, a
/// rejection mapped by [`refused`], and the bridge promise's `abort` wired
/// to [`Promise::abort`]. An abort rejects at once with `AbortError`, as
/// `AbortablePromise.abort` does, and the bridge's own later rejection is
/// dropped.
fn engine_call(started: Result<js_sys::Promise, JsValue>) -> EngineCall<Value> {
    let js = match started {
        Ok(js) => js,
        Err(thrown) => return Promise::rejected(refused(js_to_error(&thrown))),
    };
    let abort = js_sys::Reflect::get(&js, &JsValue::from_str("abort"))
        .ok()
        .and_then(|abort| abort.dyn_into::<js_sys::Function>().ok());
    let (promise, resolver) = Promise::<Value>::pending();
    let settle = resolver.clone();
    runtime::from_js(js.clone()).on_settle(move |result| {
        settle.settle(match result {
            Ok(Value::String(text)) => serde_json::from_str(text)
                .map_err(|e| Error::new(format!("[engine-bridge] malformed answer: {e}"))),
            Ok(other) => {
                Err(Error::new(format!("[engine-bridge] answer is not JSON text: {other}")))
            }
            Err(error) => Err(refused(error.clone())),
        })
    });
    let Some(abort) = abort else { return promise };
    promise.with_abort(move |reason| {
        // The kernel's promise may have already settled from `on_settle`
        // above by the time the abort handle runs; do not call the bridge's
        // `abort` (or reject a second time) once the race is decided.
        if resolver.promise().is_settled() {
            return;
        }
        let why = reason.as_ref().map_or(JsValue::UNDEFINED, value_to_js);
        let _ = abort.call1(&js, &why);
        resolver.reject(Error::abort(reason));
    })
}

/// `engine.open` and `engine.destroy` through the bridge.
pub struct JsEngine {
    bridge: EngineBridge,
}

impl JsEngine {
    pub fn new(bridge: EngineBridge) -> JsEngine {
        JsEngine { bridge }
    }
}

impl Engine for JsEngine {
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let raw = engine_call(
            self.bridge.open(&input.to_string(), options.as_ref().map(Value::to_string)),
        );
        let abort = raw.abort_handle();
        let (promise, resolver) = Promise::<Rc<dyn DocumentHandle>>::pending();
        let bridge = self.bridge.clone();
        raw.on_settle(move |result| {
            resolver.settle(result.clone().and_then(|id| match id {
                Value::String(id) => {
                    Ok(Rc::new(JsDocumentHandle { bridge, id }) as Rc<dyn DocumentHandle>)
                }
                other => Err(Error::new(format!("[engine-bridge] open answered {other}"))),
            }))
        });
        match abort {
            Some(abort) => promise.with_abort(move |reason| abort(reason)),
            None => promise,
        }
    }

    fn destroy(&self) -> EngineCall<()> {
        engine_call(self.bridge.destroy()).map(|_| Ok(()))
    }
}

/// One opened document, by the id the bridge keys it under.
pub struct JsDocumentHandle {
    bridge: EngineBridge,
    id: String,
}

/// A live `events.subscribe` registration. Dropping it unsubscribes, so the
/// closure is never called after it is freed.
struct Subscription {
    off: js_sys::Function,
    _listener: Closure<dyn FnMut(String)>,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = self.off.call0(&JsValue::UNDEFINED);
    }
}

impl DocumentHandle for JsDocumentHandle {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> Value {
        self.bridge
            .capabilities(&self.id)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_else(|| json!({}))
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(JsSecurity { bridge: self.bridge.clone(), doc: self.id.clone() })
    }

    fn has(&self, member: &str) -> bool {
        self.bridge.has(&self.id, member).unwrap_or(false)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        engine_call(self.bridge.call(&self.id, path, pon, &args_json(&args)))
    }

    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        let index = self
            .bridge
            .page_index(&self.id, pon)
            .map_err(|thrown| refused(js_to_error(&thrown)))?;
        Ok(if index >= 0.0 { index as u32 } else { UNINDEXED })
    }

    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        // An event is a JavaScript entry into the kernel, so it runs as an
        // `entry` like an export does.
        let closure = Closure::<dyn FnMut(String)>::new(move |text: String| {
            runtime::entry(|| match serde_json::from_str::<Value>(&text) {
                Ok(event) => listener(&event),
                Err(error) => {
                    console_error(&format!("[engine-bridge] malformed event {text:?}: {error}"))
                }
            })
        });
        match self.bridge.subscribe(&self.id, &closure) {
            Ok(off) => {
                let subscription = Subscription { off, _listener: closure };
                Unsubscribe::new(move || drop(subscription))
            }
            Err(thrown) => {
                console_error(&format!(
                    "[engine-bridge] subscribe failed: {}",
                    js_to_error(&thrown)
                ));
                Unsubscribe::noop()
            }
        }
    }

    fn last_server_id(&self) -> Option<String> {
        self.bridge.last_server_id(&self.id).ok().flatten()
    }
}

/// `doc.security`, read live through the bridge.
struct JsSecurity {
    bridge: EngineBridge,
    doc: String,
}

impl JsSecurity {
    fn read(&self, member: &str, args: &[Value]) -> Option<Value> {
        let text = self.bridge.security(&self.doc, member, &args_json(args)).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn check(&self, member: &str, args: &[Value]) -> bool {
        self.read(member, args).and_then(|v| v.as_bool()).unwrap_or(false)
    }
}

impl DocumentSecurity for JsSecurity {
    fn allows(&self, capability: &str) -> bool {
        self.check("allows", &[json!(capability)])
    }

    fn allows_annotation_create(&self) -> bool {
        self.check("allowsAnnotationCreate", &[])
    }

    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool {
        self.check("allowsAnnotationMutation", &[json!(action), target.clone()])
    }

    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool {
        self.check("allowsAnnotationGroupAssignment", &[json!(group_id)])
    }

    fn effective_scope(&self) -> Vec<String> {
        self.read("effectiveScope", &[])
            .and_then(|v| v.as_array().cloned())
            .map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect())
            .unwrap_or_default()
    }

    fn identity(&self) -> Value {
        self.read("identity", &[]).unwrap_or(Value::Null)
    }

    fn current(&self) -> Value {
        self.read("current", &[]).unwrap_or(Value::Null)
    }

    fn password_prompt(&self) -> Value {
        self.read("passwordPrompt", &[]).unwrap_or_else(|| json!({ "state": "none" }))
    }

    fn unlock(&self, input: Value) -> EngineCall<Value> {
        engine_call(self.bridge.call(&self.doc, "security.unlock", None, &args_json(&[input])))
    }
}
