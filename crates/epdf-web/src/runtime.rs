//! The kernel's runtime on a JavaScript event loop.
//!
//! `rt` only moves when a host drains it and advances its clock. Here the
//! host is the page: every export ends with [`after_entry`], which drains the
//! ready queue and arms one real `setTimeout` for the earliest virtual timer.
//! When that fires, the virtual clock advances by the real time that passed.
//! The virtual clock trails wall time by at most one timer's lateness.
//!
//! Promises cross in both directions as JSON values, with a rejection as the
//! kernel's canonical `{"$error": {name, message, ...}}`. A JavaScript `await`
//! on a converted promise runs on the JavaScript microtask queue, not the
//! Rust ready queue, so ordering across the boundary is not the order the
//! vectors pin.

use std::cell::RefCell;

use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, is_undefined, undefined};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &Closure<dyn FnMut()>, ms: f64) -> JsValue;
    #[wasm_bindgen(js_name = clearTimeout)]
    fn clear_timeout(handle: &JsValue);
    #[wasm_bindgen(js_namespace = performance, js_name = now)]
    fn performance_now() -> f64;
}

/// The one armed `setTimeout`.
struct Armed {
    handle: JsValue,
    /// The virtual deadline it wakes for.
    deadline: f64,
    /// Kept alive until it fires or is cleared.
    _callback: Closure<dyn FnMut()>,
}

thread_local! {
    static ARMED: RefCell<Option<Armed>> = const { RefCell::new(None) };
}

/// Call at the end of every export: drain the ready queue, then make sure a
/// real timeout is armed for the earliest virtual timer, if there is one.
pub fn after_entry() {
    rt::run_until_stalled();
    let next = rt::next_deadline();
    let keep = ARMED.with(|armed| {
        let armed = armed.borrow();
        match (armed.as_ref(), next) {
            (Some(armed), Some(next)) => armed.deadline <= next,
            _ => false,
        }
    });
    if keep {
        return;
    }
    if let Some(stale) = ARMED.with(|armed| armed.borrow_mut().take()) {
        clear_timeout(&stale.handle);
    }
    let Some(deadline) = next else { return };
    let armed_at = performance_now();
    let virtual_at = rt::now();
    let callback = Closure::<dyn FnMut()>::new(move || {
        // Dropped at the end, not while this closure is still running.
        let spent = ARMED.with(|armed| armed.borrow_mut().take());
        // A timeout can fire a little early; the timer it was armed for is
        // due either way.
        let elapsed = (performance_now() - armed_at).max(deadline - virtual_at);
        rt::advance((virtual_at + elapsed - rt::now()).max(0.0));
        after_entry();
        drop(spent);
    });
    let handle = set_timeout(&callback, (deadline - virtual_at).max(0.0));
    ARMED.with(|armed| {
        *armed.borrow_mut() = Some(Armed { handle, deadline, _callback: callback });
    });
}

/// A Rust promise as a JavaScript one. A rejection carries the canonical
/// `{"$error": ...}` object.
pub fn to_js(p: Promise<Value>) -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, reject| {
        p.on_settle(move |result| {
            let _ = match result {
                Ok(value) => resolve.call1(&JsValue::UNDEFINED, &value_to_js(value)),
                Err(error) => {
                    reject.call1(&JsValue::UNDEFINED, &value_to_js(&error.to_canonical()))
                }
            };
        });
    })
}

/// A JavaScript promise as a Rust one. Its settlement arrives on the
/// JavaScript microtask queue, which then runs [`after_entry`].
pub fn from_js(p: js_sys::Promise) -> Promise<Value> {
    let (promise, resolver) = Promise::pending();
    wasm_bindgen_futures::spawn_local(async move {
        let result = JsFuture::from(p).await;
        resolver.settle(result.map(|value| js_to_value(&value)).map_err(|e| js_to_error(&e)));
        after_entry();
    });
    promise
}

fn value_to_js(value: &Value) -> JsValue {
    if is_undefined(value) {
        return JsValue::UNDEFINED;
    }
    let json = serde_json::to_string(value).expect("a JSON value serializes");
    js_sys::JSON::parse(&json).expect("JSON.parse reads what serde_json wrote")
}

fn js_to_value(value: &JsValue) -> Value {
    if value.is_undefined() {
        return undefined();
    }
    js_sys::JSON::stringify(value)
        .ok()
        .and_then(|json| json.as_string())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_else(undefined)
}

/// What a JavaScript promise rejected with: a canonical `{"$error": ...}`,
/// an `Error` (its `name`, `message` and any `code`), or anything else as
/// an `Error` whose message is the value.
fn js_to_error(value: &JsValue) -> Error {
    if let Some(error) = value.dyn_ref::<js_sys::Error>() {
        let mut out = Error::named(String::from(error.name()), String::from(error.message()));
        let code = js_sys::Reflect::get(value, &JsValue::from_str("code")).ok();
        if let Some(code) = code.filter(|c| !c.is_undefined()) {
            out = out.with("code", js_to_value(&code));
        }
        return out;
    }
    let spec = js_to_value(value);
    if spec.get("$error").is_some() {
        return Error::from_spec(&spec);
    }
    Error::new(match spec {
        Value::String(message) => message,
        other => other.to_string(),
    })
}
