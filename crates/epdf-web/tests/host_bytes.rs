//! `set_host_bytes_source`: one resolver per wasm instance, not one per
//! `WasmKernel`. The previous design (`install_host_bytes_resolver`,
//! called from `WasmKernel::build`) let a second live kernel silently
//! overwrite the first kernel's resolver in `epdf_kernel::bytes`'s
//! thread_local — with two live kernels (two viewers on a page, or a
//! remount), a `$hostBytes` lookup for the first kernel's bridge would
//! resolve against the second kernel's bridge instead, which does not
//! recognize the id (or, since both bridges numbered hosts from 1, could
//! resolve to the *wrong* bytes). `WasmKernel::new` no longer installs
//! anything; the wasm side has exactly one resolver, installed once, and
//! it works no matter which kernel (or how many) are live, because
//! `packages/core-wasm/src/engine-bridge.ts` now hands out ids from one
//! table shared by every bridge it builds.

use epdf_kernel::bytes::Bytes;
use epdf_web::exports::{WasmKernel, set_host_bytes_source};
use epdf_web::runtime::reset;
use serde_json::json;
use wasm_bindgen::prelude::*;
use wasm_bindgen_test::wasm_bindgen_test;

/// A stand-in for `engine-bridge.ts`'s shared host table and module-level
/// `hostBytes`: one registry, one counter, so two "bridges" that both call
/// `hold` on it never collide the way two independent per-bridge tables
/// would.
const STUB_SOURCE: &str = r#"
const registry = new Map();
let next = 1;
function hold(bytes) {
  const n = next++;
  registry.set(n, bytes);
  return n;
}
function hostBytes(n) {
  const v = registry.get(n);
  if (!(v instanceof Uint8Array)) throw new Error(`no host bytes ${n}`);
  return v;
}
return { hold, hostBytes };
"#;

/// A minimal engine bridge: enough to build a `WasmKernel` with no plugins
/// and no open document. It has no `hostBytes` method of its own; nothing
/// here calls one per bridge any more.
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
};
"#;

fn eval(src: &str) -> JsValue {
    js_sys::Function::new_no_args(src).call0(&JsValue::NULL).unwrap()
}

fn no_op_call_js() -> js_sys::Function {
    js_sys::Function::new_no_args("return () => undefined;")
        .call0(&JsValue::NULL)
        .unwrap()
        .unchecked_into()
}

fn no_op_release_js() -> js_sys::Function {
    no_op_call_js()
}

fn hold(source: &JsValue, bytes: &[u8]) -> u32 {
    let hold_fn =
        js_sys::Reflect::get(source, &"hold".into()).unwrap().unchecked_into::<js_sys::Function>();
    let array = js_sys::Uint8Array::from(bytes);
    hold_fn.call1(&JsValue::NULL, &array.into()).unwrap().as_f64().unwrap() as u32
}

// One test, not several: `set_host_bytes_source`'s `Once` guard fires at
// most once for the life of this wasm instance (this whole test binary),
// so a second, separate `#[wasm_bindgen_test]` that also called it could
// not tell its own install from a leftover one another test made first.
#[wasm_bindgen_test]
fn one_resolver_serves_every_live_kernel_with_unique_ids() {
    reset();
    let source = eval(STUB_SOURCE);
    let host_bytes_fn = js_sys::Reflect::get(&source, &"hostBytes".into())
        .unwrap()
        .unchecked_into::<js_sys::Function>();

    // Before anything installs a resolver, an unresolved shape answers
    // `None`, the same as any shape `from_json` does not recognize.
    assert_eq!(Bytes::from_json(&json!({ "$hostBytes": 1 })), None);

    set_host_bytes_source(host_bytes_fn.clone());

    // Two live kernels over two different bridges. Neither installs
    // anything itself: the resolver above is the only one, and it is
    // shared, not tied to either kernel's construction.
    let first_kernel =
        WasmKernel::new("[]", eval(STUB_ENGINE), no_op_call_js(), no_op_release_js()).unwrap();
    let second_kernel =
        WasmKernel::new("[]", eval(STUB_ENGINE), no_op_call_js(), no_op_release_js()).unwrap();

    let first_id = hold(&source, &[1, 2, 3]);
    let second_id = hold(&source, &[4, 5, 6, 7]);
    assert_ne!(first_id, second_id, "ids are unique across what would be two bridges");

    assert_eq!(Bytes::from_json(&json!({ "$hostBytes": first_id })), Some(Bytes(vec![1, 2, 3])));
    assert_eq!(
        Bytes::from_json(&json!({ "$hostBytes": second_id })),
        Some(Bytes(vec![4, 5, 6, 7]))
    );

    // Dropping the first kernel changes nothing: the resolver was never
    // the first kernel's to own.
    drop(first_kernel);
    assert_eq!(
        Bytes::from_json(&json!({ "$hostBytes": second_id })),
        Some(Bytes(vec![4, 5, 6, 7]))
    );

    // A later call installs nothing new: the first `set_host_bytes_source`
    // is the one every kernel shares for the life of this wasm instance.
    let never_called = eval(r#"return () => { throw new Error("must not run"); };"#)
        .unchecked_into::<js_sys::Function>();
    set_host_bytes_source(never_called);
    assert_eq!(
        Bytes::from_json(&json!({ "$hostBytes": second_id })),
        Some(Bytes(vec![4, 5, 6, 7]))
    );

    drop(second_kernel);
}
