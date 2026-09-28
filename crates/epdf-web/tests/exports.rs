//! `WasmKernel` as JavaScript drives it: a kernel built from plugin specs
//! over a stub engine bridge, its capabilities read as tables of refs, and
//! refs called by id. The TypeScript shim over these exports is Task 6's.

use epdf_web::exports::WasmKernel;
use epdf_web::runtime::reset;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;
use wasm_bindgen_test::wasm_bindgen_test;

/// An engine bridge that opens any input as a document with no pages and
/// answers every other call with `undefined`.
const STUB_ENGINE: &str = r#"
return {
  open(input) { return Promise.resolve(JSON.stringify(JSON.parse(input).id)); },
  call(doc, path) {
    if (path === 'pages.list') return Promise.resolve('{"pageCount":0,"pages":[]}');
    return Promise.resolve('{"$undefined":true}');
  },
  has() { return false; },
  capabilities() { return '{}'; },
  security(doc, member) { return member === 'passwordPrompt' ? '{"state":"none"}' : 'null'; },
  pageIndex() { return -1; },
  lastServerId() { return null; },
  subscribe() { return () => {}; },
  destroy() { return Promise.resolve('{"$undefined":true}'); },
};
"#;

fn stub_engine() -> JsValue {
    js_sys::Function::new_no_args(STUB_ENGINE).call0(&JsValue::NULL).unwrap()
}

/// A `call_js` that logs `[id, argsJson]` into `log` and answers with
/// `answer(id, args)`.
fn call_js(log: &js_sys::Array, answer: &str) -> js_sys::Function {
    let make = js_sys::Function::new_with_args(
        "log",
        &format!("return (id, args) => {{ log.push([id, args]); return ({answer})(id, args); }};"),
    );
    make.call1(&JsValue::NULL, log).unwrap().into()
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn js_json(value: &JsValue) -> Value {
    parse(&js_sys::JSON::stringify(value).unwrap().as_string().unwrap())
}

fn ref_of(capability: &Value, member: &str) -> u32 {
    capability[member]["$ref"].as_u64().unwrap_or_else(|| panic!("{member}: {capability}")) as u32
}

/// Call a ref; the outcome as JSON.
fn invoke(kernel: &WasmKernel, id: u32, args: &str) -> Value {
    parse(&kernel.invoke_ref(id, args.into()).unwrap())
}

/// The promise id an outcome carries.
fn promise_of(outcome: &Value) -> u32 {
    outcome["promise"].as_u64().unwrap_or_else(|| panic!("{outcome}")) as u32
}

/// Start the kernel and open one document through `documents().open`.
async fn start_with_document(kernel: &WasmKernel) {
    JsFuture::from(kernel.start()).await.unwrap();
    let documents = parse(&kernel.documents());
    let outcome = invoke(kernel, ref_of(&documents, "open"), r#"[{"id":"doc-1"}]"#);
    let opened = JsFuture::from(kernel.promise(promise_of(&outcome))).await.unwrap();
    assert_eq!(opened.as_string().as_deref(), Some("doc-1"));
}

#[wasm_bindgen_test]
async fn a_getter_ref_returns() {
    reset();
    let plugins = json!([{ "factory": "metadata" }, { "factory": "shell" }]).to_string();
    let log = js_sys::Array::new();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&log, "() => undefined")).unwrap();
    start_with_document(&kernel).await;

    let shell = parse(&kernel.capability("shell".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&shell, "isOpen"), r#"["panel"]"#);
    assert_eq!(outcome, json!({ "returned": false }));

    let documents = parse(&kernel.documents());
    let outcome = invoke(&kernel, ref_of(&documents, "getCount"), "[]");
    assert_eq!(outcome, json!({ "returned": 1 }));
    assert_eq!(log.length(), 0);
}

#[wasm_bindgen_test]
async fn a_document_capability_with_no_document_throws() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&js_sys::Array::new(), "() => 0"))
            .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let thrown = kernel.capability("shell".into(), None).unwrap_err();
    assert!(js_json(&thrown).get("$error").is_some(), "{:?}", thrown);
}

#[wasm_bindgen_test]
async fn a_config_callback_calls_into_javascript_and_its_promise_resolves() {
    reset();
    let plugins = json!([{
        "factory": "commands",
        "config": { "commands": [
            { "id": "hello", "labelKey": "hello", "run": { "$fn": "js:run" } },
        ] },
    }])
    .to_string();
    let log = js_sys::Array::new();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&log, "() => Promise.resolve(7)"))
            .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["hello"]"#);
    let resolved = JsFuture::from(kernel.promise(promise_of(&outcome))).await.unwrap();

    assert_eq!(js_json(&resolved), json!({ "status": "executed" }));
    assert_eq!(log.length(), 1);
    let call: js_sys::Array = log.get(0).into();
    assert_eq!(call.get(0).as_string().as_deref(), Some("run"));
    let args = parse(&call.get(1).as_string().unwrap());
    assert_eq!(args[0]["documentId"], Value::Null);
    assert_eq!(args[0]["tryGet"], json!({ "$kernel": "tryGet", "doc": null }));
}

#[wasm_bindgen_test]
async fn a_subscriber_hears_a_store_change_until_it_unsubscribes() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&js_sys::Array::new(), "() => 0"))
            .unwrap();
    start_with_document(&kernel).await;
    let heard = js_sys::Array::new();
    let listener: js_sys::Function =
        js_sys::Function::new_with_args("heard", "return () => heard.push(1);")
            .call1(&JsValue::NULL, &heard)
            .unwrap()
            .into();
    let id = kernel.subscribe(listener);

    let shell = parse(&kernel.capability("shell".into(), None).unwrap());
    invoke(&kernel, ref_of(&shell, "open"), r#"["panel"]"#);
    let after_open = heard.length();
    assert!(after_open > 0);
    assert!(parse(&kernel.state_json())["plugins"].is_object());

    kernel.unsubscribe(id);
    invoke(&kernel, ref_of(&shell, "close"), r#"["panel"]"#);
    assert_eq!(heard.length(), after_open);
}

#[wasm_bindgen_test]
async fn the_same_capability_describes_to_the_same_text() {
    reset();
    let plugins = json!([{ "factory": "commands" }]).to_string();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&js_sys::Array::new(), "() => 0"))
            .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let first = kernel.capability("commands".into(), None).unwrap();
    assert_eq!(kernel.capability("commands".into(), None).unwrap(), first);
    assert_eq!(kernel.try_capability("commands".into(), None).unwrap(), Some(first));
    assert_eq!(kernel.documents(), kernel.documents());
}

#[wasm_bindgen_test]
async fn status_scope_and_try_capability_follow_the_kernel() {
    reset();
    let plugins = json!([{ "factory": "commands" }, { "factory": "shell" }]).to_string();
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&js_sys::Array::new(), "() => 0"))
            .unwrap();
    assert_eq!(kernel.status(), "created");
    assert_eq!(kernel.scope_of("shell".into()), "document");
    assert_eq!(kernel.scope_of("commands".into()), "workspace");
    JsFuture::from(kernel.start()).await.unwrap();
    assert_eq!(kernel.status(), "started");
    assert_eq!(kernel.try_capability("shell".into(), None).unwrap(), None);

    start_with_document(&kernel).await;
    assert!(kernel.try_capability("shell".into(), Some("doc-1".into())).unwrap().is_some());
}

#[wasm_bindgen_test]
async fn a_throw_in_javascript_is_the_callback_throwing() {
    reset();
    let plugins = json!([{
        "factory": "commands",
        "config": { "commands": [
            { "id": "boom", "labelKey": "boom", "run": { "$fn": "js:run" } },
        ] },
    }])
    .to_string();
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&log, "() => { throw new TypeError('boom'); }"),
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["boom"]"#);
    let rejected = JsFuture::from(kernel.promise(promise_of(&outcome))).await.unwrap_err();

    assert_eq!(log.length(), 1);
    let error = js_json(&rejected);
    assert!(error.to_string().contains("boom"), "{error}");
}

/// A command's `run`, in JavaScript, resolving its ctx's `tryGet` marker by
/// calling back into the kernel: the path every viewer command takes.
#[wasm_bindgen_test]
async fn a_run_callback_reenters_the_kernel_through_its_ctx_marker() {
    reset();
    let plugins = json!([
        { "factory": "shell" },
        { "factory": "commands", "config": { "commands": [
            { "id": "peek", "labelKey": "peek", "run": { "$fn": "js:run" } },
        ] } },
    ])
    .to_string();
    // `call_js` logs into it; the callback also reads the kernel from it.
    let holder = js_sys::Array::new();
    let answer = r#"(id, args) => {
      const [ctx] = JSON.parse(args);
      const k = log.k;
      const shell = JSON.parse(k.try_capability('shell', ctx.tryGet.doc));
      log.ctx = ctx;
      log.isOpen = k.invoke_ref(shell.isOpen.$ref, '["panel"]');
      return undefined;
    }"#;
    let kernel = WasmKernel::new(&plugins, stub_engine(), call_js(&holder, answer));
    let kernel = kernel.unwrap();
    start_with_document(&kernel).await;
    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let execute = ref_of(&commands, "execute");

    // From here on JavaScript owns the kernel, so its calls go through the
    // wasm-bindgen glue as the shim's will.
    let k: JsValue = kernel.into();
    js_sys::Reflect::set(&holder, &"k".into(), &k).unwrap();
    let run = js_sys::Function::new_with_args(
        "k, execute",
        "return k.promise(JSON.parse(k.invoke_ref(execute, \
         '[\"peek\", {\"documentId\": \"doc-1\"}]')).promise);",
    );
    let pending: js_sys::Promise = run.call2(&JsValue::NULL, &k, &execute.into()).unwrap().into();
    let resolved = JsFuture::from(pending).await.unwrap();

    assert_eq!(js_json(&resolved), json!({ "status": "executed" }));
    let ctx = js_json(&js_sys::Reflect::get(&holder, &"ctx".into()).unwrap());
    assert_eq!(ctx["documentId"], json!("doc-1"));
    assert_eq!(ctx["tryGet"], json!({ "$kernel": "tryGet", "doc": "doc-1" }));
    assert_eq!(ctx["get"], json!({ "$kernel": "get", "doc": "doc-1" }));
    assert_eq!(ctx["core"], json!({ "$kernel": "core", "doc": "doc-1" }));
    let is_open = js_sys::Reflect::get(&holder, &"isOpen".into()).unwrap();
    assert_eq!(parse(&is_open.as_string().unwrap()), json!({ "returned": false }));
}
