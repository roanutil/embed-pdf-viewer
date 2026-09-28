//! The runtime driven by real JavaScript timers. Runs under Node with
//! `wasm-bindgen-test-runner`; Node has `setTimeout`. The clock is never
//! started here (tests/clock.rs does that), so the virtual clock only moves
//! as timers come due.

use std::cell::Cell;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_web::runtime::{after_entry, from_js, reset, to_js};
use serde_json::json;
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

#[wasm_bindgen_test]
async fn a_virtual_sleep_finishes_on_real_time() {
    reset();
    let done = Rc::new(Cell::new(false));
    let d = done.clone();
    rt::spawn(async move {
        rt::sleep(20.0).await.unwrap();
        d.set(true);
    });
    after_entry();
    assert!(!done.get());
    js_sleep(50.0).await;
    assert!(done.get());
    assert_eq!(rt::next_deadline(), None);
}

#[wasm_bindgen_test]
async fn promises_cross_both_ways() {
    reset();
    let resolved = JsFuture::from(to_js(Promise::resolved(json!({"a": [1, "b"]})))).await.unwrap();
    assert_eq!(js_sys::JSON::stringify(&resolved).unwrap(), r#"{"a":[1,"b"]}"#);

    let rejected = to_js(Promise::rejected(Error::engine("NotFound", "gone")));
    let error = JsFuture::from(rejected).await.unwrap_err();
    assert_eq!(
        js_sys::JSON::stringify(&error).unwrap(),
        r#"{"$error":{"name":"EngineError","message":"gone","code":"NotFound"}}"#
    );

    let back = from_js(js_sys::Promise::resolve(&JsValue::from(7)));
    js_sleep(0.0).await;
    assert_eq!(back.peek().unwrap().unwrap(), json!(7));

    let thrown = js_sys::Error::new("nope");
    let back = from_js(js_sys::Promise::reject(&thrown));
    js_sleep(0.0).await;
    let error = back.peek().unwrap().unwrap_err();
    assert_eq!((error.name.as_str(), error.message.as_str()), ("Error", "nope"));
}

#[wasm_bindgen_test]
async fn a_task_awaiting_a_js_promise_resumes_when_it_resolves() {
    reset();
    let later = js_sys::Promise::new(&mut |resolve, _| {
        set_timeout(&resolve, 10.0);
    });
    let done = Rc::new(Cell::new(false));
    let d = done.clone();
    rt::spawn(async move {
        from_js(later).await.unwrap();
        d.set(true);
    });
    assert!(!done.get());
    // Nothing but `from_js` drains the ready queue once the promise resolves.
    js_sleep(30.0).await;
    assert!(done.get());
}

#[wasm_bindgen_test]
async fn a_value_json_cannot_carry_rejects_with_a_type_error() {
    reset();
    let big = js_sys::BigInt::from(1u64);
    let back = from_js(js_sys::Promise::resolve(&JsValue::from(big)));
    js_sleep(0.0).await;
    let error = back.peek().unwrap().unwrap_err();
    assert_eq!(error.name, "TypeError");
}

#[wasm_bindgen_test]
async fn a_rejection_object_keeps_its_name_and_message() {
    reset();
    let thrown = js_sys::JSON::parse(r#"{"name":"RangeError","message":"far","code":3}"#).unwrap();
    let back = from_js(js_sys::Promise::reject(&thrown));
    js_sleep(0.0).await;
    let error = back.peek().unwrap().unwrap_err();
    assert_eq!((error.name.as_str(), error.message.as_str()), ("RangeError", "far"));
    assert_eq!(error.member("code"), Some(&json!(3)));
}
