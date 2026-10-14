//! The kernel's `Engine` over `EngineWorker`. Lives on the kernel thread.
//! An engine call becomes a pending `Promise`; the worker's reply comes back
//! as a kernel job that settles it from `PENDING`.
//!
//! A mutation's reply carries the document event the host published. It goes
//! to the document's `subscribe` listeners before the call resolves, which is
//! what a plugin waiting on its own confirmed event (metadata's `update`)
//! needs.
//!
//! Document security is enforced from `ViewerConfig.scope`: `NativeSecurity`
//! answers from the same scope and permission bits the engine checks with.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_engine::auth::scope::{
    CollabAction, CollabTarget, DocCapability, IdentityClaims, decode_pdf_bits,
};
use epdf_engine::host::HostResponse;
use epdf_engine::session::authority::SessionAuthority;
use epdf_ffi::KernelPost;
use epdf_kernel::engine::{DocumentHandle, DocumentSecurity, Engine, EngineCall};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Resolver};
use epdf_kernel::{Error, Promise, Unsubscribe};
use serde_json::{Value, json};

use crate::engine_worker::EngineWorker;
use crate::host_table::HostTable;
use crate::translate::{self, Decode, WireCall};

type Pending = (u64, HashMap<u64, (String, Decode, Resolver<Value>)>);
type Listener = Rc<dyn Fn(&Value)>;
type Listeners = (u64, HashMap<String, Vec<(u64, Listener)>>);

thread_local! {
    static PENDING: RefCell<Pending> = RefCell::new((0, HashMap::new()));
    /// `events.subscribe` listeners, by document id.
    static LISTENERS: RefCell<Listeners> = RefCell::new((0, HashMap::new()));
    /// `destroy` calls waiting for the worker to drain, by id.
    static DRAINED: RefCell<(u64, HashMap<u64, Resolver<()>>)> = RefCell::new((0, HashMap::new()));
    /// Each open document's current `PageListSnapshot`: the one its open
    /// loaded, then the `layout` of every page mutation's event.
    static LAYOUTS: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
}

pub struct NativeEngine {
    inner: Rc<Inner>,
}

struct Inner {
    worker: EngineWorker,
    post: KernelPost,
    documents: Cell<u64>,
    /// The scope every `open.fatMem` carries. `["*"]` when none was set.
    scope: Vec<String>,
}

impl NativeEngine {
    pub fn new(worker: EngineWorker, post: KernelPost, scope: Option<Vec<String>>) -> NativeEngine {
        let scope = scope.unwrap_or_else(|| vec!["*".to_string()]);
        NativeEngine { inner: Rc::new(Inner { worker, post, documents: Cell::new(0), scope }) }
    }
}

impl Inner {
    /// Reserve a pending id, build the call for document `doc` with it as
    /// the `jobId`, and send it. Aborting drops the reply and rejects with an
    /// `AbortError`.
    fn send(
        &self,
        doc: &str,
        build: impl FnOnce(u64) -> Result<WireCall, Error>,
    ) -> Promise<Value> {
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
        PENDING.with(|p| p.borrow_mut().1.insert(id, (doc.to_owned(), call.decode, resolver)));
        let post = self.post.clone();
        self.worker.submit(
            call.request.to_string(),
            call.payloads,
            Box::new(move |response| {
                post(Box::new(move || settle(id, response)));
            }),
        );
        promise.with_abort(move |reason| {
            if let Some((_, _, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) {
                resolver.reject(Error::abort(reason));
            }
        })
    }

    /// Close `doc` on the host, answering nobody: the cleanup for an open
    /// that will not hand out a document.
    fn close(&self, doc: &str) {
        let _ = self.send(doc, |job| translate::translate(doc, "close", None, &[], job));
    }
}

fn settle(id: u64, response: HostResponse) {
    let Some((doc, decode, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) else {
        return; // aborted; the late reply is dropped
    };
    match translate::decode(&decode, response) {
        Ok(decoded) => {
            if let Some(event) = decoded.event {
                publish(&doc, &event);
            }
            resolver.resolve(decoded.value);
        }
        Err(error) => resolver.reject(error),
    }
}

/// The layout a page mutation's event carries, as `kernel.rs` reads it.
fn page_layout(event: &Value) -> Option<&Value> {
    match event.get("type").and_then(Value::as_str)? {
        "pages.moved" | "pages.rotated" | "pages.deleted" | "pages.inserted" => event.get("layout"),
        _ => None,
    }
}

/// Hand `event` to `doc`'s listeners. A listener added or removed by one of
/// them takes effect from the next event.
fn publish(doc: &str, event: &Value) {
    if let Some(layout) = page_layout(event) {
        LAYOUTS.with(|l| l.borrow_mut().insert(doc.to_owned(), layout.clone()));
    }
    let listeners: Vec<Listener> = LISTENERS.with(|l| {
        l.borrow().1.get(doc).map_or_else(Vec::new, |v| v.iter().map(|(_, f)| f.clone()).collect())
    });
    for listener in listeners {
        listener(event);
    }
}

impl Engine for NativeEngine {
    /// `input` names the document's bytes as `{"$host": id}` in the
    /// `HostTable`; the open takes them out of it.
    ///
    /// Once `open.fatMem` has landed the host holds a session, so an open
    /// that then fails (a locked document's `pages.list`) or is aborted
    /// closes it rather than leaving the bytes parked on the host.
    fn open(&self, input: Value, options: Option<Value>) -> EngineCall<Rc<dyn DocumentHandle>> {
        // The host's own opens name their bytes in the `HostTable`. A plugin
        // opening a document of its own (page edit's subset insert, the
        // stamp plugin's asset libraries) sends them as `Bytes` JSON under
        // `bytes`.
        let bytes = match input.get("$host").and_then(Value::as_u64) {
            Some(id) => HostTable::take(id),
            None => input.get("bytes").and_then(epdf_kernel::bytes::Bytes::from_json).map(|b| b.0),
        };
        let Some(bytes) = bytes else {
            return Promise::rejected(
                Error::new(
                    "[epdf-viewer-host] open needs {\"$host\": id} or {\"bytes\": ...} naming the bytes",
                )
                .with("code", "invalid-input"),
            );
        };
        let password = options.as_ref().and_then(|o| o.get("password")).cloned();
        let n = self.inner.documents.get() + 1;
        self.inner.documents.set(n);
        let doc = format!("doc-{n}");
        let opened = self.inner.send(&doc, |job| {
            Ok(WireCall {
                request: json!({
                    "kind": "open.fatMem",
                    "jobId": job,
                    "docId": doc,
                    "password": password.unwrap_or(Value::Null),
                    "scope": self.inner.scope,
                }),
                payloads: vec![bytes],
                decode: Decode::Field("security"),
            })
        });
        let (promise, resolver) = Promise::<Rc<dyn DocumentHandle>>::pending();
        let aborted = Rc::new(Cell::new(false));
        let inner = self.inner.clone();
        let (settled, stopped) = (resolver.clone(), aborted.clone());
        rt::spawn(async move {
            let security = match opened.await {
                Ok(security) => security,
                Err(error) => return settled.reject(error),
            };
            if stopped.get() {
                return inner.close(&doc);
            }
            // A document that needs a password stays open, parked on the
            // host, and comes up as a locked tab: `password_prompt` says so,
            // and `NativeSecurity::unlock` loads it.
            let state = Rc::new(NativeDocumentState::new(doc.clone(), inner.clone(), security));
            if state.locked.get() {
                let pages = json!({ "pages": [] });
                return settled.resolve(Rc::new(NativeDocument { id: doc, inner, pages, state }));
            }
            let listed = inner
                .send(&doc, |job| translate::translate(&doc, "pages.list", None, &[], job))
                .await;
            match listed {
                Err(error) => {
                    inner.close(&doc);
                    settled.reject(error);
                }
                Ok(_) if stopped.get() => inner.close(&doc),
                Ok(pages) => {
                    LAYOUTS.with(|l| l.borrow_mut().insert(doc.clone(), pages.clone()));
                    settled.resolve(Rc::new(NativeDocument { id: doc, inner, pages, state }))
                }
            }
        });
        promise.with_abort(move |reason| {
            aborted.set(true);
            resolver.reject(Error::abort(reason));
        })
    }

    /// Resolves once the worker has answered every call queued before it.
    /// Later calls are refused; `Session::drop` joins the thread.
    fn destroy(&self) -> EngineCall<()> {
        let (promise, resolver) = Promise::pending();
        let id = DRAINED.with(|d| {
            let mut d = d.borrow_mut();
            d.0 += 1;
            let id = d.0;
            d.1.insert(id, resolver);
            id
        });
        let post = self.inner.post.clone();
        self.inner.worker.shutdown_after(Box::new(move || {
            post(Box::new(move || {
                if let Some(resolver) = DRAINED.with(|d| d.borrow_mut().1.remove(&id)) {
                    resolver.resolve(());
                }
            }));
        }));
        promise
    }
}

/// One opened document. `pages` is the `pages.list` snapshot the open
/// loaded; `page_index` reads the live one in `LAYOUTS`, which every page
/// mutation's event replaces.
struct NativeDocument {
    id: String,
    inner: Rc<Inner>,
    pages: Value,
    /// What `security()` answers from, shared with every `NativeSecurity`
    /// it hands out so an unlock is seen by all of them.
    state: Rc<NativeDocumentState>,
}

/// One document's authority and security state.
struct NativeDocumentState {
    id: String,
    inner: Rc<Inner>,
    /// Built from the same scope the open sent, with the permission bits the
    /// engine's probe reported, so it answers as the engine enforces.
    authority: RefCell<SessionAuthority>,
    /// The `DocumentSecurityState` the open, or the unlock, reported.
    security: RefCell<Value>,
    /// Parked on the host until a password unlocks it.
    locked: Cell<bool>,
}

impl NativeDocumentState {
    fn new(id: String, inner: Rc<Inner>, security: Value) -> NativeDocumentState {
        // A scope the engine refused fails the open before this runs; the
        // default grants nothing, should one get here anyway.
        let authority =
            SessionAuthority::new(&inner.scope, IdentityClaims::default(), Default::default())
                .unwrap_or_default();
        let state = NativeDocumentState {
            id,
            inner,
            authority: RefCell::new(authority),
            security: RefCell::new(Value::Null),
            locked: Cell::new(false),
        };
        state.take_probe(security);
        state
    }

    /// Adopt a security probe: the permission bits for `pdf.permissions`,
    /// and whether the document is still waiting for a password (it needs
    /// one and has not been opened as anybody).
    fn take_probe(&self, security: Value) {
        let bits = security.get("pdfPermissionsBits").and_then(Value::as_i64);
        self.authority.borrow_mut().set_pdf_bits(decode_pdf_bits(bits));
        let needs_password = security.get("encryptionRequiresPassword") == Some(&Value::Bool(true));
        let opened = security.get("pdfOpenedAs").is_some_and(|v| !v.is_null());
        self.locked.set(needs_password && !opened);
        *self.security.borrow_mut() = security;
    }
}

impl DocumentHandle for NativeDocument {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn capabilities(&self) -> Value {
        json!({})
    }

    fn security(&self) -> Rc<dyn DocumentSecurity> {
        Rc::new(NativeSecurity { state: self.state.clone() })
    }

    fn has(&self, member: &str) -> bool {
        translate::maps(member)
    }

    fn call(&self, path: &str, pon: Option<u32>, args: Vec<Value>) -> EngineCall<Value> {
        if path == "close" {
            LAYOUTS.with(|l| l.borrow_mut().remove(&self.id));
        }
        self.inner.send(&self.id, |job| translate::translate(&self.id, path, pon, &args, job))
    }

    fn page_index(&self, pon: u32) -> Result<u32, Error> {
        let page = PageRef::new(pon);
        let layout = LAYOUTS.with(|l| l.borrow().get(&self.id).cloned());
        layout
            .as_ref()
            .unwrap_or(&self.pages)
            .get("pages")
            .and_then(Value::as_array)
            .and_then(|pages| pages.iter().find(|p| page.names(p)))
            .and_then(|p| p.get("index").and_then(Value::as_u64))
            .and_then(|i| u32::try_from(i).ok())
            .ok_or_else(|| Error::engine("NotFound", format!("NotFound: no page {pon}")))
    }

    fn subscribe(&self, listener: Rc<dyn Fn(&Value)>) -> Unsubscribe {
        let doc = self.id.clone();
        let id = LISTENERS.with(|l| {
            let mut l = l.borrow_mut();
            l.0 += 1;
            let id = l.0;
            l.1.entry(doc.clone()).or_default().push((id, listener));
            id
        });
        Unsubscribe::new(move || {
            LISTENERS.with(|l| {
                let mut l = l.borrow_mut();
                if let Some(list) = l.1.get_mut(&doc) {
                    list.retain(|(each, _)| *each != id);
                    if list.is_empty() {
                        l.1.remove(&doc);
                    }
                }
            });
        })
    }
}

/// What the kernel asks before an action, answered from the same scope and
/// permission bits the engine enforces with (`LocalDocumentSecurityService`).
/// A locked document allows nothing until it unlocks.
struct NativeSecurity {
    state: Rc<NativeDocumentState>,
}

impl NativeSecurity {
    fn open(&self) -> bool {
        !self.state.locked.get()
    }
}

/// A record's owner as the annotation plugin passes it: `{userId, groupId}`.
fn collab_target(target: &Value) -> CollabTarget {
    let text = |key: &str| target.get(key).and_then(Value::as_str).map(str::to_owned);
    CollabTarget { user_id: text("userId"), group_id: text("groupId") }
}

impl DocumentSecurity for NativeSecurity {
    fn allows(&self, capability: &str) -> bool {
        self.open()
            && DocCapability::parse(capability)
                .is_some_and(|c| self.state.authority.borrow().allows(c))
    }

    /// `ScopeGuard.targetForSelfCreate`: the caller's own identity.
    fn allows_annotation_create(&self) -> bool {
        let authority = self.state.authority.borrow();
        let identity = authority.identity();
        let target =
            CollabTarget { user_id: identity.user_id.clone(), group_id: identity.group_id.clone() };
        self.open() && authority.allows_collab(CollabAction::Create, &target)
    }

    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool {
        let Some(action) = CollabAction::parse(action) else { return false };
        self.open() && self.state.authority.borrow().allows_collab(action, &collab_target(target))
    }

    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool {
        self.open() && self.state.authority.borrow().allows_set_group(group_id)
    }

    /// Sorted, as `ScopeGuard.effectiveScope` answers.
    fn effective_scope(&self) -> Vec<String> {
        let mut scope: Vec<String> = self
            .state
            .authority
            .borrow()
            .expanded()
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        scope.sort();
        scope
    }

    /// No identity is configured natively, so `null`, as the TypeScript
    /// answers for an empty one.
    fn identity(&self) -> Value {
        Value::Null
    }

    fn current(&self) -> Value {
        self.state.security.borrow().clone()
    }

    fn password_prompt(&self) -> Value {
        if self.state.locked.get() {
            json!({ "state": "required" })
        } else {
            json!({ "state": "none" })
        }
    }

    /// `document.checkPasswordPermissions`, which on a parked session loads
    /// it with the password, then the page list the open skipped. A wrong
    /// password rejects (`DocPasswordIncorrect`) and the tab stays locked.
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        let state = self.state.clone();
        let password = input.get("password").cloned().unwrap_or(Value::Null);
        let mode = input.get("mode").cloned().unwrap_or_else(|| json!("any"));
        let id = state.id.clone();
        let checked = state.inner.send(&id, |job| {
            Ok(WireCall {
                request: json!({ "kind": "document.checkPasswordPermissions", "jobId": job,
                                 "docId": id, "password": password, "mode": mode }),
                payloads: Vec::new(),
                decode: Decode::Result,
            })
        });
        let (promise, resolver) = Promise::pending();
        rt::spawn(async move {
            let security = match checked.await {
                Ok(security) => security,
                Err(error) => return resolver.reject(error),
            };
            let pages = state
                .inner
                .send(&state.id, |job| {
                    translate::translate(&state.id, "pages.list", None, &[], job)
                })
                .await;
            match pages {
                Ok(pages) => {
                    LAYOUTS.with(|l| l.borrow_mut().insert(state.id.clone(), pages));
                    state.take_probe(security.clone());
                    resolver.resolve(json!({ "security": security }));
                }
                Err(error) => resolver.reject(error),
            }
        });
        promise
    }
}
