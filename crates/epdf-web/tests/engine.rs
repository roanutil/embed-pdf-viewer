//! `JsEngine` and `JsDocumentHandle` over a stub bridge: a plain JavaScript
//! object with the methods `createEngineBridge` returns, answering from a
//! script and logging what it was asked. The real bridge over the real
//! engine is `packages/core-wasm/test/engine-bridge.test.ts`.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_kernel::engine::{DocumentHandle, Engine};
use epdf_kernel::error::ErrorClass;
use epdf_web::engine::{EngineBridge, JsEngine, UNINDEXED};
use epdf_web::runtime::reset;
use serde_json::{Value, json};
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

const STUB: &str = r#"
const log = [];
let listener = null;
return {
  log,
  emit(json) { if (listener) listener(json); },
  open(input, options) {
    log.push(['open', input, options ?? null]);
    return Promise.resolve(JSON.stringify(JSON.parse(input).id));
  },
  call(doc, path, pon, args) {
    log.push(['call', doc, path, pon ?? null, args]);
    if (path === 'pages.list') return Promise.resolve('{"pageCount":2,"pages":[]}');
    if (path === 'refuse') {
      return Promise.reject({ $error: { name: 'PermissionDenied', message: 'no', required: 'doc.render' } });
    }
    if (path === 'throws') throw new Error('sync');
    if (path === 'hang') {
      let reject;
      const p = new Promise((_, r) => { reject = r; });
      p.abort = (reason) => {
        log.push(['abort', reason]);
        reject({ $error: { name: 'AbortError', message: 'late' } });
      };
      return p;
    }
    return Promise.resolve('{"$undefined":true}');
  },
  has(doc, member) { return member === 'render'; },
  capabilities(doc) { return '{"pageEditSessions":"supported"}'; },
  security(doc, member, args) {
    log.push(['security', member, args]);
    if (member === 'allows') return JSON.stringify(JSON.parse(args)[0] === 'doc.render');
    if (member === 'passwordPrompt') return '{"state":"required"}';
    if (member === 'effectiveScope') return '["doc.render"]';
    throw new Error('no such read');
  },
  pageIndex(doc, pon) {
    if (pon === 99) {
      const e = new Error('no page 99');
      e.name = 'EngineError';
      e.code = 'NotFound';
      throw e;
    }
    return pon === 7 ? -1 : 3;
  },
  lastServerId(doc) { return '42'; },
  subscribe(doc, l) {
    listener = l;
    return () => { listener = null; log.push(['off']); };
  },
  destroy() { log.push(['destroy']); return Promise.resolve('{"$undefined":true}'); },
};
"#;

fn stub() -> JsValue {
    js_sys::Function::new_no_args(STUB).call0(&JsValue::UNDEFINED).unwrap()
}

fn log(stub: &JsValue) -> Value {
    let log = js_sys::Reflect::get(stub, &"log".into()).unwrap();
    serde_json::from_str(&String::from(js_sys::JSON::stringify(&log).unwrap())).unwrap()
}

async fn open(stub: &JsValue) -> Rc<dyn DocumentHandle> {
    let engine = JsEngine::new(stub.clone().unchecked_into::<EngineBridge>());
    let opened = engine.open(json!({"kind": "bytes", "id": "a"}), Some(json!({"scope": ["*"]})));
    js_sleep(0.0).await;
    let Some(Ok(doc)) = opened.peek() else { panic!("open did not resolve") };
    doc
}

#[wasm_bindgen_test]
async fn open_and_call_cross_as_json_text() {
    reset();
    let stub = stub();
    let doc = open(&stub).await;
    assert_eq!(doc.id(), "a");

    let list = doc.call("pages.list", None, vec![]);
    let render = doc.call("page.render.image", Some(12), vec![json!({"width": 64})]);
    js_sleep(0.0).await;
    assert_eq!(list.peek().unwrap().unwrap(), json!({"pageCount": 2, "pages": []}));
    assert_eq!(render.peek().unwrap().unwrap(), json!({"$undefined": true}));
    assert_eq!(
        log(&stub),
        json!([
            ["open", r#"{"kind":"bytes","id":"a"}"#, r#"{"scope":["*"]}"#],
            ["call", "a", "pages.list", null, "[]"],
            ["call", "a", "page.render.image", 12, r#"[{"width":64}]"#],
        ])
    );
}

#[wasm_bindgen_test]
async fn a_refusal_keeps_its_members_and_its_class() {
    reset();
    let stub = stub();
    let doc = open(&stub).await;
    let refused = doc.call("refuse", None, vec![]);
    let thrown = doc.call("throws", None, vec![]);
    js_sleep(0.0).await;
    let error = refused.peek().unwrap().unwrap_err();
    assert_eq!(
        (error.name.as_str(), error.class),
        ("PermissionDenied", ErrorClass::PermissionDenied)
    );
    assert_eq!(error.member("required"), Some(&json!("doc.render")));
    let error = thrown.peek().unwrap().unwrap_err();
    assert_eq!((error.name.as_str(), error.message.as_str()), ("Error", "sync"));
}

#[wasm_bindgen_test]
async fn an_abort_reaches_the_bridge_and_rejects_at_once() {
    reset();
    let stub = stub();
    let doc = open(&stub).await;
    let call = doc.call("hang", None, vec![]);
    assert!(call.is_abortable());
    call.abort(Some(json!("stop")));
    let error = call.peek().unwrap().unwrap_err();
    assert_eq!((error.name.as_str(), error.message.as_str()), ("AbortError", "stop"));
    js_sleep(0.0).await;
    // The bridge's own late rejection does not replace it.
    assert_eq!(call.peek().unwrap().unwrap_err().message, "stop");
    assert_eq!(log(&stub).as_array().unwrap().last().unwrap(), &json!(["abort", "stop"]));
}

#[wasm_bindgen_test]
async fn synchronous_reads_go_to_the_bridge_each_time() {
    reset();
    let stub = stub();
    let doc = open(&stub).await;
    assert!(doc.has("render"));
    assert!(!doc.has("redaction"));
    assert_eq!(doc.capabilities(), json!({"pageEditSessions": "supported"}));
    let security = doc.security();
    assert!(security.allows("doc.render"));
    assert!(!security.allows("doc.print"));
    assert_eq!(security.password_prompt(), json!({"state": "required"}));
    assert_eq!(security.effective_scope(), vec!["doc.render".to_string()]);
    // A read that throws answers the refusing default.
    assert!(!security.allows_annotation_create());
    assert_eq!(security.current(), Value::Null);
    assert_eq!(doc.page_index(5).unwrap(), 3);
    assert_eq!(doc.page_index(7).unwrap(), UNINDEXED);
    let missing = doc.page_index(99).unwrap_err();
    assert_eq!((missing.name.as_str(), missing.code()), ("EngineError", Some("NotFound")));
    assert_eq!(doc.last_server_id().as_deref(), Some("42"));

    let unlock = security.unlock(json!({"password": "pw"}));
    js_sleep(0.0).await;
    assert!(unlock.peek().unwrap().is_ok());
    assert_eq!(
        log(&stub).as_array().unwrap().last().unwrap(),
        &json!(["call", "a", "security.unlock", null, r#"[{"password":"pw"}]"#])
    );
}

#[wasm_bindgen_test]
async fn events_arrive_parsed_until_unsubscribed() {
    reset();
    let stub = stub();
    let doc = open(&stub).await;
    let seen = Rc::new(RefCell::new(Vec::new()));
    let s = seen.clone();
    let off = doc.subscribe(Rc::new(move |event: &Value| s.borrow_mut().push(event.clone())));
    let emit =
        js_sys::Reflect::get(&stub, &"emit".into()).unwrap().unchecked_into::<js_sys::Function>();
    emit.call1(&stub, &r#"{"type":"metadata.updated"}"#.into()).unwrap();
    assert_eq!(*seen.borrow(), vec![json!({"type": "metadata.updated"})]);
    off.call();
    emit.call1(&stub, &r#"{"type":"again"}"#.into()).unwrap();
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(log(&stub).as_array().unwrap().last().unwrap(), &json!(["off"]));
}

#[wasm_bindgen_test]
async fn destroy_goes_to_the_bridge() {
    reset();
    let stub = stub();
    let engine = JsEngine::new(stub.clone().unchecked_into::<EngineBridge>());
    let destroyed = engine.destroy();
    js_sleep(0.0).await;
    assert!(destroyed.peek().unwrap().is_ok());
    assert_eq!(log(&stub), json!([["destroy"]]));
}
