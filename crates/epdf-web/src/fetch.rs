//! The host's `fetch` for the kernel's URL sources
//! (`epdf_kernel::fetch::HostFetch`), as `packages/core-wasm/src/kernel.ts`
//! (`urlFetch`) hands it over: `fetchUrl(url)` starts one download and
//! answers `{response, body, abort}`. `response` resolves `{ok, status}`
//! (or `{noFetch: true}` where there is no `fetch()`), `body()` resolves
//! `{bytes: {"$host": n}, head: [...]}`, the file kept on the JavaScript
//! side, and `abort(reason)` stops the download when the tab closes.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::fetch::{FetchResponse, FetchedBytes, HostFetch, no_fetch};
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::runtime::{self, error_to_js, js_to_error};

/// `fetchUrl`, the function core-wasm built.
pub struct JsFetch(pub js_sys::Function);

fn member(object: &JsValue, key: &str) -> Option<JsValue> {
    js_sys::Reflect::get(object, &JsValue::from_str(key)).ok().filter(|v| !v.is_undefined())
}

fn function(object: &JsValue, key: &str) -> Result<js_sys::Function, Error> {
    member(object, key)
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .ok_or_else(|| Error::new(format!("[epdf-web] fetchUrl answered no {key}()")))
}

/// A JavaScript value, or a promise of one, as a Rust promise of its JSON.
fn settled(value: JsValue) -> Promise<Value> {
    runtime::from_js(js_sys::Promise::resolve(&value))
}

impl HostFetch for JsFetch {
    fn fetch(&self, url: &str, signal: AbortSignal) -> Promise<Rc<dyn FetchResponse>> {
        let call = match self.0.call1(&JsValue::NULL, &JsValue::from_str(url)) {
            Ok(call) => call,
            Err(thrown) => return Promise::rejected(js_to_error(&thrown)),
        };
        let (abort, body) = match (function(&call, "abort"), function(&call, "body")) {
            (Ok(abort), Ok(body)) => (abort, body),
            (Err(error), _) | (_, Err(error)) => return Promise::rejected(error),
        };
        let stop = move |reason: &Error| {
            let _ = abort.call1(&JsValue::NULL, &error_to_js(reason));
        };
        match signal.reason() {
            Some(reason) => stop(&reason),
            None => {
                signal.on_abort(Rc::new(stop));
            }
        }
        let url = url.to_string();
        settled(member(&call, "response").unwrap_or(JsValue::UNDEFINED)).map(move |response| {
            if response.get("noFetch").and_then(Value::as_bool) == Some(true) {
                return Err(no_fetch(&url));
            }
            let ok = response.get("ok").and_then(Value::as_bool).unwrap_or(false);
            let status = response.get("status").and_then(Value::as_f64).unwrap_or(f64::NAN);
            Ok(Rc::new(JsResponse { ok, status, body }) as Rc<dyn FetchResponse>)
        })
    }
}

struct JsResponse {
    ok: bool,
    status: f64,
    body: js_sys::Function,
}

impl FetchResponse for JsResponse {
    fn ok(&self) -> bool {
        self.ok
    }

    fn status(&self) -> f64 {
        self.status
    }

    fn array_buffer(&self) -> Promise<FetchedBytes> {
        let answer = match self.body.call0(&JsValue::NULL) {
            Ok(answer) => answer,
            Err(thrown) => return Promise::rejected(js_to_error(&thrown)),
        };
        settled(answer).map(|file| {
            let bytes = file
                .get("bytes")
                .cloned()
                .ok_or_else(|| Error::new("[epdf-web] body() answered no bytes"))?;
            let head = file
                .get("head")
                .and_then(Value::as_array)
                .map(|head| head.iter().filter_map(Value::as_u64).map(|b| b as u8).collect())
                .unwrap_or_default();
            Ok(FetchedBytes { bytes, head })
        })
    }
}
