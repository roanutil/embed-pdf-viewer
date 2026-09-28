//! `start_clock`, in its own wasm instance: once started, the thread's
//! virtual clock follows `Date.now()` for every later test in the binary.

use epdf_kernel::rt;
use epdf_web::runtime::{after_entry, start_clock};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn start_clock_puts_the_virtual_clock_on_wall_time() {
    rt::reset();
    start_clock();
    assert!((rt::now() - js_sys::Date::now()).abs() < 1000.0);
    after_entry();
    assert!((rt::now() - js_sys::Date::now()).abs() < 1000.0);
}
