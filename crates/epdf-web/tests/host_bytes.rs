//! `install_host_bytes_resolver`, exercised as `WasmKernel::new` really
//! runs it: `Bytes::from_json` on a `{"$hostBytes": n}` value resolves
//! through the bridge's `hostBytes(n)`, the round trip a stamp preview or
//! ghost needs to get real pixels back (`crates/plugins/stamp/src/services
//! /asset_engine.rs::image_to_preview`). The render raster store never
//! calls `Bytes::from_json` on this shape at all (`crates/plugins/render
//! /src/raster_store.rs::cost` reads `$byteLength` directly), so it never
//! pays for this round trip.

use epdf_kernel::bytes::{Bytes, set_host_bytes_resolver};
use epdf_web::exports::WasmKernel;
use epdf_web::runtime::reset;
use serde_json::json;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// An engine bridge whose only interesting method is `hostBytes`: enough to
/// build a `WasmKernel` with no plugins and no open document.
const STUB_ENGINE: &str = r#"
return {
  open(input) { return Promise.resolve(JSON.stringify(JSON.parse(input).id)); },
  call() { return Promise.resolve('{"$undefined":true}'); },
  has() { return false; },
  capabilities() { return '{}'; },
  security(doc, member) { return member === 'passwordPrompt' ? '{"state":"none"}' : 'null'; },
  pageIndex() { return -1; },
  lastServerId() { return null; },
  subscribe() { return () => {}; },
  destroy() { return Promise.resolve('{"$undefined":true}'); },
  hostBytes(n) {
    if (n === 7) return new Uint8Array([1, 2, 3]);
    throw new Error(`no host bytes ${n}`);
  },
};
"#;

fn stub_engine() -> JsValue {
    js_sys::Function::new_no_args(STUB_ENGINE).call0(&JsValue::NULL).unwrap()
}

fn no_op_call_js() -> js_sys::Function {
    js_sys::Function::new_no_args("return () => undefined;")
        .call0(&JsValue::NULL)
        .unwrap()
        .unchecked_into()
}

#[wasm_bindgen_test]
fn host_bytes_is_none_with_no_kernel_built_yet() {
    reset();
    // A kernel from an earlier test in this binary may have installed a
    // resolver of its own; a fresh `$hostBytes` shape resolves through
    // whatever is currently installed, so start from a known "none".
    set_host_bytes_resolver(None);
    assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 7 })), None);
}

#[wasm_bindgen_test]
fn constructing_a_kernel_installs_a_resolver_over_its_bridge() {
    reset();
    let _kernel = WasmKernel::new("[]", stub_engine(), no_op_call_js()).unwrap();
    assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 7 })), Some(Bytes(vec![1, 2, 3])));
    // An id the bridge does not recognize resolves to `None`, the same as
    // any other value `from_json` does not recognize — never a panic.
    assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 9 })), None);
    // `$byteLength` alone (the render raster store's shape: `cost()` never
    // needs the pixels) still answers `None`: nothing here resolves it.
    assert_eq!(Bytes::from_json(&json!({ "$byteLength": 3 })), None);
}
