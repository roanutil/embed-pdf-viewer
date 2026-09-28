//! The started clock: once [`start_clock`] runs, the virtual clock follows
//! `Date.now()` at every entry, and never runs backwards.

use std::cell::Cell;
use std::rc::Rc;

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
