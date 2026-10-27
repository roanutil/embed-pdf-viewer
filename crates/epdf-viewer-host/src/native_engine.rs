//! The kernel's `Engine` over `EngineWorker`: upstream's local engine
//! (`packages/engine/main/src/LocalEngine.ts`, `LocalDocumentHandle.ts`)
//! on this host. Lives on the kernel thread. An engine call becomes a
//! pending `Promise`; the worker's reply comes back as a kernel job that
//! settles it from `PENDING`.
//!
//! A write's reply carries the document events the host published. They go
//! to the document's `subscribe` listeners before the call resolves, as the
//! local engine publishes a write's facts once the worker confirms it.
//!
//! Document security is enforced from `ViewerConfig.scope`: `NativeSecurity`
//! answers from the same scope and permission bits the engine checks with,
//! and each write carries that authority (`guard.changeAuthority()`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_engine::auth::scope::{
    CollabAction, CollabTarget, DocCapability, FieldWriteAction, IdentityClaims, decode_pdf_bits,
};
use epdf_engine::features::signature::protection::{describe_protection, protected_capabilities};
use epdf_engine::host::HostResponse;
use epdf_engine::session::authority::SessionAuthority;
use epdf_ffi::KernelPost;
use epdf_kernel::engine::{
    CallFacts, DocumentHandle, DocumentSecurity, Engine, EngineCall, ObjectNumbersLost,
    ObjectNumbersLostReason,
};
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Resolver};
use epdf_kernel::{Error, Promise, Unsubscribe};
use serde_json::{Value, json};

use crate::engine_worker::EngineWorker;
use crate::host_table::HostTable;
use crate::translate::{self, Client, Decode, WireCall};

type Pending = (u64, HashMap<u64, (String, Decode, Resolver<Value>)>);
type Listener = Rc<dyn Fn(&Value)>;
type Listeners = (u64, HashMap<String, Vec<(u64, Listener)>>);
type LostListener = Rc<dyn Fn(&ObjectNumbersLost)>;

thread_local! {
    static PENDING: RefCell<Pending> = RefCell::new((0, HashMap::new()));
    /// `events.subscribe` listeners, by document id.
    static LISTENERS: RefCell<Listeners> = RefCell::new((0, HashMap::new()));
    /// `destroy` calls waiting for the worker to drain, by id.
    static DRAINED: RefCell<(u64, HashMap<u64, Resolver<()>>)> = RefCell::new((0, HashMap::new()));
    /// Each open document's current `PageListSnapshot`: the one its open
    /// loaded, then the `layout` of every page write's event.
    static LAYOUTS: RefCell<HashMap<String, Value>> = RefCell::new(HashMap::new());
    /// Each open document's state, so a completed signature's event can
    /// hand it the new protection and its pool can drop its numbers.
    static STATES: RefCell<HashMap<String, std::rc::Weak<NativeDocumentState>>> =
        RefCell::new(HashMap::new());
}

/// `OBJECT_NUMBER_POOL_SIZE`: how many numbers the pool keeps; it gets them
/// as the document opens, and tops up to it (`LocalObjectNumberPool.ts`).
const OBJECT_NUMBER_POOL_SIZE: u32 = 64;
/// `REFILL_BELOW`: below this many, the pool asks for more in the
/// background.
const REFILL_BELOW: usize = 32;

pub struct NativeEngine {
    inner: Rc<Inner>,
}

struct Inner {
    worker: EngineWorker,
    post: KernelPost,
    documents: Cell<u64>,
    /// The asset engine: renders answer encoded bytes, not raw pixels.
    encoded_renders: bool,
    /// What its document ids start with. `PENDING`, `LISTENERS` and
    /// `LAYOUTS` are per kernel thread and keyed by id, and the asset engine
    /// shares the thread with the viewer's engine, so their ids must differ.
    prefix: &'static str,
    /// The scope every `open.fatMem` carries. `["*"]` when none was set.
    scope: Vec<String>,
}

impl NativeEngine {
    pub fn new(worker: EngineWorker, post: KernelPost, scope: Option<Vec<String>>) -> NativeEngine {
        NativeEngine::with(worker, post, false, scope)
    }

    /// The stamp plugin's asset engine: its own engine thread (PDFium's
    /// globals are per thread), with a PNG encoder, opening library bytes
    /// and answering previews as encoded bytes. The caller joins the worker
    /// when the session ends.
    pub fn assets(post: KernelPost) -> (NativeEngine, EngineWorker) {
        let worker = EngineWorker::spawn_with(|| {
            epdf_engine::host::Host::new().with_encoder(Box::new(crate::png_encoder::PngEncoder))
        });
        (NativeEngine::with(worker.clone(), post, true, None), worker)
    }

    fn with(
        worker: EngineWorker,
        post: KernelPost,
        encoded_renders: bool,
        scope: Option<Vec<String>>,
    ) -> NativeEngine {
        let prefix = if encoded_renders { "asset" } else { "doc" };
        let documents = Cell::new(0);
        let scope = scope.unwrap_or_else(|| vec!["*".to_string()]);
        NativeEngine {
            inner: Rc::new(Inner { worker, post, documents, encoded_renders, prefix, scope }),
        }
    }
}

/// The next job id: the `PENDING` key, and the request's `jobId`.
fn next_job() -> u64 {
    PENDING.with(|p| {
        let mut p = p.borrow_mut();
        p.0 += 1;
        p.0
    })
}

impl Inner {
    /// Reserve a job id, build the call for document `doc` with it, and send
    /// it. Aborting drops the reply and rejects with an `AbortError`.
    fn send(
        &self,
        doc: &str,
        build: impl FnOnce(u64) -> Result<WireCall, Error>,
    ) -> Promise<Value> {
        let (promise, resolver) = Promise::pending();
        let id = next_job();
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
        let _ = self.send(doc, |job| Ok(translate::close_request(doc, job)));
    }
}

fn settle(id: u64, response: HostResponse) {
    let Some((doc, decode, resolver)) = PENDING.with(|p| p.borrow_mut().1.remove(&id)) else {
        return; // aborted; the late reply is dropped
    };
    match translate::decode(&decode, response) {
        Ok(decoded) => {
            for event in &decoded.events {
                publish(&doc, event);
            }
            resolver.resolve(decoded.value);
        }
        Err(error) => resolver.reject(error),
    }
}

/// The layout a page write's event carries, as `kernel.rs` reads it.
fn page_layout(event: &Value) -> Option<&Value> {
    match event.get("type").and_then(Value::as_str)? {
        "pages.reordered" | "pages.rotated" | "pages.deleted" | "pages.inserted"
        | "pages.restored" | "pages.named" => event.get("layout"),
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
    let state = STATES.with(|s| s.borrow().get(doc).and_then(std::rc::Weak::upgrade));
    // `LocalDocumentSignaturesService.complete`: a completed signing moved
    // the document to new saved bytes. What its signatures forbid applies
    // from the next call on (`setProtection`), and the write's second fact
    // is `document.versioned`: byte-level facts must be re-read.
    if event.get("type").and_then(Value::as_str) == Some("signatures.completed") {
        if let Some(state) = &state {
            state.take_protection(event.get("protection").unwrap_or(&Value::Null));
        }
        for listener in listeners.iter() {
            listener(event);
        }
        let mut origin = event.get("origin").cloned().unwrap_or(Value::Null);
        if let Some(tx) = origin.get_mut("tx").filter(|tx| tx.is_object()) {
            tx["index"] = json!(1);
        }
        let versioned = json!({
            "type": "document.versioned",
            "version": event.get("version").cloned().unwrap_or(Value::Null),
            "origin": origin,
        });
        publish(doc, &versioned);
        return;
    }
    // `LocalObjectNumberPool`: a new version numbers objects anew.
    if event.get("type").and_then(Value::as_str) == Some("document.versioned")
        && let Some(state) = &state
    {
        state.pool.versioned(state);
    }
    for listener in listeners {
        listener(event);
    }
}

impl Engine for NativeEngine {
    /// `isLocalEngine`: this is upstream's local engine, on this host.
    fn is_local(&self) -> bool {
        true
    }

    /// `input` names the document's bytes as `{"$host": id}` in the
    /// `HostTable`; the open takes them out of it.
    ///
    /// Once `open.fatMem` has landed the host holds a session, so an open
    /// that then fails (`pages.list`) or is aborted closes it rather than
    /// leaving the bytes parked on the host.
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
        let doc = format!("{}-{n}", self.inner.prefix);
        let opened = self.inner.send(&doc, |job| {
            Ok(WireCall {
                request: json!({
                    "kind": "open.fatMem",
                    "effect": "open",
                    "jobId": job,
                    "docId": doc,
                    "password": password.unwrap_or(Value::Null),
                    "scope": self.inner.scope,
                    "reserveObjectNumbers": OBJECT_NUMBER_POOL_SIZE,
                }),
                payloads: vec![bytes],
                decode: Decode::Result,
            })
        });
        let (promise, resolver) = Promise::<Rc<dyn DocumentHandle>>::pending();
        let aborted = Rc::new(Cell::new(false));
        let inner = self.inner.clone();
        let (settled, stopped) = (resolver.clone(), aborted.clone());
        rt::spawn(async move {
            let mut answer = match opened.await {
                Ok(answer) => answer,
                Err(error) => return settled.reject(error),
            };
            if stopped.get() {
                return inner.close(&doc);
            }
            // A document that needs a password stays open, parked on the
            // host, and comes up as a locked tab: `password_prompt` says
            // so, `incorrect` when the open's password was wrong
            // (`markPasswordRejected`), and `NativeSecurity::unlock` loads
            // it.
            let security = answer.get_mut("security").map(Value::take).unwrap_or(Value::Null);
            let state = Rc::new(NativeDocumentState::new(doc.clone(), inner.clone(), security));
            state.take_protection(answer.get("protection").unwrap_or(&Value::Null));
            // The open asked for numbers: a range it cannot hold is the
            // host's malformed answer, and the open fails rather than
            // handing out fewer numbers than it reserved.
            if let Some(range) = answer.get("objectNumbers")
                && !state.pool.add(Some(range))
            {
                inner.close(&doc);
                return settled.reject(malformed_range(range));
            }
            if answer["passwordRejected"] == Value::Bool(true) {
                state.password_rejected.set(true);
            }
            STATES.with(|s| s.borrow_mut().insert(doc.clone(), Rc::downgrade(&state)));
            if state.locked.get() {
                let pages = json!({ "pages": [] });
                return settled.resolve(Rc::new(NativeDocument { id: doc, inner, pages, state }));
            }
            let listed =
                inner.send(&doc, |job| state.translate("pages.list", None, &[], job)).await;
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
/// write's event replaces.
struct NativeDocument {
    id: String,
    inner: Rc<Inner>,
    pages: Value,
    /// What `security()` answers from, shared with every `NativeSecurity`
    /// it hands out so an unlock is seen by all of them.
    state: Rc<NativeDocumentState>,
}

/// `LocalObjectNumberPool`: the numbers the host reserved for this session
/// and not taken yet, in order, topped up in the background when fewer
/// than half are left.
#[derive(Default)]
struct ObjectNumberPool {
    numbers: RefCell<Vec<u32>>,
    refilling: Cell<bool>,
    lost: RefCell<Vec<(u64, LostListener)>>,
    next_listener: Cell<u64>,
}

/// A host range the pool refused (`ObjectNumberPool::add`): the answer is
/// malformed, as any unreadable answer is.
fn malformed_range(range: &Value) -> Error {
    Error::engine("WireFormat", format!("the host reserved a malformed range {range}"))
}

/// `objectNumbers.reserve`'s continuation: the host's range goes into the
/// pool through `add`, and one `add` refuses rejects the reserve, which
/// would otherwise resolve holding fewer numbers than it promised.
fn reserved_from(
    request: Promise<Value>,
    add: impl FnOnce(&Value) -> bool + 'static,
) -> Promise<()> {
    Promise::spawn(async move {
        let range = request.await?;
        if add(&range) { Ok(()) } else { Err(malformed_range(&range)) }
    })
}

impl ObjectNumberPool {
    /// `add(range)`: `objectNumbersIn([range])`. A range whose `first` or
    /// `count` is not a whole number, or that runs past `u32::MAX`, is
    /// refused and the pool is left as it was: the host only hands out
    /// whole ranges, so reading a malformed one as object 0 would hand a
    /// write a number it does not hold.
    fn add(&self, range: Option<&Value>) -> bool {
        let Some(range) = range else { return false };
        let whole = |key: &str| range.get(key).and_then(Value::as_u64);
        let (Some(first), Some(count)) = (whole("first"), whole("count")) else {
            return false;
        };
        let (Ok(first), Ok(count)) = (u32::try_from(first), u32::try_from(count)) else {
            return false;
        };
        if count > 0 && first.checked_add(count - 1).is_none() {
            return false;
        }
        self.numbers.borrow_mut().extend((0..count).map(|i| first + i));
        true
    }

    /// `topUp()`: fill the pool to its size in the background, once at a
    /// time. A refusal leaves it as it is.
    fn top_up(&self, state: &Rc<NativeDocumentState>) {
        let held = self.numbers.borrow().len() as u32;
        if self.refilling.get() || held >= OBJECT_NUMBER_POOL_SIZE {
            return;
        }
        self.refilling.set(true);
        let request = state.request_numbers(OBJECT_NUMBER_POOL_SIZE - held);
        let weak = Rc::downgrade(state);
        rt::spawn(async move {
            let range = request.await;
            if let Some(state) = weak.upgrade() {
                state.pool.refilling.set(false);
                if let Ok(range) = range {
                    state.pool.add(Some(&range));
                }
            }
        });
    }

    /// `document.versioned`: the host dropped what the session held, and so
    /// does the pool, telling `onLost` listeners; then it tops up.
    fn versioned(&self, state: &Rc<NativeDocumentState>) {
        let numbers = std::mem::take(&mut *self.numbers.borrow_mut());
        if !numbers.is_empty() {
            let lost = ObjectNumbersLost { numbers, reason: ObjectNumbersLostReason::Versioned };
            let listeners: Vec<_> = self.lost.borrow().iter().map(|(_, l)| l.clone()).collect();
            for listener in listeners {
                listener(&lost);
            }
        }
        self.top_up(state);
    }
}

/// One document's authority, security state and object numbers.
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
    /// Whether the last password tried was wrong (the prompt's
    /// `incorrect`).
    password_rejected: Cell<bool>,
    pool: ObjectNumberPool,
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
            password_rejected: Cell::new(false),
            pool: ObjectNumberPool::default(),
        };
        state.take_probe(security);
        state
    }

    /// `guard.can(capability)`.
    fn allows(&self, capability: &str) -> bool {
        !self.locked.get()
            && DocCapability::parse(capability).is_some_and(|c| self.authority.borrow().allows(c))
    }

    /// The handle's side of a request: its authority, and what it may do.
    fn client(self: &Rc<Self>) -> Client {
        let state = self.clone();
        Client {
            authority: self.authority.borrow().to_wire(),
            allows: Box::new(move |capability| state.allows(capability)),
        }
    }

    fn translate(
        self: &Rc<Self>,
        path: &str,
        pon: Option<u32>,
        args: &[Value],
        job: u64,
    ) -> Result<WireCall, Error> {
        let call = translate::translate(&self.id, path, pon, args, job, &self.client())?;
        // The handle's own checks, after `opIdOf` as the TypeScript makes
        // them, and before anything is sent.
        match path {
            "page.annotations.create" => {
                let group = args.first().and_then(|draft| draft.get("groupId"));
                self.assert_may_create([group.and_then(Value::as_str).map(str::to_owned)])?;
            }
            "annotations.import" => self.assert_may_import(args)?,
            _ => {}
        }
        Ok(call)
    }

    /// `ScopeGuard.assertCapability`: a capability the signatures forbid is
    /// `ProtectedDocument`, one the scope doesn't grant `PermissionDenied`.
    fn assert_capability(&self, capability: DocCapability) -> Result<(), Error> {
        let authority = self.authority.borrow();
        if authority.allows(capability) {
            return Ok(());
        }
        match authority
            .protection()
            .filter(|p| protected_capabilities(Some(p)).contains(&capability))
        {
            Some(protection) => {
                Err(Error::engine("ProtectedDocument", describe_protection(capability, protection)))
            }
            None => Err(Error::permission_denied(capability.as_str(), Some("engine-local"))),
        }
    }

    /// `LocalPageAnnotationsService.create`'s and `assertMayCreate`'s
    /// checks, for each group an annotation is made in (`None`: the
    /// caller's own): putting it in another group than the caller's takes
    /// `assertSetGroup`, and making it takes `assertCollab('create')`
    /// against the caller in that group (`targetForSelfCreate`).
    fn assert_may_create(
        &self,
        groups: impl IntoIterator<Item = Option<String>>,
    ) -> Result<(), Error> {
        let authority = self.authority.borrow();
        let identity = authority.identity();
        for group in groups {
            if let Some(group) = &group
                && identity.group_id.as_ref() != Some(group)
                && !authority.allows_set_group(group)
            {
                return Err(Error::permission_denied(
                    format!("annotations:set-group:group={group}"),
                    Some("engine-local"),
                ));
            }
            // `assertAnnotationsUnprotected`, then the collab check.
            if let Some(protection) = authority.protection().filter(|p| {
                protected_capabilities(Some(p)).contains(&DocCapability::DocAnnotateModify)
            }) {
                return Err(Error::engine(
                    "ProtectedDocument",
                    describe_protection(DocCapability::DocAnnotateModify, protection),
                ));
            }
            let target = CollabTarget {
                user_id: identity.user_id.clone(),
                group_id: group.or_else(|| identity.group_id.clone()),
            };
            if !authority.allows_collab(CollabAction::Create, &target) {
                return Err(Error::permission_denied("annotations:create", Some("engine-local")));
            }
        }
        Ok(())
    }

    /// `LocalDocumentAnnotationsService.import`'s checks: a restoring import
    /// writes attribution that isn't the session's, so it takes
    /// `doc.annotate.modify` and `doc.annotate.import`; one that stamps the
    /// session makes each annotation as `create` would, for every group its
    /// items name (an empty bundle still takes the authority to create).
    fn assert_may_import(&self, args: &[Value]) -> Result<(), Error> {
        // `options.attribution ?? 'restore'`, then `=== 'restore'`: only an
        // absent or `null` attribution, or the string itself, restores; any
        // other value stamps.
        let attribution = args
            .get(1)
            .and_then(|options| options.get("attribution"))
            .filter(|v| !v.is_null() && !epdf_kernel::is_undefined(v));
        if attribution.is_none_or(|a| a.as_str() == Some("restore")) {
            self.assert_capability(DocCapability::DocAnnotateModify)?;
            return self.assert_capability(DocCapability::DocAnnotateImport);
        }
        // A `Set` of groups, in the order the items first name them.
        let mut groups: Vec<Option<String>> = Vec::new();
        let items = args.first().and_then(|bundle| bundle.get("items")).and_then(Value::as_array);
        for item in items.into_iter().flatten() {
            let group = item["data"].get("groupId").and_then(Value::as_str).map(str::to_owned);
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
        if groups.is_empty() {
            groups.push(None);
        }
        self.assert_may_create(groups)
    }

    /// `request(count)`: `objectNumbers.reserve`, answering the range.
    fn request_numbers(&self, count: u32) -> Promise<Value> {
        let id = self.id.clone();
        self.inner.send(&self.id, move |job| Ok(translate::reserve_request(&id, job, count)))
    }

    /// `LocalDocumentHandle.download`'s guards: the scope must grant
    /// `doc.download`, and a signed document refuses a rewrite, which would
    /// void every signature.
    fn may_download(&self, options: Option<&Value>) -> Result<(), Error> {
        let authority = self.authority.borrow();
        if self.locked.get() || !authority.allows(DocCapability::DocDownload) {
            return Err(Error::permission_denied("doc.download", None));
        }
        let rewrite = options
            .and_then(|o| o.get("mode"))
            .and_then(Value::as_str)
            .is_some_and(|mode| mode == "rewrite");
        if rewrite && authority.protection().is_some() {
            return Err(Error::engine(
                "ProtectedDocument",
                "the document is signed: a rewrite save would void every signature (use an incremental save)",
            ));
        }
        Ok(())
    }

    /// Adopt the document's signature protection (`ScopeGuard.setProtection`):
    /// what it forbids, no scope grants. `null` or unreadable is none.
    fn take_protection(&self, protection: &Value) {
        let protection = serde_json::from_value::<
            epdf_engine::features::signature::types::DocumentProtection,
        >(protection.clone())
        .ok()
        .filter(|p| p.judged.is_some());
        self.authority.borrow_mut().set_protection(protection);
    }

    /// The security state the probe gives (`securityStateFromProbe`), or
    /// `None` when the host's probe doesn't read as one.
    fn security_state(&self) -> Option<epdf_engine::features::security::state::SecurityState> {
        use epdf_engine::features::security::state::{
            SecurityProbeInfo, StateOptions, security_state_from_probe,
        };
        let probe: SecurityProbeInfo =
            serde_json::from_value(self.security.borrow().clone()).ok()?;
        Some(security_state_from_probe(&probe, &StateOptions::default()))
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

impl NativeDocument {
    /// `pageVerbs`' `one(op, options, answer)`: a page-structure verb as a
    /// one-op `apply`, answering the item's members it names.
    fn page_verb(&self, path: &str, args: &[Value]) -> EngineCall<Value> {
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Null);
        let absent = |v: &Value| v.is_null() || epdf_kernel::is_undefined(v);
        let (op, options, members): (Value, Value, &[&str]) = match path {
            "pages.reorder" => (
                json!({ "type": "pages.reorder", "pages": arg(0), "position": arg(1) }),
                arg(2),
                &["pages", "layout", "meta"],
            ),
            "pages.rotate" => (
                json!({ "type": "pages.rotate", "pages": arg(0), "rotation": arg(1) }),
                arg(2),
                &["layout", "meta"],
            ),
            "pages.delete" => {
                (json!({ "type": "pages.delete", "pages": arg(0) }), arg(1), &["layout", "meta"])
            }
            "pages.insert" | "pages.insertBlank" => {
                let mut op = if path == "pages.insert" {
                    json!({ "type": "pages.insert", "source": arg(0) })
                } else {
                    json!({ "type": "pages.insertBlank", "spec": arg(0) })
                };
                if !absent(&arg(1)) {
                    op["position"] = arg(1);
                }
                let options = arg(2);
                if path == "pages.insertBlank"
                    && let Some(numbers) = options.get("objectNumbers").filter(|v| !absent(v))
                {
                    op["objectNumbers"] = numbers.clone();
                }
                (op, options, &["insertedPages", "layout", "meta"])
            }
            _ => unreachable!("only page verbs reach here"),
        };
        let mut write = json!({});
        if let Some(op_id) = options.get("opId").filter(|v| !absent(v)) {
            write["opId"] = op_id.clone();
        }
        let ty = op["type"].as_str().unwrap_or_default().to_string();
        let applied = self.call("apply", None, vec![json!({ "ops": [op] }), write]);
        let abort = applied.abort_handle();
        let answered = Promise::spawn(async move {
            let result = applied.await?;
            let item = result.get("items").and_then(|items| items.get(0)).cloned();
            // A fresh op is never skipped: only an undo leaves one alone.
            let Some(item) = item.filter(|item| item["type"].as_str() == Some(ty.as_str())) else {
                return Err(Error::engine("Unknown", format!("{ty} wrote nothing")));
            };
            let mut answer = serde_json::Map::new();
            for member in members {
                answer.insert((*member).into(), item.get(*member).cloned().unwrap_or(Value::Null));
            }
            Ok(Value::Object(answer))
        });
        match abort {
            Some(abort) => answered.with_abort(move |reason| abort(reason)),
            None => answered,
        }
    }
}

impl DocumentHandle for NativeDocument {
    /// `isLocalDocument`: changes stay in this tab until downloaded.
    fn is_local(&self) -> bool {
        true
    }

    fn held_object_numbers(&self) -> usize {
        self.state.pool.numbers.borrow().len()
    }

    /// `objectNumbers.take()`: one held number, removed from the pool, and
    /// a top-up when fewer than half are left.
    fn take_object_number(&self) -> Result<Option<u32>, Error> {
        let number = {
            let mut numbers = self.state.pool.numbers.borrow_mut();
            (!numbers.is_empty()).then(|| numbers.remove(0))
        };
        if self.state.pool.numbers.borrow().len() < REFILL_BELOW {
            self.state.pool.top_up(&self.state);
        }
        Ok(number)
    }

    /// `objectNumbers.reserve(count)`: one request, only when the pool is
    /// short.
    fn reserve_object_numbers(&self, count: u32) -> EngineCall<()> {
        if count < 1 {
            return Promise::rejected(
                Error::engine("InvalidArg", format!("cannot reserve {count} object numbers"))
                    .with("details", json!({ "field": "count" })),
            );
        }
        let held = self.state.pool.numbers.borrow().len() as u32;
        if held >= count {
            return Promise::resolved(());
        }
        let request = self.state.request_numbers(count - held);
        let abort = request.abort_handle();
        let state = self.state.clone();
        let reserved = reserved_from(request, move |range| state.pool.add(Some(range)));
        match abort {
            Some(abort) => reserved.with_abort(move |reason| abort(reason)),
            None => reserved,
        }
    }

    fn on_lost_object_numbers(&self, listener: Rc<dyn Fn(&ObjectNumbersLost)>) -> Unsubscribe {
        let pool = &self.state.pool;
        let id = pool.next_listener.get() + 1;
        pool.next_listener.set(id);
        pool.lost.borrow_mut().push((id, listener));
        let state = Rc::downgrade(&self.state);
        Unsubscribe::new(move || {
            if let Some(state) = state.upgrade() {
                state.pool.lost.borrow_mut().retain(|(each, _)| *each != id);
            }
        })
    }

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
        match path {
            "close" => {
                LAYOUTS.with(|l| l.borrow_mut().remove(&self.id));
                STATES.with(|s| s.borrow_mut().remove(&self.id));
            }
            // `render: { getPolicy: () => resolveValue(renderPolicy) }`, the
            // local engine's default `CONTINUOUS_RENDER_POLICY`.
            "render.getPolicy" => return Promise::resolved(json!({ "kind": "continuous" })),
            "pages.reorder" | "pages.rotate" | "pages.delete" | "pages.insert"
            | "pages.insertBlank" => return self.page_verb(path, &args),
            "download" => {
                if let Err(error) = self.state.may_download(args.first()) {
                    return Promise::rejected(error);
                }
            }
            "page.render.image" if self.inner.encoded_renders => {
                return self.inner.send(&self.id, |job| {
                    translate::translate_encoded_render(&self.id, pon, &args, job)
                });
            }
            _ => {}
        }
        self.inner.send(&self.id, |job| self.state.translate(path, pon, &args, job))
    }

    /// `doc.with(facts)`: the local engine ranks queued work by them; this
    /// host's worker runs calls in order, so they change nothing here.
    fn call_with(
        &self,
        _facts: &CallFacts,
        path: &str,
        pon: Option<u32>,
        args: Vec<Value>,
    ) -> EngineCall<Value> {
        self.call(path, pon, args)
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
            .ok_or_else(|| Error::engine("NotFound", format!("no page {pon}")))
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

    /// `setWorkingSet(view, pages)`: what `view` shows of the document, a
    /// scheduling hint the host never answers. It goes on the engine worker
    /// like any other request, so the host takes it in submission order
    /// (the worker is FIFO): after every request submitted before it and
    /// before every one submitted after.
    fn set_working_set(&self, view: &str, pages: Value) {
        if !STATES.with(|s| s.borrow().contains_key(&self.id)) {
            return; // closed
        }
        let request = translate::working_set_request(&self.id, next_job(), view, pages);
        self.inner.worker.submit(request.to_string(), Vec::new(), Box::new(|_| {}));
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
        self.state.allows(capability)
    }

    /// `allowsField(action, target)`: `canSetFieldGroup` for `set-group`,
    /// else `canFieldWrite(groupId, action)`.
    fn allows_field(&self, action: &str, target: &Value) -> bool {
        let group = target.get("groupId").and_then(Value::as_str);
        let authority = self.state.authority.borrow();
        self.open()
            && match action {
                "set-group" => authority.allows_field_group(group.unwrap_or("")),
                "fill" => authority.allows_field_write(FieldWriteAction::Fill, group),
                "sign" => authority.allows_field_write(FieldWriteAction::Sign, group),
                _ => false,
            }
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

    /// `securityStateFromProbe(probe)`, as `LocalDocumentSecurityService`
    /// keeps it: the host answers the probe at open and at unlock. A probe
    /// that doesn't read is handed on as the host gave it.
    fn current(&self) -> Value {
        match self.state.security_state() {
            Some(state) => serde_json::to_value(state).unwrap_or(Value::Null),
            None => self.state.security.borrow().clone(),
        }
    }

    /// `passwordPromptFromState(this.state, this.passwordRejected)`: `hint`
    /// is `null` while the permissions are unknown (a document parked on
    /// its password), and a document opened as its user that has an owner
    /// to upgrade to is `optional`. A probe that doesn't read prompts as a
    /// parked document does, or not at all.
    fn password_prompt(&self) -> Value {
        use epdf_engine::features::security::state::password_prompt_from_state;
        let incorrect = self.state.password_rejected.get();
        match self.state.security_state() {
            Some(state) => serde_json::to_value(password_prompt_from_state(&state, incorrect))
                .unwrap_or(Value::Null),
            None if self.state.locked.get() => {
                json!({ "state": "required", "hint": null, "incorrect": incorrect })
            }
            None => json!({ "state": "none" }),
        }
    }

    /// `document.checkPasswordPermissions`, which on a parked session loads
    /// it with the password, then the page list the open skipped. A wrong
    /// password rejects (`DocPasswordIncorrect`), the tab stays locked and
    /// the prompt says the password was wrong.
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        let state = self.state.clone();
        let password = input.get("password").cloned().unwrap_or(Value::Null);
        let mode = input.get("mode").cloned().unwrap_or_else(|| json!("any"));
        let id = state.id.clone();
        let checked = state.inner.send(&id, |job| {
            Ok(WireCall {
                request: json!({ "kind": "document.checkPasswordPermissions", "effect": "open",
                                 "jobId": job, "docId": id, "password": password, "mode": mode }),
                payloads: Vec::new(),
                decode: Decode::Result,
            })
        });
        let (promise, resolver) = Promise::pending();
        rt::spawn(async move {
            // `{ security, protection }`, as `WorkerHost` answers it.
            let (security, protection) = match checked.await {
                Ok(mut answer) => (answer["security"].take(), answer["protection"].take()),
                Err(error) => {
                    if error.code() == Some("DocPasswordIncorrect") {
                        state.password_rejected.set(true);
                    }
                    return resolver.reject(error);
                }
            };
            state.password_rejected.set(false);
            let pages = state
                .inner
                .send(&state.id, |job| state.translate("pages.list", None, &[], job))
                .await;
            match pages {
                Ok(pages) => {
                    LAYOUTS.with(|l| l.borrow_mut().insert(state.id.clone(), pages));
                    state.take_probe(security.clone());
                    state.take_protection(&protection);
                    resolver.resolve(json!({ "security": security }));
                }
                Err(error) => resolver.reject(error),
            }
        });
        promise
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use epdf_ffi::KernelThread;
    use epdf_kernel::engine::{DocumentHandle, Engine, ObjectNumbersLost, ObjectNumbersLostReason};
    use futures::channel::oneshot;
    use serde_json::{Value, json};

    use super::{NativeEngine, ObjectNumberPool, publish, reserved_from};
    use crate::engine_worker::EngineWorker;
    use crate::host_table::HostTable;
    use crate::translate;

    #[test]
    fn a_malformed_range_leaves_the_pool_as_it_was() {
        let pool = ObjectNumberPool::default();
        assert!(pool.add(Some(&json!({ "first": 5, "count": 3 }))));
        for malformed in [
            json!({ "first": 1.5, "count": 2 }),
            json!({ "first": -1, "count": 2 }),
            json!({ "first": 9 }),
            json!({ "count": 2 }),
            json!({ "first": "9", "count": 2 }),
            json!({ "first": u64::from(u32::MAX) + 1, "count": 1 }),
            json!({ "first": u32::MAX, "count": 2 }),
        ] {
            assert!(!pool.add(Some(&malformed)), "{malformed}");
        }
        assert!(!pool.add(None));
        assert_eq!(*pool.numbers.borrow(), vec![5, 6, 7]);
        // The last number a `u32` holds is still a number.
        assert!(pool.add(Some(&json!({ "first": u32::MAX, "count": 1 }))));
        assert_eq!(*pool.numbers.borrow(), vec![5, 6, 7, u32::MAX]);
    }

    #[test]
    fn a_malformed_reserved_range_rejects_the_reserve_and_leaves_the_pool() {
        epdf_kernel::rt::reset();
        let pool = Rc::new(ObjectNumberPool::default());
        let into = pool.clone();
        let reserved = reserved_from(
            epdf_kernel::Promise::resolved(json!({ "first": 1.5, "count": 10 })),
            move |range| into.add(Some(range)),
        );
        epdf_kernel::rt::run_until_stalled();
        let error = reserved.peek().expect("settled").unwrap_err();
        assert_eq!(error.code(), Some("WireFormat"), "{error:?}");
        assert!(pool.numbers.borrow().is_empty());
        // A whole range fills it and resolves.
        let into = pool.clone();
        let reserved = reserved_from(
            epdf_kernel::Promise::resolved(json!({ "first": 70, "count": 2 })),
            move |range| into.add(Some(range)),
        );
        epdf_kernel::rt::run_until_stalled();
        assert!(reserved.peek().expect("settled").is_ok());
        assert_eq!(*pool.numbers.borrow(), vec![70, 71]);
    }

    /// `password_prompt` on a probe that does not read as a
    /// `DocumentSecurityProbeInfo`: one that parks the document (a password
    /// needed, opened as nobody) prompts as a parked document does, `hint`
    /// null; any other prompts for nothing.
    #[test]
    fn a_probe_that_does_not_read_prompts_by_whether_it_parks_the_document() {
        use epdf_kernel::engine::DocumentSecurity;
        let worker = EngineWorker::spawn();
        let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(worker, post, None))).unwrap();
        let prompts = futures::executor::block_on(thread.call(|engine: &mut NativeEngine| {
            [
                json!({ "encryptionRequiresPassword": true, "pdfOpenedAs": null }),
                json!({ "encryptionRequiresPassword": false }),
            ]
            .map(|probe| {
                let state = Rc::new(super::NativeDocumentState::new(
                    "unread-probe".into(),
                    engine.inner.clone(),
                    probe,
                ));
                assert!(state.security_state().is_none(), "the probe does not read");
                super::NativeSecurity { state }.password_prompt()
            })
        }))
        .unwrap();
        assert_eq!(prompts[0], json!({ "state": "required", "hint": null, "incorrect": false }));
        assert_eq!(prompts[1], json!({ "state": "none" }));
    }

    struct Versioned {
        held_before: usize,
        held_numbers: Vec<u32>,
        held_after_event: usize,
        lost: Vec<ObjectNumbersLost>,
        events: Vec<Value>,
        held_after_refill: usize,
    }

    /// `LocalObjectNumberPool` on `document.versioned`, which a completed
    /// signing raises (`LocalDocumentSignaturesService.complete`): the pool
    /// drops every number it held, tells `onLost` they are gone as
    /// `versioned`, and fills up again.
    #[test]
    fn a_completed_signing_drops_the_pool_tells_on_lost_and_refills() {
        let worker = EngineWorker::spawn();
        let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(worker, post, None))).unwrap();
        let answer = futures::executor::block_on(thread.call(|engine: &mut NativeEngine| {
            let (tx, rx) = oneshot::channel::<Versioned>();
            let input =
                json!({ "$host": HostTable::put(test_support::fixture(test_support::REPORT)) });
            engine.open(input, None).on_settle(move |doc| {
                let doc: Rc<dyn DocumentHandle> = doc.as_ref().ok().cloned().expect("opened");
                // Take a few so the pool is not where the open left it.
                for _ in 0..5 {
                    doc.take_object_number().unwrap();
                }
                let held_before = doc.held_object_numbers();
                let lost = Rc::new(RefCell::new(Vec::new()));
                let seen = lost.clone();
                let _off_lost =
                    doc.on_lost_object_numbers(Rc::new(move |l: &ObjectNumbersLost| {
                        seen.borrow_mut().push(l.clone())
                    }));
                let events = Rc::new(RefCell::new(Vec::new()));
                let seen = events.clone();
                let _off = doc
                    .subscribe(Rc::new(move |e: &Value| seen.borrow_mut().push(e["type"].clone())));
                let state = super::STATES
                    .with(|s| s.borrow().get(&doc.id()).and_then(std::rc::Weak::upgrade))
                    .unwrap();
                let held_numbers = state.pool.numbers.borrow().clone();
                publish(
                    &doc.id(),
                    &json!({
                        "type": "signatures.completed",
                        "version": 2,
                        "origin": { "tx": { "index": 0, "count": 2 } },
                    }),
                );
                let held_after_event = doc.held_object_numbers();
                let doc2 = doc.clone();
                // The refill went to the worker first, so it is answered first.
                doc.call("pages.list", None, vec![]).on_settle(move |_| {
                    let _keep = (_off_lost, _off);
                    let _ = tx.send(Versioned {
                        held_before,
                        held_numbers,
                        held_after_event,
                        lost: lost.borrow().clone(),
                        events: events.borrow().clone(),
                        held_after_refill: doc2.held_object_numbers(),
                    });
                });
            });
            rx
        }))
        .unwrap();
        let v = futures::executor::block_on(answer).unwrap();
        assert_eq!(v.held_before, 59);
        assert_eq!(v.held_after_event, 0, "the pool drops what it held at once");
        assert_eq!(
            v.lost,
            vec![ObjectNumbersLost {
                numbers: v.held_numbers.clone(),
                reason: ObjectNumbersLostReason::Versioned
            }]
        );
        assert_eq!(v.events, vec![json!("signatures.completed"), json!("document.versioned")]);
        assert_eq!(v.held_after_refill, 64, "and fills up again");
    }

    /// `set_working_set` claims submission order: the worker answers its
    /// requests first in, first out, the hint among them.
    #[test]
    fn the_worker_takes_a_working_set_in_submission_order() {
        let worker = EngineWorker::spawn();
        let order = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        let requests = [
            ("before", json!({ "kind": "documents.list", "jobId": 1 })),
            (
                "working set",
                translate::working_set_request(
                    "nothing-open",
                    2,
                    "main",
                    json!([{ "page": { "kind": "objectNumber", "objectNumber": 3 }, "distance": 0 }]),
                ),
            ),
            ("after", json!({ "kind": "documents.list", "jobId": 3 })),
        ];
        for (name, request) in requests {
            let order = order.clone();
            let tx = tx.clone();
            worker.submit(
                request.to_string(),
                Vec::new(),
                Box::new(move |_| {
                    order.lock().unwrap().push(name);
                    let _ = tx.send(());
                }),
            );
        }
        for _ in 0..3 {
            rx.recv().unwrap();
        }
        assert_eq!(*order.lock().unwrap(), vec!["before", "working set", "after"]);
    }
}
