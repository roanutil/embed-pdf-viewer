//! `kernel.ts`: assemble a kernel from an engine and plugins.
//!
//! The structure follows the TypeScript function for function, including
//! where it awaits, because the vectors record the order things happen in
//! and an extra or missing turn reorders two documents opening at once.
//!
//!   plan_plugins       validate dependencies, order them
//!   resolve            workspace singletons, or per-document instances built lazily
//!   document lifecycle one Session per document: transactional open
//!                      (publish last), one idempotent close for every phase
//!   start / destroy    an explicit status machine; destroy closes everything
//!
//! The kernel closes every handle it opened. It never destroys the engine:
//! the caller handed that in.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::LazyLock;

use serde_json::Value;

use crate::action::{Action, CoreAction};
use crate::engine::{DocumentHandle, Engine, EngineCall};
use crate::error::{Error, to_plugin_error};
use crate::event_hook::EventHookSource;
use crate::geometry::PageRef;
use crate::plan::{PluginPlan, plan_plugins};
use crate::plugin::{
    AnyPlugin, ConnectFn, ControllerContext, EffectContext, PluginContext, PluginDef, Services,
    SessionView, slice_key,
};
use crate::rt::{self, Promise};
use crate::scope::{Scope, cancelled, is_cancelled};
use crate::signal::AbortSignal;
use crate::store::{Report, SliceLease, Store, Unsubscribe};
use crate::token::{AnyToken, Token};
use crate::types::*;

/// The built-in token for the document registry.
pub static DOCUMENTS: LazyLock<Token<Documents>> = LazyLock::new(|| Token::new("documents"));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KernelStatus {
    Created,
    Starting,
    Started,
    Failed,
    Destroying,
    Destroyed,
}

impl KernelStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            KernelStatus::Created => "created",
            KernelStatus::Starting => "starting",
            KernelStatus::Started => "started",
            KernelStatus::Failed => "failed",
            KernelStatus::Destroying => "destroying",
            KernelStatus::Destroyed => "destroyed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Opening,
    Locked,
    Bringup,
    Ready,
    Error,
    Closing,
}

/// What `documents.open()` accepts: an input, or a thunk producing one under
/// the loading tab. The thunk gets the session's cancellation signal.
#[derive(Clone)]
pub enum OpenSource {
    Input(Value),
    Thunk(Rc<dyn Fn(AbortSignal) -> Result<Promise<Value>, Error>>),
}

/// `OpenDocumentOptions`: `activate` and `name` are the kernel's; the rest
/// go to the engine as they are.
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    pub activate: Option<bool>,
    pub name: Option<String>,
    /// Everything else, as a JSON object (password, scope, identity, ...).
    pub engine: serde_json::Map<String, Value>,
}

impl OpenOptions {
    /// Split a TypeScript options object into the kernel's two members and
    /// the engine's rest.
    pub fn from_json(value: &Value) -> Self {
        let mut engine = value.as_object().cloned().unwrap_or_default();
        // An `undefined` member is no member once the engine reads it.
        engine.retain(|_, member| !crate::is_undefined(member));
        let activate = engine.remove("activate").and_then(|v| v.as_bool());
        let name = engine.remove("name").and_then(|v| v.as_str().map(str::to_string));
        OpenOptions { activate, name, engine }
    }
}

/// One boot document for `openAll`.
pub struct InitialDocument {
    pub source: OpenSource,
    pub active: bool,
    pub options: OpenOptions,
}

pub struct KernelOptions {
    pub report: Option<Report>,
    /// Observe every plugin dispatch, with its slice key, before the reducer
    /// runs. For devtools and the conformance harness.
    pub on_dispatch: Option<Rc<dyn Fn(&str, &dyn Action)>>,
    /// Once a session finishes closing, drop its capabilities and connectors
    /// so nothing keeps their context (and the session) alive past the
    /// close. On by default: each context holds its session, so with this
    /// off a closed session and its capabilities keep each other alive past
    /// the kernel.
    pub release_closed_sessions: bool,
}

impl Default for KernelOptions {
    fn default() -> Self {
        KernelOptions { report: None, on_dispatch: None, release_closed_sessions: true }
    }
}

// ── sessions ──────────────────────────────────────────────────────────────────

/// The `$host` values in an open source: what a web host kept on its side
/// instead of inlining (the PDF's bytes). Handed back to the host once no
/// session records the source any more: a close drops its session's record,
/// and a failed tab's `retry()` carries the same hold to the next session.
/// A native source has none.
struct HostHold(Vec<Value>);

impl HostHold {
    fn of(input: &OpenSource) -> Option<Rc<HostHold>> {
        let OpenSource::Input(value) = input else { return None };
        HostHold::of_value(value)
    }

    fn of_value(value: &Value) -> Option<Rc<HostHold>> {
        let mut held = Vec::new();
        collect_hosts(value, &mut held);
        (!held.is_empty()).then(|| Rc::new(HostHold(held)))
    }
}

impl Drop for HostHold {
    fn drop(&mut self) {
        self.0.iter().for_each(crate::bytes::release_host);
    }
}

fn collect_hosts(value: &Value, out: &mut Vec<Value>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_hosts(item, out)),
        Value::Object(members) => {
            if members.contains_key("$host") {
                out.push(value.clone());
            }
            members.values().for_each(|member| collect_hosts(member, out));
        }
        _ => {}
    }
}

/// What `open()` was called with, so a failed tab can `retry()`.
type Recorded = (OpenSource, OpenOptions, Option<Rc<HostHold>>);

struct Session {
    change_count: Cell<u64>,
    settle_flushes: Rc<RefCell<Vec<crate::settle::SettleFlush>>>,
    download_wraps: Rc<RefCell<Vec<crate::settle::DownloadWrap>>>,
    burst: RefCell<Option<(String, crate::store::StoreHold)>>,
    changes: crate::changes::DocumentChanges,
    events: EventHookSource<Value>,
    mirror_starts: RefCell<Vec<(String, Rc<dyn Fn()>)>>,
    constructing: RefCell<Vec<*const PluginDef>>,
    id: RefCell<String>,
    instance_id: String,
    name: RefCell<Option<String>>,
    source: RefCell<Option<Recorded>>,
    /// The hold on what a thunk source produced, which a retry produces
    /// afresh.
    produced: RefCell<Option<Rc<HostHold>>>,
    /// The recorded source's hold, kept once the document is ready and the
    /// source itself is dropped: the engine may still read the host's bytes.
    held: RefCell<Option<Rc<HostHold>>>,
    phase: Cell<Phase>,
    handle: RefCell<Option<Rc<dyn DocumentHandle>>>,
    staged_meta: RefCell<Option<DocumentMeta>>,
    scope: Scope,
    operation: RefCell<Option<Promise<Value>>>,
    engine_op: RefCell<Option<Rc<dyn Fn(Option<Value>)>>>,
    cancel: AbortSignal,
    capabilities: RefCell<Vec<(*const PluginDef, Rc<dyn Any>)>>,
    /// The `connect` halves of `create`, run in the effects phase.
    connectors: RefCell<Vec<(*const PluginDef, ConnectFn)>>,
    leases: RefCell<Vec<(*const PluginDef, SliceLease)>>,
    closing: RefCell<Option<Promise<()>>>,
}

impl SessionView for Session {
    fn on_settle(&self, flush: crate::settle::SettleFlush) -> Unsubscribe {
        if !self.settle_flushes.borrow().iter().any(|f| Rc::ptr_eq(f, &flush)) {
            self.settle_flushes.borrow_mut().push(flush.clone());
        }
        let weak = Rc::downgrade(&self.settle_flushes);
        Unsubscribe::new(move || {
            if let Some(flushes) = weak.upgrade() {
                flushes.borrow_mut().retain(|f| !Rc::ptr_eq(f, &flush));
            }
        })
    }
    fn around_download(&self, wrap: crate::settle::DownloadWrap) -> Unsubscribe {
        self.download_wraps.borrow_mut().push(wrap.clone());
        let weak = Rc::downgrade(&self.download_wraps);
        Unsubscribe::new(move || {
            if let Some(wraps) = weak.upgrade() {
                let index = wraps.borrow().iter().position(|w| Rc::ptr_eq(w, &wrap));
                if let Some(index) = index {
                    wraps.borrow_mut().remove(index);
                }
            }
        })
    }

    fn changes(&self) -> crate::changes::DocumentChanges {
        self.changes.clone()
    }
    fn events(&self) -> EventHookSource<Value> {
        self.events.clone()
    }
    fn add_mirror(&self, id: &str, start: Rc<dyn Fn()>) {
        self.mirror_starts.borrow_mut().push((id.into(), start));
    }
    fn id(&self) -> String {
        self.id.borrow().clone()
    }
    fn instance_id(&self) -> String {
        self.instance_id.clone()
    }
    fn handle(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.handle.borrow().clone()
    }
    fn staged_meta(&self) -> Option<DocumentMeta> {
        self.staged_meta.borrow().clone()
    }
    fn scope(&self) -> Scope {
        self.scope.clone()
    }
    fn lease(&self, plugin: &PluginDef) -> Option<SliceLease> {
        let key = plugin as *const PluginDef;
        self.leases.borrow().iter().find(|(p, _)| *p == key).map(|(_, l)| l.clone())
    }
}

/// Insertion-ordered sessions, as a JavaScript `Map` iterates.
#[derive(Default)]
struct Sessions(Vec<(String, Rc<Session>)>);

impl Sessions {
    fn get(&self, id: &str) -> Option<Rc<Session>> {
        self.0.iter().find(|(k, _)| k == id).map(|(_, s)| s.clone())
    }
    fn has(&self, id: &str) -> bool {
        self.0.iter().any(|(k, _)| k == id)
    }
    fn set(&mut self, id: String, session: Rc<Session>) {
        match self.0.iter_mut().find(|(k, _)| *k == id) {
            Some(entry) => entry.1 = session,
            None => self.0.push((id, session)),
        }
    }
    fn delete(&mut self, id: &str) {
        self.0.retain(|(k, _)| k != id);
    }
    fn values(&self) -> Vec<Rc<Session>> {
        self.0.iter().map(|(_, s)| s.clone()).collect()
    }
}

// ── the kernel ────────────────────────────────────────────────────────────────

struct Inner {
    viewer_settings: crate::settings::Settings,
    this: Weak<Inner>,
    engine: Rc<dyn Engine>,
    clock: Rc<dyn crate::clock::HostClock>,
    store: Rc<Store>,
    plan: PluginPlan,
    settings: RefCell<std::collections::HashMap<String, crate::settings::Settings>>,
    document_plugins: Vec<AnyPlugin>,
    workspace_capabilities: RefCell<Vec<(AnyToken, Rc<dyn Any>)>>,
    workspace_leases: RefCell<Vec<(*const PluginDef, SliceLease)>>,
    workspace_connectors: RefCell<Vec<(*const PluginDef, ConnectFn)>>,
    workspace_cancel: AbortSignal,
    workspace_scope: Scope,
    events: LifecycleEvents,
    sessions: RefCell<Sessions>,
    instance_counter: Cell<u64>,
    status: Cell<KernelStatus>,
    start_promise: RefCell<Option<Promise<()>>>,
    destroy_promise: RefCell<Option<Promise<()>>>,
    ticket: Cell<u64>,
    report: Report,
    on_dispatch: Option<Rc<dyn Fn(&str, &dyn Action)>>,
    release_closed_sessions: bool,
}

/// The kernel's lifecycle events, disposed at destroy.
#[derive(Clone)]
pub struct LifecycleEvents {
    pub opened: EventHookSource<Value>,
    pub open_failed: EventHookSource<Value>,
    pub locked: EventHookSource<Value>,
    pub closed: EventHookSource<Value>,
    pub active_changed: EventHookSource<Value>,
    pub pages_changed: EventHookSource<Value>,
    pub unsaved_changes_changed: EventHookSource<Value>,
}

impl LifecycleEvents {
    fn new(report: &Report) -> Self {
        let hook = || EventHookSource::new(Some(report.clone()));
        LifecycleEvents {
            opened: hook(),
            open_failed: hook(),
            locked: hook(),
            closed: hook(),
            active_changed: hook(),
            pages_changed: hook(),
            unsaved_changes_changed: hook(),
        }
    }

    fn dispose(&self) {
        for hook in [
            &self.opened,
            &self.open_failed,
            &self.locked,
            &self.closed,
            &self.active_changed,
            &self.pages_changed,
            &self.unsaved_changes_changed,
        ] {
            hook.dispose();
        }
    }
}

/// The kernel. Cloning shares it.
#[derive(Clone)]
pub struct Kernel(Rc<Inner>);

/// The document registry capability (`DocumentsCapability`).
#[derive(Clone)]
pub struct Documents(Weak<Inner>);

fn core_action(kind: &'static str) -> CoreAction {
    CoreAction { kind }
}

fn is_abort_like(error: &Error) -> bool {
    error.is_abort()
}

fn layout_from_event(event: &Value) -> Option<&Value> {
    match event.get("type").and_then(Value::as_str)? {
        "pages.reordered" | "pages.rotated" | "pages.deleted" | "pages.inserted"
        | "pages.restored" => event.get("layout"),
        _ => None,
    }
}

fn id_of_input(input: &Value) -> Option<String> {
    input.get("id").and_then(Value::as_str).map(str::to_string)
}

impl Inner {
    fn services(&self) -> Weak<dyn Services> {
        let weak: Weak<Inner> = self.this.clone();
        weak as Weak<dyn Services>
    }

    /// Every core write goes through here so an active-tab change is observed
    /// exactly once.
    fn set_core(&self, patch: impl FnOnce(&mut CoreState), action: &dyn Action) {
        let before = self.store.core().active_id.clone();
        self.store.set_core(patch, action);
        let after = self.store.core().active_id.clone();
        if before != after {
            self.events.active_changed.emit(&serde_json::json!({
                "documentId": after,
                "previousDocumentId": before,
            }));
        }
    }

    fn guard_usable(&self, what: &str) -> Result<(), Error> {
        match self.status.get() {
            s @ (KernelStatus::Failed | KernelStatus::Destroying | KernelStatus::Destroyed) => {
                Err(Error::new(format!("[kernel] {what} on a {} kernel", s.as_str())))
            }
            _ => Ok(()),
        }
    }

    fn checkpoint(session: &Session) -> Result<(), Error> {
        if session.cancel.aborted() {
            return Err(cancelled(&format!("closed while opening: {}", session.id())));
        }
        Ok(())
    }

    fn create_session(&self, id: String, name: Option<String>) -> Rc<Session> {
        self.instance_counter.set(self.instance_counter.get() + 1);
        let session = Rc::new_cyclic(|weak: &Weak<Session>| {
            let w = weak.clone();
            let store = Rc::downgrade(&self.store);
            let changes = crate::changes::DocumentChanges::new(
                Rc::new(move |change, options| match w.upgrade().and_then(|s| s.handle()) {
                    Some(handle) => handle.call("apply", None, vec![change, options]),
                    None => Promise::rejected(crate::error::plugin_error(
                        "not-ready",
                        "documents",
                        "the document has not opened yet",
                        None,
                        None,
                    )),
                }),
                Rc::new(move || {
                    if let Some(store) = store.upgrade() {
                        store.notify();
                    }
                }),
                self.report.clone(),
            );
            Session {
                change_count: Cell::new(0),
                settle_flushes: Rc::default(),
                download_wraps: Rc::default(),
                burst: RefCell::new(None),
                changes,
                events: EventHookSource::new(Some(self.report.clone())),
                mirror_starts: RefCell::new(Vec::new()),
                constructing: RefCell::new(Vec::new()),
                instance_id: format!("{id}#{}", self.instance_counter.get()),
                id: RefCell::new(id),
                name: RefCell::new(name),
                source: RefCell::new(None),
                produced: RefCell::new(None),
                held: RefCell::new(None),
                phase: Cell::new(Phase::Opening),
                handle: RefCell::new(None),
                staged_meta: RefCell::new(None),
                scope: Scope::new(self.report.clone()),
                operation: RefCell::new(None),
                engine_op: RefCell::new(None),
                cancel: AbortSignal::new(),
                capabilities: RefCell::new(Vec::new()),
                connectors: RefCell::new(Vec::new()),
                leases: RefCell::new(Vec::new()),
                closing: RefCell::new(None),
            }
        });
        let weak = Rc::downgrade(&session);
        let reserve = weak.clone();
        session.changes.set_object_numbers(
            Rc::new(move || {
                weak.upgrade()
                    .and_then(|s| s.handle())
                    .ok_or_else(|| Error::new("document not ready"))?
                    .take_object_number()
            }),
            Rc::new(move |count| match reserve.upgrade().and_then(|s| s.handle()) {
                Some(h) => h.reserve_object_numbers(count),
                None => Promise::rejected(Error::new("document not ready")),
            }),
        );
        let changes = session.changes.clone();
        session.scope.defer_fn(move || changes.close());
        let events = session.events.clone();
        session.cancel.on_abort(Rc::new(move |_| events.dispose()));
        session
    }

    /// `engineCall`: await an engine or network call under the session's
    /// cancellation, retaining it so close() can abort it, and racing the
    /// cancellation so close()'s join never blocks on a call that cannot be
    /// aborted. A result that lands after the close won goes to
    /// `on_late_result`, through the session scope.
    async fn engine_call<T: Clone + 'static>(
        session: &Rc<Session>,
        call: EngineCall<T>,
        on_late_result: Option<Rc<dyn Fn(T) -> Result<Option<Promise<()>>, Error>>>,
    ) -> Result<T, Error> {
        *session.engine_op.borrow_mut() = call.abort_handle();
        let (cancellation, cancel) = Promise::<T>::pending();
        let what = format!("closed while opening: {}", session.id());
        let listener = {
            let cancel = cancel.clone();
            let what = what.clone();
            Rc::new(move |_: &Error| cancel.reject(cancelled(&what)))
        };
        let registration = if session.cancel.aborted() {
            listener(&cancelled(&what));
            None
        } else {
            Some(session.cancel.on_abort(listener))
        };
        let result = Promise::race(vec![call.clone(), cancellation]).await;
        if let Err(error) = &result
            && is_cancelled(error)
        {
            let scope = session.scope.clone();
            let on_late = on_late_result.clone();
            rt::spawn(async move {
                // `void call.then(...)`: a reaction, not an await.
                if let Ok(value) = call.reaction().await
                    && let Some(on_late) = on_late
                {
                    scope.defer(Box::new(move || on_late(value)));
                }
            });
        }
        if let Some(id) = registration {
            session.cancel.remove(id);
        }
        *session.engine_op.borrow_mut() = None;
        result
    }

    /// One transition at a time per session.
    fn begin_operation(
        &self,
        session: &Rc<Session>,
        run: impl std::future::Future<Output = Result<Value, Error>> + 'static,
    ) -> Result<Promise<Value>, Error> {
        if session.operation.borrow().is_some() {
            return Err(Error::new(format!(
                "[documents] \"{}\" already has an active transition",
                session.id()
            )));
        }
        let (operation, resolver) = Promise::pending();
        let s = session.clone();
        let op = operation.clone();
        rt::spawn(async move {
            let result = run.await;
            // `.finally(() => { if (session.operation === operation) session.operation = null })`
            // runs one turn after `run` settles. `.finally` then settles its
            // promise through `Promise.resolve(onFinally()).then(() => value)`,
            // which the returned promise adopts: one turn for that `.then`
            // and one for the adoption's reaction, so it settles on the third.
            rt::tick().await;
            let current = s.operation.borrow().clone();
            if let Some(current) = current
                && current.same(&op)
            {
                *s.operation.borrow_mut() = None;
            }
            rt::tick().await;
            rt::tick().await;
            resolver.settle(result);
        });
        *session.operation.borrow_mut() = Some(operation.clone());
        Ok(operation)
    }

    fn rekey_session(&self, session: &Rc<Session>, next_id: &str) -> Result<(), Error> {
        let previous_id = session.id();
        if previous_id == next_id {
            return Ok(());
        }
        let core = self.store.core();
        if self.sessions.borrow().has(next_id)
            || core.documents.contains_key(next_id)
            || core.pending.contains_key(next_id)
        {
            return Err(crate::error::plugin_error(
                "conflict",
                "documents",
                format!("duplicate document id: {next_id}"),
                None,
                None,
            ));
        }
        self.sessions.borrow_mut().delete(&previous_id);
        *session.id.borrow_mut() = next_id.to_string();
        self.sessions.borrow_mut().set(next_id.to_string(), session.clone());
        if let Some(slot) = core.pending.get(&previous_id).cloned() {
            let next = next_id.to_string();
            self.set_core(
                |core| {
                    core.pending.shift_remove(&previous_id);
                    core.pending.insert(next.clone(), PendingMeta { id: next.clone(), ..slot });
                    for id in core.order.iter_mut() {
                        if *id == previous_id {
                            *id = next.clone();
                        }
                    }
                    if core.active_id.as_deref() == Some(previous_id.as_str()) {
                        core.active_id = Some(next.clone());
                    }
                },
                &core_action(CORE_ORDER_CHANGED),
            );
        }
        Ok(())
    }

    fn close_session(&self, session: &Rc<Session>) -> Promise<()> {
        if let Some(closing) = session.closing.borrow().clone() {
            return closing;
        }
        let this = self.this.upgrade().expect("kernel");
        let s = session.clone();
        let closing = Promise::spawn(async move {
            s.phase.set(Phase::Closing);
            this.unpublish_slot(&s.id());
            // Write authority ends now.
            let leases: Vec<SliceLease> =
                s.leases.borrow().iter().map(|(_, l)| l.clone()).collect();
            for lease in leases {
                lease.revoke();
            }
            let reason = cancelled(&format!("closed while opening: {}", s.id()));
            s.burst.borrow_mut().take();
            s.mirror_starts.borrow_mut().clear();
            // The change queue closes in the scope's teardown (`create`'s
            // defer), as TS closes it: after the plugins' event sources are
            // disposed, so no listener hears the refusals.
            s.cancel.abort(reason.clone());
            let engine_op = s.engine_op.borrow().clone();
            if let Some(abort) = engine_op {
                abort(Some(reason.to_canonical()));
            }
            let operation = s.operation.borrow().clone();
            if let Some(operation) = operation {
                // `await session.operation?.catch(...)`: the handler runs in
                // the `.catch` reaction, one turn after the operation
                // settles, and the await resumes a turn after that.
                if let Err(error) = operation.await
                    && !is_cancelled(&error)
                    && !is_abort_like(&error)
                {
                    (this.report)(&error);
                }
                rt::tick().await;
            } else {
                // `await session.operation?.catch(...)` on null is `await undefined`.
                rt::tick().await;
            }
            let _ = s.scope.dispose().await;
            let id = s.id();
            let is_current = this.sessions.borrow().get(&id).is_some_and(|c| Rc::ptr_eq(&c, &s));
            if is_current {
                this.sessions.borrow_mut().delete(&id);
            }
            this.events.closed.emit(&serde_json::json!({ "documentId": id }));
            // Nothing retries a closed session, and something may hold it
            // long after: its source goes now, and with it the host's bytes
            // unless a `retry()` took them on to the next session.
            drop(s.source.borrow_mut().take());
            drop(s.produced.borrow_mut().take());
            drop(s.held.borrow_mut().take());
            // Each capability's context holds this session, so while the
            // session holds the capabilities (and their connects) the two
            // keep each other alive past the close, and past the kernel.
            // Nothing resolves a closing session's capabilities again, so
            // dropping them here is safe. A caller can opt out and rely on
            // Weak-guarded deferred work going quiet on its own instead.
            if this.release_closed_sessions {
                let capabilities = std::mem::take(&mut *s.capabilities.borrow_mut());
                let connectors = std::mem::take(&mut *s.connectors.borrow_mut());
                drop((capabilities, connectors));
            }
            Ok(())
        });
        *session.closing.borrow_mut() = Some(closing.clone());
        closing
    }

    // ── store projection ──────────────────────────────────────────────────────

    fn next_active_document(core: &CoreState, removed: &str) -> Option<String> {
        if core.active_id.as_deref() != Some(removed) {
            return core.active_id.clone();
        }
        let index = core.order.iter().position(|id| id == removed).map(|i| i as i64).unwrap_or(-1);
        let remaining: Vec<&String> = core.order.iter().filter(|id| *id != removed).collect();
        if remaining.is_empty() {
            return None;
        }
        let at = (index - 1).max(0) as usize;
        Some(remaining.get(at).copied().unwrap_or(remaining[0]).clone())
    }

    fn publish_pending_slot(&self, session: &Session, activate: bool) {
        let id = session.id();
        let name = session.name.borrow().clone();
        self.set_core(
            |core| {
                core.pending.insert(
                    id.clone(),
                    PendingMeta {
                        id: id.clone(),
                        name,
                        status: PendingStatus::Loading,
                        password_provided: None,
                        error: None,
                    },
                );
                if !core.order.contains(&id) {
                    core.order.push(id.clone());
                }
                if activate || core.active_id.is_none() {
                    core.active_id = Some(id.clone());
                }
            },
            &core_action(CORE_DOCUMENT_OPENING),
        );
    }

    fn publish_locked(&self, session: &Session, password_provided: bool) {
        let id = session.id();
        let name = session.name.borrow().clone();
        self.set_core(
            |core| {
                core.pending.insert(
                    id.clone(),
                    PendingMeta {
                        id: id.clone(),
                        name,
                        status: PendingStatus::Locked,
                        password_provided: Some(password_provided),
                        error: None,
                    },
                );
            },
            &core_action(CORE_DOCUMENT_LOCKED),
        );
        self.events.locked.emit(&serde_json::json!({
            "documentId": session.id(),
            "passwordProvided": password_provided,
        }));
    }

    fn publish_error(&self, session: &Session, error: Error) {
        let id = session.id();
        let name = session.name.borrow().clone();
        let info = to_plugin_error("documents", &error).plugin_error_info();
        self.set_core(
            |core| {
                core.pending.insert(
                    id.clone(),
                    PendingMeta {
                        id: id.clone(),
                        name,
                        status: PendingStatus::Error,
                        password_provided: None,
                        error: Some(error),
                    },
                );
            },
            &core_action(CORE_DOCUMENT_OPEN_FAILED),
        );
        self.events
            .open_failed
            .emit(&serde_json::json!({ "documentId": session.id(), "error": info }));
    }

    fn unpublish_slot(&self, id: &str) {
        let core = self.store.core();
        if !core.pending.contains_key(id) && !core.documents.contains_key(id) {
            return;
        }
        let next_active = Self::next_active_document(&core, id);
        self.set_core(
            |core| {
                core.pending.shift_remove(id);
                core.documents.shift_remove(id);
                core.order.retain(|other| other != id);
                core.active_id = next_active;
            },
            &core_action(CORE_DOCUMENT_REMOVED),
        );
    }

    fn commit_ready(&self, session: &Session) {
        session.phase.set(Phase::Ready);
        // Only a failed open retries, so a ready document needs its source
        // no longer (inline `$bytes` can be the whole PDF). Its host hold
        // stays until close.
        let recorded = session.source.borrow_mut().take();
        if let Some((_, _, hold)) = recorded {
            *session.held.borrow_mut() = hold;
        }
        let meta = session.staged_meta().expect("staged meta");
        let id = session.id();
        self.set_core(
            |core| {
                core.documents.insert(id.clone(), meta);
                core.pending.shift_remove(&id);
            },
            &core_action(CORE_DOCUMENT_ADDED),
        );
        let meta = self.store.core().documents.get(&id).cloned().expect("committed");
        self.events.opened.emit(&serde_json::json!({
            "documentId": id,
            "info": serde_json::to_value(DocInfo::from_meta(&meta)).unwrap_or(Value::Null),
        }));
    }

    fn set_unsaved_changes(&self, session: &Session, dirty: bool) {
        let id = session.id();
        let published = self.store.core().documents.get(&id).cloned();
        let Some(meta) = published else {
            if let Some(meta) = session.staged_meta.borrow_mut().as_mut() {
                meta.has_unsaved_changes = dirty;
            }
            return;
        };
        if meta.instance_id != session.instance_id || meta.has_unsaved_changes == dirty {
            return;
        }
        self.set_core(
            |core| {
                if let Some(meta) = core.documents.get_mut(&id) {
                    meta.has_unsaved_changes = dirty;
                }
            },
            &core_action("@@core/document-changed"),
        );
        self.events
            .unsaved_changes_changed
            .emit(&serde_json::json!({"documentId":id,"hasUnsavedChanges":dirty}));
    }

    fn download_file(
        &self,
        id: Option<&str>,
        mode: Option<Value>,
        layer: bool,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        let verb = if layer { "downloadLayer" } else { "download" };
        let not_ready = move || {
            crate::error::plugin_error(
                "not-ready",
                "documents",
                format!("no open document to {verb}"),
                None,
                None,
            )
        };
        let Some(session) =
            self.session_of(id).filter(|s| matches!(s.phase.get(), Phase::Bringup | Phase::Ready))
        else {
            return Promise::rejected(not_ready());
        };
        let Some(handle) = session.handle() else {
            return Promise::rejected(not_ready());
        };
        if !handle.security().allows("doc.download") {
            return Promise::rejected(
                crate::error::plugin_error(
                    "permission-denied",
                    "documents",
                    format!("documents.{verb} requires doc.download"),
                    None,
                    None,
                )
                .with("permission", "doc.download"),
            );
        }
        if layer && (!handle.is_local() || !handle.has("downloadLayer")) {
            return Promise::rejected(crate::error::plugin_error(
                "unsupported",
                "documents",
                "only the local engine gives a layer to download",
                None,
                None,
            ));
        }
        let this = self.this.upgrade().expect("kernel");
        let changes = session.changes.clone();
        let queue_flush: crate::settle::SettleFlush = Rc::new(move |_| Ok(Some(changes.settle())));
        let mut flushes = vec![queue_flush];
        flushes.extend(session.settle_flushes.borrow().iter().cloned());
        let mut signals = vec![session.cancel.clone()];
        if let Some(signal) = &signal {
            signals.push(signal.clone());
        }
        let settled = crate::settle::settle(flushes, signals, self.report.clone());
        document_errors(Promise::spawn(async move {
            settled.await?;
            // Keep the captured instance, never resolve the active tab again.
            let live = this
                .sessions
                .borrow()
                .get(&session.id())
                .is_some_and(|current| Rc::ptr_eq(&current, &session));
            if !live || !matches!(session.phase.get(), Phase::Bringup | Phase::Ready) {
                return Err(not_ready());
            }
            let read_at = Rc::new(Cell::new(None));
            let read_session = session.clone();
            let read_signal = signal.clone();
            let stamp = read_at.clone();
            let read: crate::settle::DownloadRead = Rc::new(move || {
                if read_session.cancel.aborted()
                    || read_signal.as_ref().is_some_and(AbortSignal::aborted)
                {
                    return Promise::rejected(crate::error::plugin_error(
                        "operation-cancelled",
                        "documents",
                        "operation cancelled",
                        None,
                        None,
                    ));
                }
                stamp.set(Some(read_session.change_count.get()));
                let task = if layer {
                    handle.call("downloadLayer", None, vec![])
                } else {
                    let options = mode
                        .clone()
                        .map(|mode| serde_json::json!({"mode":mode}))
                        .unwrap_or_else(crate::undefined);
                    handle.call("download", None, vec![options])
                };
                let abort = task.abort_handle();
                let task = crate::cancellable::cancellable(
                    "documents",
                    Some(read_session.cancel.clone()),
                    task,
                );
                // Caller cancellation must still reach the original read after
                // the document-lifetime wrapper has been applied.
                let task = match abort {
                    Some(abort) => task.with_abort_member(move |reason| abort(reason)),
                    None => task,
                };
                crate::cancellable::cancellable("documents", read_signal.clone(), task)
            });
            let wraps = session.download_wraps.borrow().clone();
            let bytes = crate::settle::run_wrapped(wraps, read).await?;
            if read_at.get() == Some(session.change_count.get()) && !session.cancel.aborted() {
                this.set_unsaved_changes(&session, false);
            }
            Ok(bytes)
        }))
    }

    // ── capability resolution ─────────────────────────────────────────────────

    fn session_of(&self, document_id: Option<&str>) -> Option<Rc<Session>> {
        let id = match document_id {
            Some(id) => id.to_string(),
            None => self.store.core().active_id.clone()?,
        };
        self.sessions.borrow().get(&id)
    }

    fn workspace_capability(&self, token: &AnyToken) -> Option<Rc<dyn Any>> {
        self.workspace_capabilities
            .borrow()
            .iter()
            .find(|(t, _)| t == token)
            .map(|(_, c)| c.clone())
    }

    fn build_document_capability(
        &self,
        plugin: &AnyPlugin,
        session: &Rc<Session>,
    ) -> Result<Rc<dyn Any>, Error> {
        let key = Rc::as_ptr(plugin);
        if let Some((_, capability)) = session.capabilities.borrow().iter().find(|(p, _)| *p == key)
        {
            return Ok(capability.clone());
        }
        let view = Some(session.clone() as Rc<dyn SessionView>);
        let capability = match &plugin.create {
            Some(create) => {
                let cx = ControllerContext::new(
                    self.services(),
                    plugin,
                    view,
                    session.cancel.clone(),
                    session.scope.clone(),
                )?;
                if session.constructing.borrow().contains(&key) {
                    return Err(Error::new(format!(
                        "[kernel] plugin {} is still being created",
                        plugin.id
                    )));
                }
                session.constructing.borrow_mut().push(key);
                let created = create(cx);
                session.constructing.borrow_mut().retain(|p| *p != key);
                let created = created?;
                if let Some(connect) = created.connect {
                    session.connectors.borrow_mut().push((key, connect));
                }
                created.api
            }
            None => {
                let ctx = PluginContext::new(self.services(), plugin, view)?;
                (plugin.capability.as_ref().expect("provider has a capability"))(ctx)?
            }
        };
        session.capabilities.borrow_mut().push((key, capability.clone()));
        Ok(capability)
    }

    fn resolve_capability(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
    ) -> Result<Rc<dyn Any>, Error> {
        self.guard_usable(&format!("capability(\"{}\")", token.name()))?;
        if let Some(capability) = self.workspace_capability(token) {
            return Ok(capability);
        }
        let Some(provider) = self.plan.provider_of(token).cloned() else {
            return Err(Error::new(format!("No capability \"{}\".", token.name())));
        };
        let id = match document_id {
            Some(id) => id.to_string(),
            None => match self.store.core().active_id.clone() {
                Some(id) => id,
                None => {
                    return Err(Error::new(format!(
                        "Capability \"{}\" requires an active document.",
                        token.name()
                    )));
                }
            },
        };
        let Some(session) = self.sessions.borrow().get(&id) else {
            return Err(Error::new(format!(
                "Capability \"{}\" unavailable: no document \"{id}\".",
                token.name()
            )));
        };
        match session.phase.get() {
            Phase::Bringup | Phase::Ready => self.build_document_capability(&provider, &session),
            phase => {
                let shown = match phase {
                    Phase::Opening => "loading",
                    Phase::Locked => "locked",
                    Phase::Error => "error",
                    Phase::Closing => "closing",
                    _ => unreachable!(),
                };
                Err(Error::new(format!(
                    "Capability \"{}\" unavailable: document \"{id}\" is {shown}.",
                    token.name()
                )))
            }
        }
    }

    fn try_resolve(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
        bringup_counts: bool,
    ) -> Result<Option<Rc<dyn Any>>, Error> {
        if matches!(self.status.get(), KernelStatus::Destroying | KernelStatus::Destroyed) {
            return Ok(None);
        }
        if let Some(capability) = self.workspace_capability(token) {
            return Ok(Some(capability));
        }
        let Some(provider) = self.plan.provider_of(token).cloned() else { return Ok(None) };
        let Some(session) = self.session_of(document_id) else { return Ok(None) };
        let usable = match session.phase.get() {
            Phase::Ready => true,
            Phase::Bringup => bringup_counts,
            _ => false,
        };
        if !usable {
            return Ok(None);
        }
        self.build_document_capability(&provider, &session).map(Some)
    }

    fn document_handle_of(&self, document_id: Option<&str>) -> Option<Rc<dyn DocumentHandle>> {
        let session = self.session_of(document_id)?;
        match session.phase.get() {
            Phase::Bringup | Phase::Ready => session.handle(),
            _ => None,
        }
    }

    // ── document lifecycle ────────────────────────────────────────────────────

    fn next_ticket(&self) -> String {
        self.ticket.set(self.ticket.get() + 1);
        format!("pending:{}", self.ticket.get())
    }

    /// Slices, event subscription, plugin inits and effect setup: every
    /// release deferred into the session scope, every await followed by a
    /// checkpoint. Entirely pre-commit.
    async fn bring_up(this: Rc<Inner>, session: Rc<Session>, snapshot: Value) -> Result<(), Error> {
        session.phase.set(Phase::Bringup);
        let handle = session.handle().expect("handle");
        let mut render_policy = continuous_render_policy();
        if handle.has("render") {
            // `(await handle.render?.getPolicy()) ?? CONTINUOUS_RENDER_POLICY`, best-effort
            if let Ok(policy) = handle.call("render.getPolicy", None, Vec::new()).await
                && !policy.is_null()
                && !crate::is_undefined(&policy)
            {
                render_policy = policy;
            }
        } else {
            rt::tick().await; // `await undefined`
        }
        Self::checkpoint(&session)?;
        let page_count = snapshot.get("pageCount").and_then(Value::as_u64).unwrap_or(0) as u32;
        *session.staged_meta.borrow_mut() = Some(DocumentMeta {
            id: session.id(),
            instance_id: session.instance_id.clone(),
            name: session.name.borrow().clone(),
            page_count,
            has_unsaved_changes: false,
            // `snapshot.pages`: absent stays undefined, as the TypeScript copies it.
            pages: Rc::new(snapshot.get("pages").cloned().unwrap_or_else(crate::undefined)),
            revision: 0,
            render_policy,
        });

        let burst = Rc::downgrade(&session);
        session.scope.defer_fn(move || {
            if let Some(session) = burst.upgrade() {
                session.burst.borrow_mut().take();
            }
        });
        let weak = Rc::downgrade(&this);
        let s = Rc::downgrade(&session);
        let unsubscribe = handle.subscribe(Rc::new(move |event: &Value| {
            let (Some(this), Some(session)) = (weak.upgrade(), s.upgrade()) else { return };
            let tx = event.pointer("/origin/tx");
            let id = tx.and_then(|t| t["id"].as_str());
            let old = if session
                .burst
                .borrow()
                .as_ref()
                .is_some_and(|(open, _)| Some(open.as_str()) != id)
            {
                session.burst.borrow_mut().take()
            } else {
                None
            };
            drop(old);
            if session.burst.borrow().is_none()
                && tx.is_some_and(|t| t["index"] == 0 && t["count"].as_u64().is_some_and(|n| n > 1))
            {
                if let Some(id) = id {
                    *session.burst.borrow_mut() = Some((id.into(), this.store.hold()));
                }
            }
            if session.handle().is_some_and(|h| h.is_local())
                && !matches!(
                    event["type"].as_str(),
                    Some(
                        "stream.desynced"
                            | "document.versioned"
                            | "signatures.prepared"
                            | "signatures.cancelled"
                    )
                )
            {
                session.change_count.set(session.change_count.get() + 1);
                this.set_unsaved_changes(&session, true);
            }
            let Some(layout) = layout_from_event(event) else {
                session.events.emit(event);
                return;
            };
            let id = session.id();
            let core = this.store.core();
            let Some(existing) = core.documents.get(&id).cloned() else {
                session.events.emit(event);
                return;
            };
            let updated = DocumentMeta {
                page_count: layout.get("pageCount").and_then(Value::as_u64).unwrap_or(0) as u32,
                pages: Rc::new(layout.get("pages").cloned().unwrap_or_else(crate::undefined)),
                revision: existing.revision + 1,
                ..existing
            };
            let (revision, pages) = (updated.revision, updated.pages.clone());
            this.set_core(
                |core| {
                    core.documents.insert(id.clone(), updated);
                },
                &core_action(CORE_DOCUMENT_PAGES_UPDATED),
            );
            this.events.pages_changed.emit(&serde_json::json!({
                "documentId": id,
                "revision": revision,
                "pages": pages,
            }));
            session.events.emit(event);
        }));
        session.scope.defer_fn(move || unsubscribe.call());

        for plugin in &this.document_plugins {
            // The lease is the write authority for this instance's slice.
            let lease = this.store.lease_with_text(
                &slice_key(&plugin.id, Some(&session.id())),
                plugin.reducer.clone(),
                (plugin.initial_state)(),
                plugin.to_json.clone(),
                plugin.to_text.clone(),
                Some(session.instance_id.clone()),
            );
            session.leases.borrow_mut().push((Rc::as_ptr(plugin), lease.clone()));
            session.scope.defer_fn(move || lease.revoke()); // LIFO: reverse dependency order
        }
        for plugin in &this.document_plugins {
            if plugin.create.is_some() {
                continue;
            }
            if let Some(init) = &plugin.init {
                let ctx = PluginContext::new(
                    this.services(),
                    plugin,
                    Some(session.clone() as Rc<dyn SessionView>),
                )?;
                match init(ctx)? {
                    Some(promise) => promise.await?,
                    None => rt::tick().await,
                }
            } else {
                rt::tick().await; // `await plugin.init?.(...)` on a plugin without one
            }
            Self::checkpoint(&session)?;
        }
        let hold = this.store.hold();
        for plugin in &this.document_plugins {
            if let Some(effects) = &plugin.effects {
                let ctx = PluginContext::new(
                    this.services(),
                    plugin,
                    Some(session.clone() as Rc<dyn SessionView>),
                )?;
                effects(EffectContext { ctx })?;
            }
            if plugin.create.is_some() {
                // Cheap eager construction, in dependency order.
                this.build_document_capability(plugin, &session)?;
                let key = Rc::as_ptr(plugin);
                let connect = session
                    .connectors
                    .borrow()
                    .iter()
                    .find(|(p, _)| *p == key)
                    .map(|(_, c)| c.clone());
                if let Some(connect) = connect {
                    connect()?;
                }
                let starts: Vec<_> = session
                    .mirror_starts
                    .borrow()
                    .iter()
                    .filter(|(id, _)| id == &plugin.id)
                    .map(|(_, f)| f.clone())
                    .collect();
                for start in starts {
                    start();
                }
            }
        }
        drop(hold);
        let weak = Rc::downgrade(&session);
        let off = handle.subscribe(Rc::new(move |event| {
            if let Some(session) = weak.upgrade() {
                if let Some(id) = crate::mirror::last_event_of(event) {
                    let close = session.burst.borrow().as_ref().is_some_and(|(open, _)| open == id);
                    if close {
                        let held = session.burst.borrow_mut().take();
                        drop(held);
                    }
                }
            }
        }));
        session.scope.defer_fn(move || off.call());
        Self::checkpoint(&session)
    }

    fn open_document(
        &self,
        input: OpenSource,
        options: OpenOptions,
        hold: Option<Rc<HostHold>>,
    ) -> Promise<Value> {
        // An async function: its synchronous throws are rejections, and the
        // operation it returns is adopted.
        match self.reserve_and_open(input, options, hold) {
            Ok((_, operation)) => adopt(operation),
            Err(error) => Promise::rejected(error),
        }
    }

    /// Reserve the tab slot synchronously and start the open; the slot id at
    /// once, and the open's promise.
    fn reserve_and_open(
        &self,
        input: OpenSource,
        options: OpenOptions,
        hold: Option<Rc<HostHold>>,
    ) -> Result<(Rc<Session>, Promise<Value>), Error> {
        self.reserve_and_open_as(input, options, hold, None)
    }

    fn reserve_and_open_as(
        &self,
        input: OpenSource,
        options: OpenOptions,
        hold: Option<Rc<HostHold>>,
        reuse_id: Option<String>,
    ) -> Result<(Rc<Session>, Promise<Value>), Error> {
        self.guard_usable("documents.open()")?;
        let hold = hold.or_else(|| HostHold::of(&input));
        let recorded = (input.clone(), options.clone(), hold);
        let OpenOptions { activate, name, engine: mut engine_options } = options;
        let viewer = self.viewer_settings.get();
        for key in ["scope", "identity"] {
            if engine_options.get(key).is_none_or(|v| v.is_null() || crate::is_undefined(v))
                && let Some(value) =
                    viewer.get(key).filter(|v| !v.is_null() && !crate::is_undefined(v))
            {
                engine_options.insert(key.into(), value.clone());
            }
        }
        let requested_id = reuse_id.unwrap_or_else(|| match &input {
            OpenSource::Thunk(_) => self.next_ticket(),
            OpenSource::Input(value) => id_of_input(value).unwrap_or_else(|| self.next_ticket()),
        });
        if self.sessions.borrow().has(&requested_id) {
            return Err(crate::error::plugin_error(
                "conflict",
                "documents",
                format!("document already open: {requested_id}"),
                None,
                None,
            ));
        }
        let session = self.create_session(requested_id.clone(), name);
        *session.source.borrow_mut() = Some(recorded);
        self.sessions.borrow_mut().set(requested_id, session.clone());
        self.publish_pending_slot(&session, activate.unwrap_or(true));

        let this = self.this.upgrade().expect("kernel");
        let s = session.clone();
        let run = async move {
            let result = Self::open_steps(this.clone(), s.clone(), input, engine_options).await;
            match result {
                Ok(id) => {
                    let core = this.store.core();
                    let document = core
                        .documents
                        .get(&id)
                        .map(DocInfo::from_meta)
                        .or_else(|| core.pending.get(&id).map(DocInfo::from_pending))
                        .ok_or_else(|| {
                            crate::error::plugin_error(
                                "instance-closed",
                                "documents",
                                format!("{id} was closed"),
                                None,
                                None,
                            )
                        })?;
                    Ok(serde_json::json!({"document":document}))
                }
                Err(error) => {
                    if is_cancelled(&error) {
                        return Err(error);
                    }
                    if s.phase.get() == Phase::Closing || is_abort_like(&error) {
                        return Err(cancelled(&format!("closed while opening: {}", s.id())));
                    }
                    let _ = s.scope.dispose().await;
                    s.phase.set(Phase::Error);
                    this.publish_error(&s, error.clone());
                    Err(error)
                }
            }
        };
        let done = self.begin_operation(&session, run)?;
        Ok((session, done))
    }

    async fn open_steps(
        this: Rc<Inner>,
        session: Rc<Session>,
        input: OpenSource,
        engine_options: serde_json::Map<String, Value>,
    ) -> Result<String, Error> {
        let mut source = match input {
            OpenSource::Thunk(thunk) => {
                // A synchronous throw skips `engineCall`: it leaves
                // `input(signal)` before there is a promise to race.
                let produced = thunk(session.cancel.clone())?;
                let value = rt::returned(Self::engine_call(&session, produced, None)).await?;
                *session.produced.borrow_mut() = HostHold::of_value(&value);
                value
            }
            OpenSource::Input(value) => value,
        };
        // As for the options: an `undefined` member is no member.
        if let Value::Object(members) = &mut source {
            members.retain(|_, member| !crate::is_undefined(member));
        }
        Self::checkpoint(&session)?;
        if let Some(source_id) = id_of_input(&source) {
            this.rekey_session(&session, &source_id)?;
        }

        let on_late: Rc<dyn Fn(Rc<dyn DocumentHandle>) -> Result<Option<Promise<()>>, Error>> =
            Rc::new(|late: Rc<dyn DocumentHandle>| Ok(Some(close_handle(&late))));
        let options_value = Value::Object(engine_options.clone());
        let handle = rt::returned(Self::engine_call(
            &session,
            this.engine.open(source.clone(), Some(options_value)),
            Some(on_late),
        ))
        .await?;
        *session.handle.borrow_mut() = Some(handle.clone());
        {
            let handle = handle.clone();
            session.scope.defer(Box::new(move || Ok(Some(close_handle(&handle)))));
        }
        Self::checkpoint(&session)?;
        if handle.id() != session.id() {
            this.rekey_session(&session, &handle.id())?;
        }

        let prompt = handle.security().password_prompt();
        if prompt.get("state").and_then(Value::as_str) == Some("required") {
            let password_provided =
                prompt.get("incorrect").and_then(Value::as_bool).unwrap_or(false);
            session.phase.set(Phase::Locked);
            this.publish_locked(&session, password_provided);
            return Ok(session.id());
        }

        let snapshot = rt::returned(Self::engine_call(
            &session,
            handle.call("pages.list", None, Vec::new()),
            None,
        ))
        .await?;
        Self::checkpoint(&session)?;
        rt::returned(Self::bring_up(this.clone(), session.clone(), snapshot)).await?;
        this.commit_ready(&session);
        Ok(session.id())
    }

    fn unlock_document(&self, id: &str, password: String) -> Promise<()> {
        match self.unlock_document_sync(id, password, None) {
            Ok(promise) => promise,
            Err(error) => Promise::rejected(error),
        }
    }

    fn unlock_document_sync(
        &self,
        id: &str,
        password: String,
        signal: Option<AbortSignal>,
    ) -> Result<Promise<()>, Error> {
        self.guard_usable("documents.unlock()")?;
        let session = self.sessions.borrow().get(id);
        let session = match session {
            Some(s) if s.phase.get() == Phase::Locked && s.handle().is_some() => s,
            _ => {
                return Err(crate::error::plugin_error(
                    "invalid-input",
                    "documents",
                    format!("document is not locked: {id}"),
                    None,
                    None,
                ));
            }
        };
        let handle = session.handle().expect("handle");
        let this = self.this.upgrade().expect("kernel");
        let s = session.clone();
        let run = async move {
            let unlock = crate::cancellable::cancellable(
                "documents",
                signal,
                handle.security().unlock(serde_json::json!({ "password": password })),
            );
            if let Err(error) = rt::returned(Self::engine_call(&s, unlock, None)).await {
                if s.phase.get() == Phase::Closing || is_abort_like(&error) {
                    return Err(cancelled(&format!("closed while opening: {}", s.id())));
                }
                return Err(error);
            }
            Self::checkpoint(&s)?;
            let steps = async {
                let snapshot = rt::returned(Self::engine_call(
                    &s,
                    handle.call("pages.list", None, Vec::new()),
                    None,
                ))
                .await?;
                Self::checkpoint(&s)?;
                rt::returned(Self::bring_up(this.clone(), s.clone(), snapshot)).await?;
                this.commit_ready(&s);
                Ok::<(), Error>(())
            };
            if let Err(error) = steps.await {
                if is_cancelled(&error) {
                    return Err(error);
                }
                if s.phase.get() == Phase::Closing || is_abort_like(&error) {
                    return Err(cancelled(&format!("closed while opening: {}", s.id())));
                }
                let _ = s.scope.dispose().await;
                s.phase.set(Phase::Error);
                this.publish_error(&s, error.clone());
                return Err(error);
            }
            Ok(Value::Null)
        };
        let operation = self.begin_operation(&session, run)?;
        // `return beginOperation(...)` from an async function adopts the
        // operation. It is still pending here, so the adoption settles one
        // turn after it does, which is what `.then` costs as well.
        Ok(Promise::spawn(async move {
            let result = operation
                .reaction()
                .await
                .map_err(|e| crate::error::to_plugin_error("documents", &e));
            rt::tick().await;
            result.map(|_| ())
        }))
    }

    fn close_document(&self, id: &str) -> Promise<()> {
        let session = self.sessions.borrow().get(id);
        match session {
            None => Promise::spawn(async { Ok(()) }),
            Some(session) => {
                let closing = self.close_session(&session);
                // `await session.close()` inside an async function.
                Promise::spawn(async move { closing.await })
            }
        }
    }

    fn reorder(&self, next: Vec<String>) {
        self.set_core(|core| core.order = next, &core_action(CORE_ORDER_CHANGED));
    }
}

impl Services for Inner {
    fn clock(&self) -> Rc<dyn crate::clock::HostClock> {
        self.clock.clone()
    }
    fn settings(&self, id: &str) -> Option<crate::settings::Settings> {
        self.settings.borrow().get(id).cloned()
    }
    fn engine(&self) -> Rc<dyn Engine> {
        self.engine.clone()
    }
    fn store(&self) -> Rc<Store> {
        self.store.clone()
    }
    fn workspace_scope(&self) -> Scope {
        self.workspace_scope.clone()
    }
    fn workspace_lease(&self, plugin: &PluginDef) -> Option<SliceLease> {
        let key = plugin as *const PluginDef;
        self.workspace_leases.borrow().iter().find(|(p, _)| *p == key).map(|(_, l)| l.clone())
    }
    fn report(&self) -> Report {
        self.report.clone()
    }
    fn resolve(&self, token: &AnyToken, document_id: Option<&str>) -> Result<Rc<dyn Any>, Error> {
        self.resolve_capability(token, document_id)
    }
    fn try_resolve_internal(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<dyn Any>>, Error> {
        self.try_resolve(token, document_id, true)
    }
    fn document_handle(&self, document_id: Option<&str>) -> Option<Rc<dyn DocumentHandle>> {
        self.document_handle_of(document_id)
    }
    fn observe_dispatch(&self, slice: &str, action: &dyn Action) {
        if let Some(observe) = &self.on_dispatch {
            observe(slice, action);
        }
    }
}

impl Kernel {
    pub fn settings_of(&self, token: &AnyToken) -> Result<crate::settings::Settings, Error> {
        let plugin = self
            .0
            .plan
            .ordered
            .iter()
            .find(|p| p.token.as_ref() == Some(token))
            .ok_or_else(|| Error::new("[kernel] unknown settings token"))?;
        self.0
            .settings
            .borrow()
            .get(&plugin.id)
            .cloned()
            .ok_or_else(|| Error::new("[kernel] plugin declares no settings"))
    }

    /// `createKernel`. Fails where the TypeScript throws: a bad plugin list,
    /// or a workspace capability whose constructor throws.
    pub fn new(
        engine: Rc<dyn Engine>,
        plugins: Vec<AnyPlugin>,
        options: KernelOptions,
    ) -> Result<Kernel, Error> {
        Self::new_with_clock(engine, plugins, options, Rc::new(crate::clock::TimerClock))
    }

    pub fn new_with_clock(
        engine: Rc<dyn Engine>,
        plugins: Vec<AnyPlugin>,
        options: KernelOptions,
        clock: Rc<dyn crate::clock::HostClock>,
    ) -> Result<Kernel, Error> {
        Self::new_with_clock_and_settings(engine, plugins, options, clock, None)
    }

    pub fn new_with_viewer_settings(
        engine: Rc<dyn Engine>,
        plugins: Vec<AnyPlugin>,
        options: KernelOptions,
        registered: Option<Value>,
    ) -> Result<Kernel, Error> {
        Self::new_with_clock_and_settings(
            engine,
            plugins,
            options,
            Rc::new(crate::clock::TimerClock),
            registered,
        )
    }

    pub fn get_settings(&self) -> Rc<Value> {
        self.0.viewer_settings.get()
    }
    pub fn update_settings(&self, changes: &Value) {
        self.0.viewer_settings.update_settings(changes);
    }
    pub fn reset_settings(&self) {
        self.0.viewer_settings.reset_settings();
    }
    pub fn on_settings_changed(&self) -> EventHookSource<crate::settings::SettingsChanged> {
        self.0.viewer_settings.on_settings_changed()
    }

    pub fn new_with_clock_and_settings(
        engine: Rc<dyn Engine>,
        plugins: Vec<AnyPlugin>,
        options: KernelOptions,
        clock: Rc<dyn crate::clock::HostClock>,
        registered: Option<Value>,
    ) -> Result<Kernel, Error> {
        let report = options
            .report
            .unwrap_or_else(|| Rc::new(|error: &Error| eprintln!("[kernel] {error}")));
        let store = Store::new(report.clone());
        let plan = plan_plugins(&plugins, DOCUMENTS.any())?;
        let document_plugins =
            plan.ordered.iter().filter(|p| p.scope == PluginScope::Document).cloned().collect();
        let workspace_scope = Scope::new(report.clone());
        let events = LifecycleEvents::new(&report);
        {
            let events = events.clone();
            workspace_scope.defer_fn(move || events.dispose());
        }
        let weak_store = Rc::downgrade(&store);
        let viewer_settings = crate::settings::Settings::new(
            crate::settings::SettingsDeclaration {
                defaults: serde_json::json!({"identity":null,"scope":null,"accent":"#3858e9","page":{"background":"#ffffff","shadow":"0 6px 18px rgb(0 0 0 / 0.18)"}}),
                registered,
                whole: vec!["identity".into()],
            },
            Rc::new(move || {
                if let Some(store) = weak_store.upgrade() {
                    store.notify();
                }
            }),
            report.clone(),
        );
        let dispose_viewer = viewer_settings.clone();
        workspace_scope.defer_fn(move || dispose_viewer.dispose());
        let release_closed_sessions = options.release_closed_sessions;
        let inner = Rc::new_cyclic(|this| Inner {
            this: this.clone(),
            viewer_settings,
            engine,
            clock,
            store,
            plan,
            settings: RefCell::new(std::collections::HashMap::new()),
            document_plugins,
            workspace_capabilities: RefCell::new(Vec::new()),
            workspace_leases: RefCell::new(Vec::new()),
            workspace_connectors: RefCell::new(Vec::new()),
            workspace_cancel: AbortSignal::new(),
            workspace_scope,
            events,
            sessions: RefCell::new(Sessions::default()),
            instance_counter: Cell::new(0),
            status: Cell::new(KernelStatus::Created),
            start_promise: RefCell::new(None),
            destroy_promise: RefCell::new(None),
            ticket: Cell::new(0),
            report,
            on_dispatch: options.on_dispatch,
            release_closed_sessions,
        });
        let documents: Rc<dyn Any> = Rc::new(Documents(Rc::downgrade(&inner)));
        inner.workspace_capabilities.borrow_mut().push((DOCUMENTS.any().clone(), documents));

        for plugin in &inner.plan.ordered {
            if let Some(declaration) = &plugin.settings {
                let store = Rc::downgrade(&inner.store);
                let settings = crate::settings::Settings::new(
                    declaration.clone(),
                    Rc::new(move || {
                        if let Some(store) = store.upgrade() {
                            store.notify();
                        }
                    }),
                    inner.report.clone(),
                );

                inner.settings.borrow_mut().insert(plugin.id.clone(), settings);
            }
            if plugin.scope != PluginScope::Document {
                let lease = inner.store.lease_with_text(
                    &plugin.id,
                    plugin.reducer.clone(),
                    (plugin.initial_state)(),
                    plugin.to_json.clone(),
                    plugin.to_text.clone(),
                    None,
                );
                inner.workspace_leases.borrow_mut().push((Rc::as_ptr(plugin), lease));
            }
        }
        let stores = inner.settings.borrow().values().cloned().collect::<Vec<_>>();
        inner.workspace_scope.defer_fn(move || {
            for settings in stores {
                settings.dispose();
            }
        });
        for plugin in &inner.plan.ordered {
            if plugin.scope == PluginScope::Document {
                continue;
            }
            if let Some(create) = &plugin.create {
                let cx = ControllerContext::new(
                    inner.services(),
                    plugin,
                    None,
                    inner.workspace_cancel.clone(),
                    inner.workspace_scope.clone(),
                )?;
                let created = create(cx)?;
                if let Some(token) = &plugin.token {
                    inner.workspace_capabilities.borrow_mut().push((token.clone(), created.api));
                }
                if let Some(connect) = created.connect {
                    inner.workspace_connectors.borrow_mut().push((Rc::as_ptr(plugin), connect));
                }
            } else if let (Some(token), Some(capability)) = (&plugin.token, &plugin.capability) {
                let ctx = PluginContext::new(inner.services(), plugin, None)?;
                let built = capability(ctx)?;
                inner.workspace_capabilities.borrow_mut().push((token.clone(), built));
            }
        }
        Ok(Kernel(inner))
    }

    pub fn engine(&self) -> Rc<dyn Engine> {
        self.0.engine.clone()
    }

    pub fn documents(&self) -> Documents {
        Documents(Rc::downgrade(&self.0))
    }

    pub fn status(&self) -> KernelStatus {
        self.0.status.get()
    }

    /// Whether a plugin in this kernel's config provides `token`, for a
    /// document or the workspace.
    pub fn provides(&self, token: &AnyToken) -> bool {
        self.0.workspace_capability(token).is_some() || self.0.plan.provider_of(token).is_some()
    }

    /// `kernel.capability(token, documentId?)`.
    pub fn capability_any(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
    ) -> Result<Rc<dyn Any>, Error> {
        self.0.resolve_capability(token, document_id)
    }

    /// `kernel.tryCapability(token, documentId?)`: ready documents only.
    pub fn try_capability_any(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<dyn Any>>, Error> {
        self.0.try_resolve(token, document_id, false)
    }

    pub fn capability<C: Clone + 'static>(
        &self,
        token: &Token<C>,
        document_id: Option<&str>,
    ) -> Result<C, Error> {
        crate::plugin::downcast(self.capability_any(token.any(), document_id)?, token.name())
    }

    pub fn try_capability<C: Clone + 'static>(
        &self,
        token: &Token<C>,
        document_id: Option<&str>,
    ) -> Result<Option<C>, Error> {
        match self.try_capability_any(token.any(), document_id)? {
            Some(any) => crate::plugin::downcast(any, token.name()).map(Some),
            None => Ok(None),
        }
    }

    pub fn scope_of(&self, token: &AnyToken) -> PluginScope {
        self.0.plan.scope_of(token)
    }

    pub fn subscribe(&self, listener: Rc<crate::store::ChangeListener>) -> Unsubscribe {
        self.0.store.subscribe(listener)
    }

    pub fn store(&self) -> Rc<Store> {
        self.0.store.clone()
    }

    /// `getState()` as canonical JSON.
    pub fn state_json(&self) -> Value {
        self.0.store.state_json()
    }

    /// `getState()` as canonical JSON text, cached per store revision.
    pub fn state_text(&self) -> Rc<str> {
        self.0.store.state_text()
    }

    /// Moves whenever [`state_json`](Self::state_json) could change.
    pub fn state_revision(&self) -> u64 {
        self.0.store.revision()
    }

    /// Moves whenever core changes: a document opened, closed, became ready
    /// or active.
    pub fn core_revision(&self) -> u64 {
        self.0.store.core_revision()
    }

    /// A revision for core and for each slice (`Store::revisions_json`).
    pub fn state_revisions(&self) -> String {
        self.0.store.revisions_json()
    }

    /// `getState().core` as canonical JSON text.
    pub fn core_text(&self) -> Rc<str> {
        self.0.store.core_text()
    }

    /// `getState().plugins[key]` as canonical JSON text.
    pub fn slice_text(&self, key: &str) -> Option<Rc<str>> {
        self.0.store.slice_text(key)
    }

    pub fn core(&self) -> Rc<CoreState> {
        self.0.store.core()
    }

    /// Idempotent: a second call joins the first.
    pub fn start(&self) -> Promise<()> {
        let inner = &self.0;
        if let s @ (KernelStatus::Destroying | KernelStatus::Destroyed | KernelStatus::Failed) =
            inner.status.get()
        {
            return Promise::rejected(Error::new(format!(
                "[kernel] start() on a {} kernel",
                s.as_str()
            )));
        }
        if let Some(start) = inner.start_promise.borrow().clone() {
            return start;
        }
        let this = inner.clone();
        let start = Promise::spawn(async move {
            this.status.set(KernelStatus::Starting);
            let result: Result<(), Error> = async {
                for plugin in this.plan.ordered.clone() {
                    if this.status.get() != KernelStatus::Starting {
                        return Ok(());
                    }
                    if plugin.scope != PluginScope::Document {
                        match &plugin.init {
                            Some(init) => {
                                let ctx = PluginContext::new(this.services(), &plugin, None)?;
                                match init(ctx)? {
                                    Some(promise) => promise.await?,
                                    None => rt::tick().await,
                                }
                            }
                            None => rt::tick().await,
                        }
                    }
                }
                if this.status.get() != KernelStatus::Starting {
                    return Ok(());
                }
                for plugin in this.plan.ordered.clone() {
                    if plugin.scope == PluginScope::Document {
                        continue;
                    }
                    if let Some(effects) = &plugin.effects {
                        let ctx = PluginContext::new(this.services(), &plugin, None)?;
                        effects(EffectContext { ctx })?;
                    }
                    let key = Rc::as_ptr(&plugin);
                    let connect = this
                        .workspace_connectors
                        .borrow()
                        .iter()
                        .find(|(p, _)| *p == key)
                        .map(|(_, c)| c.clone());
                    if let Some(connect) = connect {
                        connect()?;
                    }
                }
                this.status.set(KernelStatus::Started);
                Ok(())
            }
            .await;
            if let Err(error) = result {
                this.status.set(KernelStatus::Failed);
                let _ = this.workspace_scope.dispose().await;
                this.workspace_capabilities.borrow_mut().clear();
                return Err(error);
            }
            Ok(())
        });
        *inner.start_promise.borrow_mut() = Some(start.clone());
        start
    }

    /// Idempotent: every call returns the same promise. Never rejects.
    pub fn destroy(&self) -> Promise<()> {
        let inner = &self.0;
        if let Some(destroy) = inner.destroy_promise.borrow().clone() {
            return destroy;
        }
        let this = inner.clone();
        let destroy = Promise::spawn(async move {
            let was_failed = this.status.get() == KernelStatus::Failed;
            this.status.set(KernelStatus::Destroying); // an in-flight start exits at its next check
            this.workspace_cancel.abort(cancelled("kernel destroyed"));
            let start = this.start_promise.borrow().clone();
            match start {
                Some(start) => {
                    // `await startPromise?.catch(...)`: the `.catch` reaction,
                    // then the await's own turn.
                    if let Err(error) = start.await
                        && !is_cancelled(&error)
                        && !was_failed
                    {
                        (this.report)(&error);
                    }
                    rt::tick().await;
                }
                None => rt::tick().await,
            }
            let sessions = this.sessions.borrow().values();
            let closes: Vec<Promise<()>> = sessions.iter().map(|s| this.close_session(s)).collect();
            all_settled(closes).await;
            this.sessions.borrow_mut().0.clear();
            let _ = this.workspace_scope.dispose().await;
            this.workspace_capabilities.borrow_mut().clear();
            this.store.destroy();
            this.status.set(KernelStatus::Destroyed);
            Ok(())
        });
        *inner.destroy_promise.borrow_mut() = Some(destroy.clone());
        destroy
    }
}

/// `handle.close()` as a teardown returns it: the engine's
/// `AbortablePromise` itself, which the scope's `await` adopts, not a
/// `.then` of it.
fn close_handle(handle: &Rc<dyn DocumentHandle>) -> Promise<()> {
    handle.call("close", None, Vec::new()).convert(|_| Ok(()))
}

/// `return promise` from an `async` function: the function's own promise
/// adopts it, through a resolve-thenable job that registers one turn after
/// the return, then the reaction one turn after `promise` settles.
fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Promise<T> {
    Promise::spawn(async move {
        rt::tick().await;
        promise.reaction().await
    })
}

/// `Promise.allSettled`, awaited: settles one turn after the last input.
async fn all_settled(promises: Vec<Promise<()>>) {
    let (all, resolver) = Promise::<()>::pending();
    let remaining = Rc::new(Cell::new(promises.len()));
    if promises.is_empty() {
        resolver.resolve(());
    }
    for promise in promises {
        let remaining = remaining.clone();
        let resolver = resolver.clone();
        promise.on_settle(move |_| {
            let remaining = remaining.clone();
            let resolver = resolver.clone();
            rt::spawn(async move {
                rt::tick().await;
                remaining.set(remaining.get() - 1);
                if remaining.get() == 0 {
                    resolver.resolve(());
                }
            });
        });
    }
    let _ = all.await;
}

fn document_errors<T: Clone + 'static>(task: Promise<T>) -> Promise<T> {
    let (out, resolver) = Promise::pending();
    task.on_settle(move |result| {
        resolver.settle(result.clone().map_err(|e| crate::error::to_plugin_error("documents", &e)))
    });
    out
}

impl Documents {
    fn inner(&self) -> Result<Rc<Inner>, Error> {
        self.0.upgrade().ok_or_else(|| Error::new("[kernel] used after the kernel was dropped"))
    }

    pub fn open_with_signal(
        &self,
        input: OpenSource,
        options: OpenOptions,
        signal: AbortSignal,
    ) -> Promise<Value> {
        if signal.aborted() {
            return Promise::rejected(crate::error::plugin_error(
                "operation-cancelled",
                "documents",
                "the open was cancelled",
                None,
                None,
            ));
        }
        let inner = match self.inner() {
            Ok(i) => i,
            Err(e) => return Promise::rejected(e),
        };
        let (session, done) = match inner.reserve_and_open(input, options, None) {
            Ok(v) => v,
            Err(e) => return Promise::rejected(crate::error::to_plugin_error("documents", &e)),
        };
        let cancel = inner.clone();
        let registration = signal.on_abort(Rc::new(move |_| {
            cancel.close_session(&session);
        }));
        let task = adopt(done);
        let (out, resolver) = Promise::pending();
        task.on_settle(move |result| {
            signal.remove(registration);
            resolver
                .settle(result.clone().map_err(|e| crate::error::to_plugin_error("documents", &e)));
        });
        out
    }

    /// [`unlock`](Self::unlock) with the caller's signal: TS has one
    /// `unlockDocument` taking it as an option, so the errors and the turn
    /// the promise settles on are the same.
    pub fn unlock_with_signal(&self, id: &str, password: &str, signal: AbortSignal) -> Promise<()> {
        match self
            .inner()
            .and_then(|inner| inner.unlock_document_sync(id, password.into(), Some(signal)))
        {
            Ok(promise) => promise,
            Err(error) => Promise::rejected(error),
        }
    }

    pub fn open(&self, input: OpenSource, options: OpenOptions) -> Promise<Value> {
        match self.inner() {
            Ok(inner) => document_errors(inner.open_document(input, options, None)),
            Err(error) => Promise::rejected(error),
        }
    }

    /// Re-run a failed open with the same source and options.
    pub fn retry(&self, id: &str) -> Promise<Value> {
        let inner = match self.inner() {
            Ok(inner) => inner,
            Err(error) => return Promise::rejected(error),
        };
        let id = id.to_string();
        Promise::spawn(async move {
            let session = inner.sessions.borrow().get(&id);
            let recorded = session.as_ref().and_then(|s| {
                (s.phase.get() == Phase::Error).then(|| s.source.borrow().clone()).flatten()
            });
            let (Some(session), Some((source, options, hold))) = (session, recorded) else {
                return Err(crate::error::plugin_error(
                    "invalid-input",
                    "documents",
                    format!("{id} didn't fail to open; nothing to retry"),
                    None,
                    None,
                ));
            };
            inner.sessions.borrow_mut().delete(&id);
            // Failure already disposed this session's scope. Keep the tab and
            // its position while replacing only the failed session.
            let _old = session;
            let mut options = options;
            options.activate = Some(false);
            let reopened = match inner.reserve_and_open_as(source, options, hold, Some(id)) {
                Ok((_, operation)) => adopt(operation),
                Err(error) => Promise::rejected(error),
            };
            // `return openDocument(...)` from an async function adopts it.
            rt::tick().await;
            reopened.reaction().await
        })
    }

    /// Change the tab name.
    pub fn rename(&self, id: &str, name: &str) {
        let Ok(inner) = self.inner() else { return };
        let core = inner.store.core();
        let (id, name) = (id.to_string(), name.to_string());
        if core.documents.contains_key(&id) {
            let (i, n) = (id.clone(), name.clone());
            inner.set_core(
                move |core| {
                    if let Some(meta) = core.documents.get_mut(&i) {
                        meta.name = Some(n);
                    }
                },
                &core_action(CORE_DOCUMENT_RENAMED),
            );
        } else if core.pending.contains_key(&id) {
            let (i, n) = (id.clone(), name.clone());
            inner.set_core(
                move |core| {
                    if let Some(meta) = core.pending.get_mut(&i) {
                        meta.name = Some(n);
                    }
                },
                &core_action(CORE_DOCUMENT_RENAMED),
            );
        }
        let session = inner.sessions.borrow().get(&id);
        if let Some(session) = session {
            *session.name.borrow_mut() = Some(name);
        }
    }

    /// Fire every open without awaiting; exactly one activation, decided
    /// now. The reserved ids, in order: a thunk's ticket, and for a refused
    /// reservation the id it asked for.
    pub fn open_all(&self, docs: Vec<InitialDocument>) -> Result<Vec<String>, Error> {
        let inner = self.inner()?;
        inner.guard_usable("documents.openAll()")?;
        let active_index = docs.iter().position(|d| d.active).unwrap_or(0);
        let mut ids = Vec::new();
        for (index, doc) in docs.into_iter().enumerate() {
            let mut options = doc.options;
            options.activate = Some(index == active_index);
            let asked = match &doc.source {
                OpenSource::Thunk(_) => String::new(),
                OpenSource::Input(value) => id_of_input(value).unwrap_or_default(),
            };
            match inner.reserve_and_open(doc.source, options, None) {
                Ok((session, done)) => {
                    done.ignore();
                    ids.push(session.id());
                }
                Err(_) => ids.push(asked),
            }
        }
        Ok(ids)
    }

    pub fn unlock(&self, id: &str, password: &str) -> Promise<()> {
        match self.inner() {
            Ok(inner) => inner.unlock_document(id, password.to_string()),
            Err(error) => Promise::rejected(error),
        }
    }

    pub fn close(&self, id: &str) -> Promise<()> {
        match self.inner() {
            Ok(inner) => inner.close_document(id),
            Err(error) => Promise::rejected(error),
        }
    }

    pub fn close_all(&self) -> Promise<()> {
        let Ok(inner) = self.inner() else { return Promise::resolved(()) };
        Promise::spawn(async move {
            let order = inner.store.core().order.clone();
            for id in order {
                inner.close_document(&id).await?;
            }
            Ok(())
        })
    }

    pub fn set_active(&self, id: &str) {
        let Ok(inner) = self.inner() else { return };
        let core = inner.store.core();
        if core.documents.contains_key(id) || core.pending.contains_key(id) {
            let id = id.to_string();
            inner.set_core(|core| core.active_id = Some(id), &core_action(CORE_ACTIVE_CHANGED));
        }
    }

    pub fn get_active_id(&self) -> Option<String> {
        self.inner().ok()?.store.core().active_id.clone()
    }

    pub fn get_active(&self) -> Option<DocInfo> {
        let id = self.get_active_id()?;
        self.get(&id)
    }

    pub fn list(&self) -> Vec<DocInfo> {
        let Ok(inner) = self.inner() else { return Vec::new() };
        let core = inner.store.core();
        core.order
            .iter()
            .filter_map(|id| match core.documents.get(id) {
                Some(meta) => Some(DocInfo::from_meta(meta)),
                None => core.pending.get(id).map(DocInfo::from_pending),
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<DocInfo> {
        let core = self.inner().ok()?.store.core();
        match core.documents.get(id) {
            Some(meta) => Some(DocInfo::from_meta(meta)),
            None => core.pending.get(id).map(DocInfo::from_pending),
        }
    }

    pub fn has(&self, id: &str) -> bool {
        self.inner().is_ok_and(|inner| {
            let core = inner.store.core();
            core.documents.contains_key(id) || core.pending.contains_key(id)
        })
    }

    pub fn get_count(&self) -> usize {
        self.inner().map(|inner| inner.store.core().order.len()).unwrap_or(0)
    }

    pub fn get_order(&self) -> Vec<String> {
        self.inner().map(|inner| inner.store.core().order.clone()).unwrap_or_default()
    }

    /// Reorder every tab at once; `ids` must be a permutation of the order.
    pub fn set_order(&self, ids: &[String]) -> Result<(), Error> {
        let inner = self.inner()?;
        let current = inner.store.core().order.clone();
        let mut a = ids.to_vec();
        let mut b = current.clone();
        a.sort();
        b.sort();
        if ids.len() != current.len() || a != b {
            return Err(crate::error::plugin_error(
                "invalid-input",
                "documents",
                "setOrder() needs a permutation of the current order",
                None,
                None,
            ));
        }
        inner.reorder(ids.to_vec());
        Ok(())
    }

    pub fn move_to(&self, id: &str, to_index: i64) {
        let Ok(inner) = self.inner() else { return };
        let core = inner.store.core();
        if !core.documents.contains_key(id) && !core.pending.contains_key(id) {
            return;
        }
        let mut without: Vec<String> = core.order.iter().filter(|x| *x != id).cloned().collect();
        let clamped = to_index.clamp(0, without.len() as i64) as usize;
        without.insert(clamped, id.to_string());
        inner.reorder(without);
    }

    pub fn swap(&self, a: &str, b: &str) {
        let Ok(inner) = self.inner() else { return };
        let core = inner.store.core();
        let (Some(index_a), Some(index_b)) =
            (core.order.iter().position(|x| x == a), core.order.iter().position(|x| x == b))
        else {
            return;
        };
        let mut next = core.order.clone();
        next[index_a] = b.to_string();
        next[index_b] = a.to_string();
        inner.reorder(next);
    }

    /// Download after deferred plugin work and held changes have settled.
    pub fn save(&self, id: Option<&str>, mode: Option<Value>) -> Promise<Value> {
        self.save_with_signal(id, mode, None)
    }
    pub fn save_with_signal(
        &self,
        id: Option<&str>,
        mode: Option<Value>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        match self.inner() {
            Ok(inner) => inner.download_file(id, mode, false, signal),
            Err(e) => Promise::rejected(to_plugin_error("documents", &e)),
        }
    }
    pub fn save_layer(&self, id: Option<&str>) -> Promise<Value> {
        self.save_layer_with_signal(id, None)
    }
    pub fn save_layer_with_signal(
        &self,
        id: Option<&str>,
        signal: Option<AbortSignal>,
    ) -> Promise<Value> {
        match self.inner() {
            Ok(inner) => inner.download_file(id, None, true, signal),
            Err(e) => Promise::rejected(to_plugin_error("documents", &e)),
        }
    }

    fn meta_of(&self, id: Option<&str>) -> Option<DocumentMeta> {
        let core = self.inner().ok()?.store.core();
        let id = match id {
            Some(id) => id.to_string(),
            None => core.active_id.clone()?,
        };
        core.documents.get(&id).cloned()
    }

    fn pages_of(&self, id: Option<&str>) -> Option<Vec<Value>> {
        self.meta_of(id).map(|meta| meta.pages.as_array().cloned().unwrap_or_default())
    }

    /// The page registry of a document, the active one by default.
    pub fn list_pages(&self, id: Option<&str>) -> Vec<Value> {
        self.pages_of(id).unwrap_or_default()
    }

    pub fn get_page(&self, page: PageRef, id: Option<&str>) -> Option<Value> {
        self.pages_of(id)?.into_iter().find(|p| page.names(p))
    }

    pub fn get_page_at(&self, index: i64, id: Option<&str>) -> Option<Value> {
        let pages = self.pages_of(id)?;
        usize::try_from(index).ok().and_then(|i| pages.get(i).cloned())
    }

    pub fn get_page_index(&self, page: PageRef, id: Option<&str>) -> i64 {
        match self.pages_of(id) {
            Some(pages) => pages.iter().position(|p| page.names(p)).map_or(-1, |i| i as i64),
            None => -1,
        }
    }

    pub fn get_revision(&self, id: Option<&str>) -> i64 {
        self.meta_of(id).map_or(-1, |meta| meta.revision as i64)
    }

    /// The permissions.md chrome exception: print and download.
    pub fn allows(&self, capability: &str, id: Option<&str>) -> bool {
        self.inner()
            .ok()
            .and_then(|inner| inner.document_handle_of(id))
            .is_some_and(|handle| handle.security().allows(capability))
    }

    /// The lifecycle events: `onOpened`, `onOpenFailed`, `onLocked`,
    /// `onClosed`, `onActiveChanged`, `onPagesChanged`. Payloads are the
    /// TypeScript's, as JSON.
    pub fn events(&self) -> Option<LifecycleEvents> {
        self.inner().ok().map(|inner| inner.events.clone())
    }
}
