//! `set_host_release`: what `epdf_kernel::bytes::release_host` calls, one
//! per wasm instance like `set_host_bytes_source` (`host_bytes.rs`). It
//! hands JavaScript the value's `$host` id, and nothing for a value that
//! has none.

use epdf_kernel::bytes::release_host;
use epdf_web::exports::set_host_release;
use serde_json::json;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// A stand-in for `engine-bridge.ts`'s module-level `releaseHost`: it
/// records every id it is handed.
const STUB_RELEASE: &str = r#"
const released = [];
return { released, releaseHost: (n) => { released.push(n); } };
"#;

fn released(stub: &JsValue) -> Vec<f64> {
    let array: js_sys::Array =
        js_sys::Reflect::get(stub, &"released".into()).unwrap().unchecked_into();
    array.iter().map(|n| n.as_f64().unwrap()).collect()
}

// One test, for the reason `host_bytes.rs` gives: the `Once` guard fires at
// most once for this whole test binary.
#[wasm_bindgen_test]
fn one_releaser_hands_javascript_the_host_id() {
    let stub = js_sys::Function::new_no_args(STUB_RELEASE).call0(&JsValue::NULL).unwrap();
    let release_fn: js_sys::Function =
        js_sys::Reflect::get(&stub, &"releaseHost".into()).unwrap().unchecked_into();

    // Before anything installs a releaser, releasing does nothing.
    release_host(&json!({ "$host": 1 }));

    set_host_release(release_fn);
    release_host(&json!({ "$host": 7, "width": 1 }));
    release_host(&json!({ "width": 1 }));
    assert_eq!(released(&stub), vec![7.0]);

    // A later call installs nothing new.
    let never_called = js_sys::Function::new_no_args(r#"throw new Error("must not run");"#);
    set_host_release(never_called);
    release_host(&json!({ "$host": 8 }));
    assert_eq!(released(&stub), vec![7.0, 8.0]);
}
