//! The kernel's `Engine` over `EngineWorker`. Lives on the kernel thread.
//! An engine call becomes a pending `Promise`; the worker's reply comes back
//! as a kernel job that settles it from `PENDING`.
//!
//! Document security is not enforced: `NativeSecurity` allows everything and
//! reports an anonymous identity. Enforcing document permissions is
//! follow-on plan B4. The engine emits no events yet either, so `subscribe`
//! is inert.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_engine::host::HostResponse;
use epdf_ffi::KernelPost;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::rt::Resolver;
use epdf_kernel::{Error, Promise, Unsubscribe};
use serde_json::{Value, json};

use crate::engine_worker::EngineWorker;
use crate::host_table::HostTable;
use crate::translate::{self, Decode, WireCall};

type Pending = (u64, HashMap<u64, (Decode, Resolver<Value>)>);

thread_local! {
    static PENDING: RefCell<Pending> = RefCell::new((0, HashMap::new()));
}

pub struct NativeEngine {
    inner: Rc<Inner>,
}

struct Inner {
    worker: EngineWorker,
    post: KernelPost,
    documents: Cell<u64>,
}

impl NativeEngine {
    pub fn new(worker: EngineWorker, post: KernelPost) -> NativeEngine {
        NativeEngine { inner: Rc::new(Inner { worker, post, documents: Cell::new(0) }) }
    }
}

impl Inner {
    /// Reserve a pending id, build the call with it as the `jobId`, and send
    /// it. Aborting drops the reply and rejects with an `AbortError`.
    fn send(&self, build: impl FnOnce(u64) -> Result<WireCall, Error>) -> Promise<Value> {
        let (promise, resolver) = Promise::pending();
        let id = PENDING.with(|p| {
            let mut p = p.borrow_mut();
            p.0 += 1;
            p.0
        });
        let call = match build(id) {
            Ok(call) => call,
            Err(error) => return Promise::rejected(error),
        };
        PENDING.with(|p| p.borrow_mut().1.insert(id, (call.decode, resolver)));
        let post = self.post.clone();
        self.worker.submit(
            call.request.to_string(),
            call.payloads,
            Box::new(move |response| {
                post(Box::new(move || settle(id, response)));
            }),
        );
        promise.with_abort(move |reason| {
            if let Some((_, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) {
                resolver.reject(Error::abort(reason));
            }
        })
    }
}

fn settle(id: u64, response: HostResponse) {
    let Some((decode, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) else {
        return; // aborted; the late reply is dropped
    };
    match translate::decode(&decode, response) {
        Ok(value) => resolver.resolve(value),
        Err(error) => resolver.reject(error),
    }
}

impl Engine for NativeEngine {
    /// `input` names the document's bytes as `{"$host": id}` in the
    /// `HostTable`; the open takes them out of it.
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        let Some(bytes) = input.get("$host").and_then(Value::as_u64).and_then(HostTable::take)
        else {
            return Promise::rejected(
                Error::new("[epdf-viewer-host] open needs {\"$host\": id} naming the bytes")
                    .with("code", "invalid-input"),
            );
        };
        let password = options.as_ref().and_then(|o| o.get("password")).cloned();
        let n = self.inner.documents.get() + 1;
        self.inner.documents.set(n);
        let doc = format!("doc-{n}");
        let opened = self.inner.send(|job| {
            Ok(WireCall {
                request: json!({
                    "kind": "open.fatMem",
                    "jobId": job,
                    "docId": doc,
                    "password": password.unwrap_or(Value::Null),
                }),
                payloads: vec![bytes],
                decode: Decode::Field("security"),
            })
        });
        let inner = self.inner.clone();
        Promise::spawn(async move {
            let security = opened.await?;
            let pages =
                inner.send(|job| translate::translate(&doc, "pages.list", None, &[], job)).await?;
            Ok(Rc::new(NativeDocument { id: doc, inner, pages, security })
                as Rc<dyn DocumentHandle>)
        })
    }

    fn destroy(&self) -> EngineCall<()> {
        self.inner.worker.shutdown();
        Promise::resolved(())
    }
}

/// One opened document. The `pages.list` snapshot is the one the open
/// loaded; nothing mapped yet changes the page list.
struct NativeDocument {
    id: String,
    inner: Rc<Inner>,
    pages: Value,
    /// The `DocumentSecurityState` the open reported.
    security: Value,
}

impl DocumentHandle for NativeDocument {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> Value {
        json!({})
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(NativeSecurity { current: self.security.clone() })
    }

    fn has(&self, member: &str) -> bool {
        translate::maps(member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        self.inner.send(|job| translate::translate(&self.id, path, pon, &args, job))
    }

    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        self.pages
            .get("pages")
            .and_then(Value::as_array)
            .and_then(|pages| {
                pages.iter().position(|p| {
                    p.get("pageObjectNumber").and_then(Value::as_u64) == Some(u64::from(pon))
                })
            })
            .map(|i| i as u32)
            .ok_or_else(|| Error::engine("NotFound", format!("NotFound: no page {pon}")))
    }

    fn subscribe(&self, _listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        Unsubscribe::noop()
    }
}

/// Allows everything, for an anonymous caller.
struct NativeSecurity {
    current: Value,
}

impl DocumentSecurity for NativeSecurity {
    fn allows(&self, _capability: &str) -> bool {
        true
    }

    fn allows_annotation_create(&self) -> bool {
        true
    }

    fn allows_annotation_mutation(&self, _action: &str, _target: &Value) -> bool {
        true
    }

    fn allows_annotation_group_assignment(&self, _group_id: &str) -> bool {
        true
    }

    fn effective_scope(&self) -> Vec<String> {
        Vec::new()
    }

    fn identity(&self) -> Value {
        Value::Null
    }

    fn current(&self) -> Value {
        self.current.clone()
    }

    fn password_prompt(&self) -> Value {
        json!({ "state": "none" })
    }

    fn unlock(&self, _input: Value) -> EngineCall<Value> {
        Promise::rejected(
            Error::new("[epdf-viewer-host] the native engine has no \"security.unlock\" yet")
                .with("code", "unsupported"),
        )
    }
}
