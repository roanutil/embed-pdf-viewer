//! `WasmKernel` as JavaScript drives it: a kernel built from plugin specs
//! over a stub engine bridge, its capabilities read as tables of refs, and
//! refs called by id. The TypeScript shim over these exports is Task 6's.

use epdf_kernel::rt::{self, Promise};
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

/// A `release_js` that ignores every release.
fn no_release() -> js_sys::Function {
    js_sys::Function::new_no_args("")
}

/// A `release_js` that logs each id it is handed into `log`.
fn release_into(log: &js_sys::Array) -> js_sys::Function {
    let make = js_sys::Function::new_with_args("log", "return (id) => { log.push(id); };");
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
    let opened = JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap();
    assert_eq!(js_json(&opened)["document"]["id"], "doc-1");
}

#[wasm_bindgen_test]
async fn a_getter_ref_returns() {
    reset();
    let plugins = json!([{ "factory": "metadata" }, { "factory": "shell" }]).to_string();
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&log, "() => undefined"),
        no_release(),
        None,
    )
    .unwrap();
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
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
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
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&log, "() => Promise.resolve(7)"),
        no_release(),
        None,
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["hello"]"#);
    let resolved = JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap();

    assert_eq!(js_json(&resolved), json!({ "status": "executed" }));
    assert_eq!(log.length(), 1);
    let call: js_sys::Array = log.get(0).into();
    assert_eq!(call.get(0).as_string().as_deref(), Some("run"));
    let args = parse(&call.get(1).as_string().unwrap());
    assert_eq!(args[0]["documentId"], Value::Null);
    assert_eq!(args[0]["tryGet"], json!({ "$kernel": "tryGet", "doc": null }));
}

/// A live harness's promise table gives up its entry the moment JavaScript
/// takes it: `debug_counts().promises` drops by one, and a second take of
/// the same id throws rather than answering again.
#[wasm_bindgen_test]
async fn a_promise_is_taken_once_and_a_second_take_throws() {
    reset();
    let plugins = json!([{
        "factory": "commands",
        "config": { "commands": [
            { "id": "hello", "labelKey": "hello", "run": { "$fn": "js:run" } },
        ] },
    }])
    .to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => Promise.resolve(7)"),
        no_release(),
        None,
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["hello"]"#);
    let id = promise_of(&outcome);

    let before = parse(&kernel.debug_counts())["promises"].as_u64().unwrap();
    let promise = kernel.promise(id).unwrap();
    let after = parse(&kernel.debug_counts())["promises"].as_u64().unwrap();
    assert_eq!(after, before - 1, "the take drops the harness's live count by one");

    let error = js_json(&kernel.promise(id).unwrap_err());
    assert!(error.to_string().contains("was already taken"), "{error}");

    let resolved = JsFuture::from(promise).await.unwrap();
    assert_eq!(js_json(&resolved), json!({ "status": "executed" }));
}

/// `abort_promise` after the take still finds the promise, through
/// `WasmKernel`'s own table rather than the harness's (which no longer has
/// the entry): it does not throw or panic, before or after the promise
/// settles. Nothing here is wired abortable, so this only proves the
/// lookup is safe, not that an abort reaches anywhere; see the task report
/// for why a genuine abort is not cheaply reachable in this harness.
#[wasm_bindgen_test]
async fn abort_promise_after_the_take_is_still_a_safe_call() {
    reset();
    let plugins = json!([{
        "factory": "commands",
        "config": { "commands": [
            { "id": "hello", "labelKey": "hello", "run": { "$fn": "js:run" } },
        ] },
    }])
    .to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => Promise.resolve(7)"),
        no_release(),
        None,
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["hello"]"#);
    let id = promise_of(&outcome);
    let promise = kernel.promise(id).unwrap();

    kernel.abort_promise(id, None).unwrap();
    JsFuture::from(promise).await.unwrap();
    // Settled: the `on_settle` cleanup already dropped this kernel's own
    // entry, so a call after that is still a harmless no-op.
    kernel.abort_promise(id, None).unwrap();
}

#[wasm_bindgen_test]
async fn a_subscriber_hears_a_store_change_until_it_unsubscribes() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
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
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let first = kernel.capability("commands".into(), None).unwrap();
    assert_eq!(kernel.capability("commands".into(), None).unwrap(), first);
    assert_eq!(kernel.try_capability("commands".into(), None).unwrap(), Some(first));
    assert_eq!(kernel.documents(), kernel.documents());
}

/// A capability's description pins its refs, so `release_ref` leaves them
/// callable, while a ref a call handed out is freed: calling it afterwards
/// throws, it does not answer.
#[wasm_bindgen_test]
async fn release_ref_frees_a_returned_ref_and_ignores_a_pinned_one() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let refs = |kernel: &WasmKernel| parse(&kernel.debug_counts())["refs"].as_u64().unwrap();

    let shell = parse(&kernel.capability("shell".into(), None).unwrap());
    assert_eq!(shell["isOpen"]["pinned"], json!(true), "{shell}");
    let outcome = invoke(&kernel, ref_of(&shell, "onSurfaceOpened"), r#"[{"$fn":"js:listener"}]"#);
    let off = outcome["returned"]["$ref"].as_u64().unwrap() as u32;
    assert!(outcome["returned"].get("pinned").is_none(), "{outcome}");
    let before = refs(&kernel);

    kernel.release_ref(ref_of(&shell, "isOpen"));
    assert_eq!(refs(&kernel), before, "a pinned ref is not released");
    assert_eq!(invoke(&kernel, ref_of(&shell, "isOpen"), r#"["panel"]"#)["returned"], false);

    kernel.release_ref(off);
    assert_eq!(refs(&kernel), before - 1);
    let called = invoke(&kernel, off, "[]");
    assert!(called["threw"].to_string().contains("after its release"), "{called}");
}

#[wasm_bindgen_test]
async fn status_scope_and_try_capability_follow_the_kernel() {
    reset();
    let plugins = json!([{ "factory": "commands" }, { "factory": "shell" }]).to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
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
        no_release(),
        None,
    )
    .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();

    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let outcome = invoke(&kernel, ref_of(&commands, "execute"), r#"["boom"]"#);
    let rejected = JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap_err();

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
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&holder, answer), no_release(), None);
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
    let is_open = js_sys::Reflect::get(&holder, &"isOpen".into()).unwrap();
    assert_eq!(parse(&is_open.as_string().unwrap()), json!({ "returned": false }));
}

/// An export called from JavaScript while another export's body is still on
/// the stack is not an entry of its own: it leaves the due timer and the
/// ready task alone, and the outer export runs both once it returns.
#[wasm_bindgen_test]
async fn an_export_reentered_from_a_callback_leaves_timers_and_the_queue_to_the_outer_one() {
    reset();
    let plugins = json!([{ "factory": "commands", "config": { "commands": [
        { "id": "hello", "labelKey": "hello", "run": { "$fn": "js:run" } },
    ] } }])
    .to_string();
    let order = js_sys::Array::new();
    let answer = r#"(id, args) => {
      log.k.status();
      log.push('nested returned');
      return undefined;
    }"#;
    let kernel =
        WasmKernel::new(&plugins, stub_engine(), call_js(&order, answer), no_release(), None)
            .unwrap();
    JsFuture::from(kernel.start()).await.unwrap();
    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let execute = ref_of(&commands, "execute");

    // A timer due now, and a task woken but not yet polled.
    let timer_order = order.clone();
    rt::set_timeout(0.0, move || {
        timer_order.push(&"timer".into());
    });
    let (woken, resolver) = Promise::<Value>::pending();
    let task_order = order.clone();
    rt::spawn(async move {
        let _ = woken.await;
        task_order.push(&"task".into());
    });
    resolver.settle(Ok(Value::Null));

    let k: JsValue = kernel.into();
    js_sys::Reflect::set(&order, &"k".into(), &k).unwrap();
    let run = js_sys::Function::new_with_args(
        "k, execute, order",
        "k.invoke_ref(execute, '[\"hello\"]'); order.push('outer returned');",
    );
    run.call3(&JsValue::NULL, &k, &execute.into(), &order).unwrap();

    let seen: Vec<String> = order
        .iter()
        .map(|entry| match entry.as_string() {
            Some(text) => text,
            None => js_sys::Array::from(&entry).get(0).as_string().unwrap(),
        })
        .collect();
    assert_eq!(seen, ["run", "nested returned", "timer", "task", "outer returned"]);
}

/// The ids `release_js` was handed, in order.
fn released(log: &js_sys::Array) -> Vec<String> {
    log.iter().map(|id| id.as_string().unwrap()).collect()
}

#[wasm_bindgen_test]
async fn an_unregistered_handler_releases_its_functions_once_each() {
    reset();
    let plugins = json!([{ "factory": "interaction" }]).to_string();
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => undefined"),
        release_into(&log),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let interaction = parse(&kernel.capability("interaction".into(), None).unwrap());
    let register = ref_of(&interaction, "registerHandler");

    // `js:1` backs both handlers, as one function passed twice would.
    let first = invoke(
        &kernel,
        register,
        r#"[{"id":"a","priority":1,"onDown":{"$fn":"js:1"},"onMove":{"$fn":"js:2"}}]"#,
    );
    let second = invoke(&kernel, register, r#"[{"id":"b","priority":1,"onDown":{"$fn":"js:1"}}]"#);
    let held = |kernel: &WasmKernel| parse(&kernel.debug_counts())["heldFns"].as_u64().unwrap();
    assert_eq!(held(&kernel), 2);

    let unregister = |outcome: &Value| outcome["returned"]["$ref"].as_u64().unwrap() as u32;
    invoke(&kernel, unregister(&first), "[]");
    assert_eq!(released(&log), vec!["2"], "js:1 still backs the second handler");
    invoke(&kernel, unregister(&second), "[]");
    assert_eq!(released(&log), vec!["2", "1"]);
    assert_eq!(held(&kernel), 0);
}

/// A command whose `run` drops the last callback on `js:1` (unregistering
/// its handler) and makes a new one from it (registering another handler
/// with it), both from inside the one outermost call: the release, due at
/// that call's end, finds `js:1` held again and leaves it.
#[wasm_bindgen_test]
async fn a_callback_dropped_and_remade_in_one_call_keeps_its_function() {
    reset();
    let plugins = json!([
        { "factory": "interaction" },
        { "factory": "commands", "config": { "commands": [
            { "id": "swap", "labelKey": "swap", "run": { "$fn": "js:run" } },
        ] } },
    ])
    .to_string();
    let holder = js_sys::Array::new();
    let answer = r#"(id) => {
      if (id !== 'run') return undefined;
      log.k.invoke_ref(log.off, '[]');
      log.k.invoke_ref(log.register, '[{"id":"b","priority":1,"onDown":{"$fn":"js:1"}}]');
      return undefined;
    }"#;
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&holder, answer),
        release_into(&log),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let interaction = parse(&kernel.capability("interaction".into(), None).unwrap());
    let register = ref_of(&interaction, "registerHandler");
    let first = invoke(&kernel, register, r#"[{"id":"a","priority":1,"onDown":{"$fn":"js:1"}}]"#);
    let off = first["returned"]["$ref"].as_u64().unwrap() as u32;
    let commands = parse(&kernel.capability("commands".into(), None).unwrap());
    let execute = ref_of(&commands, "execute");

    let k: JsValue = kernel.into();
    js_sys::Reflect::set(&holder, &"k".into(), &k).unwrap();
    js_sys::Reflect::set(&holder, &"off".into(), &off.into()).unwrap();
    js_sys::Reflect::set(&holder, &"register".into(), &register.into()).unwrap();
    let run = js_sys::Function::new_with_args(
        "k, execute",
        "return k.promise(JSON.parse(k.invoke_ref(execute, '[\"swap\"]')).promise);",
    );
    let pending: js_sys::Promise = run.call2(&JsValue::NULL, &k, &execute.into()).unwrap().into();
    let resolved = JsFuture::from(pending).await.unwrap();

    assert_eq!(js_json(&resolved), json!({ "status": "executed" }));
    assert!(released(&log).is_empty(), "released {:?}", released(&log));
    let debug_counts = js_sys::Reflect::get(&k, &"debug_counts".into()).unwrap();
    let counts: js_sys::Function = debug_counts.unchecked_into();
    let counts = counts.call0(&k).unwrap().as_string().unwrap();
    assert_eq!(parse(&counts)["heldFns"], json!(2), "js:1 and the command's js:run");
}

fn count(kernel: &WasmKernel, table: &str) -> u64 {
    parse(&kernel.debug_counts())[table].as_u64().unwrap()
}

/// Closing a document drops its capabilities from the memo, and their refs
/// are no longer pinned: a release frees them, as it frees any other ref.
#[wasm_bindgen_test]
async fn closing_a_document_drops_its_capabilities_from_the_memo() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => 0"),
        no_release(),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let documents = parse(&kernel.documents());
    let before = count(&kernel, "described");
    // Once with the document's id and once through the active document: the
    // same capability, so one entry.
    let shell = parse(&kernel.capability("shell".into(), Some("doc-1".into())).unwrap());
    kernel.capability("shell".into(), None).unwrap();
    assert_eq!(count(&kernel, "described"), before + 1);

    let outcome = invoke(&kernel, ref_of(&documents, "close"), r#"["doc-1"]"#);
    JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap();

    assert_eq!(count(&kernel, "described"), before);
    let refs = count(&kernel, "refs");
    kernel.release_ref(ref_of(&shell, "isOpen"));
    assert_eq!(count(&kernel, "refs"), refs - 1, "the closed document's refs are unpinned");
    kernel.release_ref(ref_of(&documents, "getCount"));
    assert_eq!(count(&kernel, "refs"), refs - 1, "the documents capability stays pinned");
}

/// Two kernels share one instance's runtime (its armed timeout, its ready
/// queue, its exit hooks). Destroying and dropping one, as `free()` does,
/// leaves the other opening documents and firing timers.
#[wasm_bindgen_test]
async fn a_dropped_kernel_leaves_another_in_the_instance_working() {
    reset();
    let plugins = json!([{ "factory": "shell" }]).to_string();
    let make = || {
        WasmKernel::new(
            &plugins,
            stub_engine(),
            call_js(&js_sys::Array::new(), "() => 0"),
            no_release(),
            None,
        )
        .unwrap()
    };
    let (first, second) = (make(), make());
    start_with_document(&first).await;
    start_with_document(&second).await;
    let fired = std::rc::Rc::new(std::cell::Cell::new(false));
    let flag = fired.clone();
    rt::set_timeout(20.0, move || flag.set(true));
    // An entry, so the timeout is armed before the kernel goes.
    first.status();

    JsFuture::from(first.destroy()).await.unwrap();
    drop(first);

    let documents = parse(&second.documents());
    let outcome = invoke(&second, ref_of(&documents, "open"), r#"[{"id":"doc-2"}]"#);
    let opened = JsFuture::from(second.promise(promise_of(&outcome)).unwrap()).await.unwrap();
    assert_eq!(js_json(&opened)["document"]["id"], "doc-2");
    let shell = parse(&second.capability("shell".into(), Some("doc-2".into())).unwrap());
    assert_eq!(invoke(&second, ref_of(&shell, "isOpen"), r#"["panel"]"#)["returned"], false);
    let wait = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        let set_timeout: js_sys::Function =
            js_sys::Reflect::get(&global, &"setTimeout".into()).unwrap().unchecked_into();
        set_timeout.call2(&JsValue::NULL, &resolve, &JsValue::from(60)).unwrap();
    });
    JsFuture::from(wait).await.unwrap();
    assert!(fired.get(), "the instance's armed timeout still fires");
}

/// Every `$ref` id in a description.
fn ref_ids(value: &Value, out: &mut Vec<u32>) {
    match value {
        Value::Object(object) => match object.get("$ref").and_then(Value::as_u64) {
            Some(id) => out.push(id as u32),
            None => object.values().for_each(|member| ref_ids(member, out)),
        },
        Value::Array(items) => items.iter().for_each(|item| ref_ids(item, out)),
        _ => {}
    }
}

/// A closed document's plugins go once JavaScript lets go of their refs:
/// a handler registered on its interaction capability hands its function
/// back. Nothing but the refs may keep them, the kernel's own session
/// included.
#[wasm_bindgen_test]
async fn a_closed_documents_plugins_go_once_their_refs_are_released() {
    reset();
    let plugins = json!([{ "factory": "interaction" }]).to_string();
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&js_sys::Array::new(), "() => undefined"),
        release_into(&log),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let interaction =
        parse(&kernel.capability("interaction".into(), Some("doc-1".into())).unwrap());
    let registered = invoke(
        &kernel,
        ref_of(&interaction, "registerHandler"),
        r#"[{"id":"a","priority":1,"onDown":{"$fn":"js:1"}}]"#,
    );

    let documents = parse(&kernel.documents());
    let outcome = invoke(&kernel, ref_of(&documents, "close"), r#"["doc-1"]"#);
    JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap();
    assert!(released(&log).is_empty(), "the refs still hold the handler");

    let mut refs = Vec::new();
    ref_ids(&interaction, &mut refs);
    ref_ids(&registered, &mut refs);
    for id in refs {
        kernel.release_ref(id);
    }
    assert_eq!(released(&log), vec!["1"]);
}

/// `getBehaviorFor` describes a registered behavior with fresh refs each
/// time, from the callbacks made when it was registered: releasing one
/// answer's refs gives back none of the behavior's functions, and the next
/// answer still calls them.
#[wasm_bindgen_test]
async fn a_behavior_described_again_keeps_its_functions() {
    reset();
    let plugins =
        json!([{ "factory": "interaction" }, { "factory": "annotation-live" }]).to_string();
    let calls = js_sys::Array::new();
    let log = js_sys::Array::new();
    let kernel = WasmKernel::new(
        &plugins,
        stub_engine(),
        call_js(&calls, "() => true"),
        release_into(&log),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let annotation = parse(&kernel.capability("annotation".into(), None).unwrap());
    let behavior = r#"[{"id":"b","matches":{"$fn":"js:m"},"engaged":{"$fn":"js:e"}}]"#;
    invoke(&kernel, ref_of(&annotation, "registerBehavior"), behavior);
    let get = ref_of(&annotation, "getBehaviorFor");
    let query = r#"[{"subtype":"square","ref":null}]"#;

    let first = invoke(&kernel, get, query)["returned"].clone();
    let mut refs = Vec::new();
    ref_ids(&first, &mut refs);
    assert_eq!(refs.len(), 2, "{first}");
    refs.into_iter().for_each(|id| kernel.release_ref(id));
    assert!(released(&log).is_empty(), "released {:?}", released(&log));

    let second = invoke(&kernel, get, query)["returned"].clone();
    for (member, callback) in [("matches", "m"), ("engaged", "e")] {
        assert_eq!(invoke(&kernel, ref_of(&second, member), query), json!({ "returned": true }));
        let last = js_sys::Array::from(&calls.get(calls.length() - 1));
        assert_eq!(last.get(0).as_string().as_deref(), Some(callback));
        assert_eq!(parse(&last.get(1).as_string().unwrap()), parse(query));
    }
    let mut refs = Vec::new();
    ref_ids(&second, &mut refs);
    refs.into_iter().for_each(|id| kernel.release_ref(id));
    assert!(released(&log).is_empty(), "registered callbacks outlive returned refs");
}

#[wasm_bindgen_test]
async fn a_staged_annotation_gets_a_live_operation_id_and_a_refusal_keeps_the_runtime_working() {
    reset();
    let make = js_sys::Function::new_no_args(&format!(
        r#"
const bridge = (() => {{ {STUB_ENGINE} }})();
const call = bridge.call;
bridge.applied = [];
bridge.call = function(doc, path, pon, args) {{
    if (path === 'pages.list') return Promise.resolve(JSON.stringify({{
        pageCount: 1,
        pages: [{{ ref: {{kind:'objectNumber',objectNumber:3}}, size: {{width:612,height:792}}, rotation:0, userUnit:1 }}]
    }}));
    if (path === 'annotations.list') return Promise.resolve('[]');
    if (path === 'apply') {{
        this.applied.push(JSON.parse(args));
        return Promise.reject({{$error:{{name:'EngineError',code:'InvalidArg',message:'write refused'}}}});
    }}
    return call(doc, path, pon, args);
}};
bridge.callWith = function(doc, facts, path, pon, args) {{ return this.call(doc, path, pon, args); }};
bridge.security = (doc, member) => member.startsWith('allows') ? 'true' : member === 'passwordPrompt' ? '{{"state":"none"}}' : 'null';
bridge.pageIndex = () => 0;
bridge.heldObjectNumbers = () => 100;
bridge.takeObjectNumber = () => 100;
bridge.onLostObjectNumbers = () => () => {{}};
return bridge;
"#
    ));
    let bridge = make.call0(&JsValue::NULL).unwrap();
    let kernel = WasmKernel::new(
        r#"[{"factory":"interaction"},{"factory":"annotation-live"}]"#,
        bridge.clone(),
        call_js(&js_sys::Array::new(), "() => undefined"),
        no_release(),
        None,
    )
    .unwrap();
    start_with_document(&kernel).await;
    let annotation = parse(&kernel.capability("annotation".into(), Some("doc-1".into())).unwrap());
    assert_eq!(invoke(&kernel, ref_of(&annotation, "canCreate"), "[]")["returned"], true);
    let before = js_sys::Date::now();
    let outcome = invoke(
        &kernel,
        ref_of(&annotation, "create"),
        r#"[{"kind":"objectNumber","objectNumber":3},{"subtype":"square","rect":{"x":72,"y":49,"width":204,"height":28}}]"#,
    );
    let error = JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap_err();
    assert!(js_json(&error)["$error"]["message"].as_str().unwrap().contains("write refused"));
    let applied = js_json(&js_sys::Reflect::get(&bridge, &"applied".into()).unwrap());
    assert_eq!(applied.as_array().unwrap().len(), 1);
    let op_id = applied[0][1]["opId"].as_str().unwrap();
    assert_eq!(op_id.len(), 36);
    assert_eq!(&op_id[14..15], "7");
    assert!(matches!(&op_id[19..20], "8" | "9" | "a" | "b"));
    let millis = u64::from_str_radix(&op_id[..13].replace('-', ""), 16).unwrap() as f64;
    assert!(millis >= before && millis <= js_sys::Date::now());
    // Opening another document requires the Rust task queue to drain again.
    let documents = parse(&kernel.documents());
    let outcome = invoke(&kernel, ref_of(&documents, "open"), r#"[{"id":"doc-2"}]"#);
    let opened = JsFuture::from(kernel.promise(promise_of(&outcome)).unwrap()).await.unwrap();
    assert_eq!(js_json(&opened)["document"]["id"], "doc-2");
    JsFuture::from(kernel.destroy()).await.unwrap();
}
