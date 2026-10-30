//! The started clock: once [`start_clock`] runs, the virtual clock follows
//! `Date.now()` at every entry, and never runs backwards.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::identity::{generate_uuid_v7, with_uuid_source};
use epdf_kernel::rt;
use epdf_web::runtime::{after_entry, reset, start_clock};
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
async fn a_started_clock_follows_wall_time() {
    reset();
    start_clock();
    let started = rt::now();
    assert!((started - js_sys::Date::now()).abs() < 1000.0);
    js_sleep(50.0).await;
    after_entry();
    assert!(rt::now() - started >= 40.0);
}

#[wasm_bindgen_test]
async fn an_export_called_from_a_timer_does_not_turn_the_clock_back() {
    reset();
    start_clock();
    let inner = Rc::new(Cell::new(0.0));
    let seen = inner.clone();
    rt::set_timeout(1.0, move || {
        // A callback into JavaScript that takes a while, then calls an
        // export, which ends with `after_entry`.
        let until = js_sys::Date::now() + 5.0;
        while js_sys::Date::now() < until {}
        after_entry();
        seen.set(rt::now());
    });
    js_sleep(10.0).await;
    // This entry's catch-up fires the timer inside its `rt::advance`.
    after_entry();
    assert!(inner.get() > 0.0);
    assert!(rt::now() >= inner.get(), "{} went back to {}", inner.get(), rt::now());
}

fn uuid_millis(id: &str) -> f64 {
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "7");
    assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
    u64::from_str_radix(&id[..13].replace('-', ""), 16).unwrap() as f64
}

#[wasm_bindgen_test]
fn started_runtime_generates_live_uuid_v7() {
    reset();
    start_clock();
    let before = js_sys::Date::now();
    let millis = uuid_millis(&generate_uuid_v7(None));
    assert!(millis >= before && millis <= js_sys::Date::now());
    reset();
}

#[wasm_bindgen_test]
fn same_millisecond_operation_ids_use_browser_entropy() {
    reset();
    start_clock();
    let ids = Rc::new(RefCell::new(Vec::new()));
    let generated = ids.clone();
    let generate = Closure::<dyn FnMut()>::new(move || {
        for _ in 0..100 {
            generated.borrow_mut().push(generate_uuid_v7(Some(123.0)));
        }
    });
    let with_crypto = js_sys::Function::new_with_args(
        "generate",
        r#"
const crypto = globalThis.crypto;
const descriptor = Object.getOwnPropertyDescriptor(crypto, 'getRandomValues');
let calls = 0;
Object.defineProperty(crypto, 'getRandomValues', {
    configurable: true,
    value(bytes) {
        bytes.fill(0xab);
        bytes[bytes.length - 1] = ++calls;
        return bytes;
    }
});
try {
    generate();
    return calls;
} finally {
    if (descriptor) Object.defineProperty(crypto, 'getRandomValues', descriptor);
    else delete crypto.getRandomValues;
}
"#,
    );
    let calls = with_crypto.call1(&JsValue::NULL, generate.as_ref()).unwrap();
    assert!(calls.as_f64().unwrap() > 0.0, "UUIDs must read Web Crypto entropy");
    let mut distinct = std::collections::HashSet::new();
    for (index, id) in ids.borrow().iter().enumerate() {
        assert_eq!(uuid_millis(id), 123.0);
        assert_eq!(id, &format!("00000000-007b-7bab-abab-ababababab{:02x}", index + 1));
        assert!(distinct.insert(id.clone()));
    }
    assert_eq!(distinct.len(), 100);
    reset();
}

#[wasm_bindgen_test]
fn nested_uuid_source_restores_browser_source() {
    reset();
    start_clock();
    {
        let _source = with_uuid_source(Rc::new(|bytes| bytes.fill(0)), Rc::new(|| 123.0));
        // Constructing another kernel must not replace a scoped source.
        start_clock();
        assert_eq!(generate_uuid_v7(None), "00000000-007b-7000-8000-000000000000");
        assert_eq!(generate_uuid_v7(Some(256.0)), "00000000-0100-7000-8000-000000000000");
    }
    let before = js_sys::Date::now();
    let millis = uuid_millis(&generate_uuid_v7(None));
    assert!(millis >= before && millis <= js_sys::Date::now());
    reset();
}

#[wasm_bindgen_test]
fn reset_releases_uuid_source_before_restart() {
    reset();
    let _outer = with_uuid_source(Rc::new(|bytes| bytes.fill(0)), Rc::new(|| 123.0));
    start_clock();
    start_clock();
    let before = js_sys::Date::now();
    let millis = uuid_millis(&generate_uuid_v7(None));
    assert!(millis >= before && millis <= js_sys::Date::now());
    reset();
    assert_eq!(generate_uuid_v7(None), "00000000-007b-7000-8000-000000000000");
    start_clock();
    let before = js_sys::Date::now();
    let millis = uuid_millis(&generate_uuid_v7(None));
    assert!(millis >= before && millis <= js_sys::Date::now());
    reset();
}
