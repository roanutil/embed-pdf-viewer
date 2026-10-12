//! An engine call aborted before its bridge promise settles: the kernel's
//! promise rejects at once, so the engine's late answer reaches nobody, and
//! every `$host` it carries is handed to `release_host`. An answer that
//! settles first is delivered and releases nothing. Its own test binary,
//! since `set_host_release` installs once per instance.

use epdf_kernel::engine::Engine;
use epdf_web::engine::{EngineBridge, JsEngine};
use epdf_web::exports::set_host_release;
use epdf_web::runtime::reset;
use serde_json::json;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = setTimeout)]
    fn set_timeout(handler: &js_sys::Function, ms: f64) -> JsValue;
}

async fn js_sleep(ms: f64) {
    let wait = js_sys::Promise::new(&mut |resolve, _| {
        set_timeout(&resolve, ms);
    });
    JsFuture::from(wait).await.unwrap();
}

/// A bridge whose `late` call ignores its abort and answers a value with
/// two `$host`s, one inside the other, a tick later; `prompt` answers one
/// at once. `releaseHost` records what it is handed.
const STUB: &str = r#"
const released = [];
const image = '{"$host":5,"width":2,"source":{"kind":"bytes","bytes":{"$hostBytes":4,"$byteLength":3}}}';
return {
  released,
  releaseHost: (n) => { released.push(n); },
  open(input) { return Promise.resolve(JSON.stringify(JSON.parse(input).id)); },
  call(doc, path) {
    if (path === 'prompt') return Promise.resolve('{"$host":9,"width":1}');
    const p = new Promise((resolve) => {
      setTimeout(() => resolve('{"appearances":[{"image":' + image + ',"owner":{"$host":6}}]}'), 5);
    });
    p.abort = () => {};
    return p;
  },
};
"#;

fn released(stub: &JsValue) -> Vec<f64> {
    let array: js_sys::Array =
        js_sys::Reflect::get(stub, &"released".into()).unwrap().unchecked_into();
    array.iter().map(|n| n.as_f64().unwrap()).collect()
}

#[wasm_bindgen_test]
async fn a_late_answer_after_an_abort_releases_its_hosts() {
    reset();
    let stub = js_sys::Function::new_no_args(STUB).call0(&JsValue::UNDEFINED).unwrap();
    let release: js_sys::Function =
        js_sys::Reflect::get(&stub, &"releaseHost".into()).unwrap().unchecked_into();
    set_host_release(release);

    let engine = JsEngine::new(stub.clone().unchecked_into::<EngineBridge>());
    let opened = engine.open(json!({"kind": "bytes", "id": "a"}), None);
    js_sleep(0.0).await;
    let Some(Ok(doc)) = opened.peek() else { panic!("open did not resolve") };

    let delivered = doc.call("prompt", None, vec![]);
    let late = doc.call("late", None, vec![]);
    late.abort(Some(json!("stop")));
    assert_eq!(late.peek().unwrap().unwrap_err().name, "AbortError");
    js_sleep(0.0).await;
    assert_eq!(delivered.peek().unwrap().unwrap(), json!({"$host": 9, "width": 1}));
    assert!(released(&stub).is_empty());

    js_sleep(20.0).await;
    let mut ids = released(&stub);
    ids.sort_by(f64::total_cmp);
    assert_eq!(ids, vec![5.0, 6.0]);
}

/// A bridge whose `open` has already resolved when it returns, so its
/// `abort` can no longer stop anything, and which records every `call`.
const OPENED_STUB: &str = r#"
const calls = [];
return {
  calls,
  open(input) {
    const p = Promise.resolve(JSON.stringify(JSON.parse(input).id));
    p.abort = () => {};
    return p;
  },
  call(doc, path) {
    calls.push(doc + ':' + path);
    return Promise.resolve('null');
  },
};
"#;

#[wasm_bindgen_test]
async fn an_open_aborted_after_the_engine_answered_closes_the_late_document() {
    reset();
    let stub = js_sys::Function::new_no_args(OPENED_STUB).call0(&JsValue::UNDEFINED).unwrap();
    let engine = JsEngine::new(stub.clone().unchecked_into::<EngineBridge>());
    let opened = engine.open(json!({"kind": "bytes", "id": "late"}), None);
    // The bridge's promise has resolved; this side hears of it a microtask
    // later, so the abort still wins here.
    opened.abort(Some(json!("stop")));
    let Some(Err(error)) = opened.peek() else { panic!("the abort did not reject the open") };
    assert_eq!(error.name, "AbortError");

    js_sleep(0.0).await;
    let calls: js_sys::Array =
        js_sys::Reflect::get(&stub, &"calls".into()).unwrap().unchecked_into();
    let calls: Vec<String> = calls.iter().map(|c| c.as_string().unwrap()).collect();
    assert_eq!(calls, vec!["late:close".to_string()]);
}
