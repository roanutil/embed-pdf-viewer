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

#[derive(Default)]
pub struct KernelOptions {
    pub report: Option<Report>,
    /// Observe every plugin dispatch, with its slice key, before the reducer
    /// runs. For devtools and the conformance harness.
    pub on_dispatch: Option<Rc<dyn Fn(&str, &dyn Action)>>,
    /// Once a session finishes closing, drop its capabilities and connectors
    /// so nothing keeps their context (and the session) alive past the
    /// close. Off by default: native callers rely on a closing session's
    /// Weak-guarded deferred work going quiet on its own, not on the lists
    /// being emptied. The web build turns this on.
    pub release_closed_sessions: bool,
}

// ── sessions ──────────────────────────────────────────────────────────────────

struct Session {
    id: RefCell<String>,
    instance_id: String,
    name: RefCell<Option<String>>,
    /// What `open()` was called with, so a failed tab can `retry()`.
    source: RefCell<Option<(OpenSource, OpenOptions)>>,
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
    this: Weak<Inner>,
    engine: Rc<dyn Engine>,
    store: Rc<Store>,
    plan: PluginPlan,
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
        "pages.moved" | "pages.rotated" | "pages.deleted" | "pages.inserted" => event.get("layout"),
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
        Rc::new(Session {
            instance_id: format!("{id}#{}", self.instance_counter.get()),
            id: RefCell::new(id),
            name: RefCell::new(name),
            source: RefCell::new(None),
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
        })
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
                if let Ok(value) = call.await
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
            // runs one turn after `run` settles, before the returned promise does.
            rt::tick().await;
            let current = s.operation.borrow().clone();
            if let Some(current) = current
                && current.same(&op)
            {
                *s.operation.borrow_mut() = None;
            }
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
            return Err(Error::new(format!("[documents] duplicate document id: {next_id}")));
        }
        self.sessions.borrow_mut().delete(&previous_id);
        *session.id.borrow_mut() = next_id.to_string();
        self.sessions.borrow_mut().set(next_id.to_string(), session.clone());
        if let Some(slot) = core.pending.get(&previous_id).cloned() {
            let next = next_id.to_string();
            self.set_core(
                |core| {
                    core.pending.remove(&previous_id);
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
            s.cancel.abort(reason.clone());
            let engine_op = s.engine_op.borrow().clone();
            if let Some(abort) = engine_op {
                abort(Some(reason.to_canonical()));
            }
            let operation = s.operation.borrow().clone();
            if let Some(operation) = operation {
                if let Err(error) = operation.await
                    && !is_cancelled(&error)
                    && !is_abort_like(&error)
                {
                    (this.report)(&error);
                }
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
            // Each capability's context holds this session, so while the
            // session holds the capabilities (and their connects) the two
            // keep each other alive past the close, and past the kernel.
            // Nothing resolves a closing session's capabilities again, so
            // dropping them here is safe; native callers still rely on
            // Weak-guarded deferred work going quiet on its own, so this
            // only runs when the caller opts in.
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
                core.order.push(id.clone());
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
                core.pending.remove(id);
                core.documents.remove(id);
                core.order.retain(|other| other != id);
                core.active_id = next_active;
            },
            &core_action(CORE_DOCUMENT_REMOVED),
        );
    }

    fn commit_ready(&self, session: &Session) {
        session.phase.set(Phase::Ready);
        let meta = session.staged_meta().expect("staged meta");
        let id = session.id();
        self.set_core(
            |core| {
                core.documents.insert(id.clone(), meta);
                core.pending.remove(&id);
            },
            &core_action(CORE_DOCUMENT_ADDED),
        );
        let meta = self.store.core().documents.get(&id).cloned().expect("committed");
        self.events.opened.emit(&serde_json::json!({
            "documentId": id,
            "info": serde_json::to_value(DocInfo::from_meta(&meta)).unwrap_or(Value::Null),
        }));
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
                let created = create(cx)?;
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
            // `(await handle.render?.policy()) ?? CONTINUOUS_RENDER_POLICY`, best-effort
            if let Ok(policy) = handle.call("render.policy", None, Vec::new()).await
                && !policy.is_null()
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
            // `snapshot.pages`: absent stays undefined, as the TypeScript copies it.
            pages: snapshot.get("pages").cloned().unwrap_or_else(crate::undefined),
            revision: 0,
            render_policy,
        });

        let weak = Rc::downgrade(&this);
        let s = Rc::downgrade(&session);
        let unsubscribe = handle.subscribe(Rc::new(move |event: &Value| {
            let (Some(this), Some(session)) = (weak.upgrade(), s.upgrade()) else { return };
            let Some(layout) = layout_from_event(event) else { return };
            let id = session.id();
            let core = this.store.core();
            let Some(existing) = core.documents.get(&id).cloned() else { return };
            let updated = DocumentMeta {
                page_count: layout.get("pageCount").and_then(Value::as_u64).unwrap_or(0) as u32,
                pages: layout.get("pages").cloned().unwrap_or_else(crate::undefined),
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
        }));
        session.scope.defer_fn(move || unsubscribe.call());

        for plugin in &this.document_plugins {
            // The lease is the write authority for this instance's slice.
            let lease = this.store.lease(
                &slice_key(&plugin.id, Some(&session.id())),
                plugin.reducer.clone(),
                (plugin.initial_state)(),
                plugin.to_json.clone(),
                Some(session.instance_id.clone()),
            );
            session.leases.borrow_mut().push((Rc::as_ptr(plugin), lease.clone()));
            session.scope.defer_fn(move || lease.revoke()); // LIFO: reverse dependency order
        }
        for plugin in &this.document_plugins {
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
            }
        }
        Self::checkpoint(&session)
    }

    fn open_document(&self, input: OpenSource, options: OpenOptions) -> Promise<Value> {
        // An async function: its synchronous throws are rejections.
        match self.reserve_and_open(input, options) {
            Ok((_, promise)) => promise,
            Err(error) => Promise::rejected(error),
        }
    }

    /// Reserve the tab slot synchronously and start the open; the slot id at
    /// once, and the open's promise.
    fn reserve_and_open(
        &self,
        input: OpenSource,
        options: OpenOptions,
    ) -> Result<(String, Promise<Value>), Error> {
        self.guard_usable("documents.open()")?;
        let recorded = (input.clone(), options.clone());
        let OpenOptions { activate, name, engine: engine_options } = options;
        let requested_id = match &input {
            OpenSource::Thunk(_) => self.next_ticket(),
            OpenSource::Input(value) => id_of_input(value).unwrap_or_else(|| self.next_ticket()),
        };
        if self.sessions.borrow().has(&requested_id) {
            return Err(Error::new(format!("[documents] document already open: {requested_id}")));
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
                Ok(id) => Ok(Value::String(id)),
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
        let id = session.id();
        Ok((id, self.begin_operation(&session, run)?))
    }

    async fn open_steps(
        this: Rc<Inner>,
        session: Rc<Session>,
        input: OpenSource,
        engine_options: serde_json::Map<String, Value>,
    ) -> Result<String, Error> {
        let source = match input {
            OpenSource::Thunk(thunk) => {
                let produced = match thunk(session.cancel.clone()) {
                    Ok(promise) => promise,
                    Err(error) => Promise::rejected(error),
                };
                rt::returned(Self::engine_call(&session, produced, None)).await?
            }
            OpenSource::Input(value) => value,
        };
        Self::checkpoint(&session)?;
        if let Some(source_id) = id_of_input(&source) {
            this.rekey_session(&session, &source_id)?;
        }

        let on_late: Rc<dyn Fn(Rc<dyn DocumentHandle>) -> Result<Option<Promise<()>>, Error>> =
            Rc::new(|late: Rc<dyn DocumentHandle>| {
                Ok(Some(late.call("close", None, Vec::new()).map(|_| Ok(()))))
            });
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
            session.scope.defer(Box::new(move || {
                Ok(Some(handle.call("close", None, Vec::new()).map(|_| Ok(()))))
            }));
        }
        Self::checkpoint(&session)?;
        if handle.id() != session.id() {
            this.rekey_session(&session, &handle.id())?;
        }

        let prompt = handle.security().password_prompt();
        if prompt.get("state").and_then(Value::as_str) == Some("required") {
            let password_provided = engine_options.get("password").is_some_and(|p| !p.is_null())
                || source.get("password").is_some_and(|p| !p.is_null());
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
        match self.unlock_document_sync(id, password) {
            Ok(promise) => promise,
            Err(error) => Promise::rejected(error),
        }
    }

    fn unlock_document_sync(&self, id: &str, password: String) -> Result<Promise<()>, Error> {
        self.guard_usable("documents.unlock()")?;
        let session = self.sessions.borrow().get(id);
        let session = match session {
            Some(s) if s.phase.get() == Phase::Locked && s.handle().is_some() => s,
            _ => return Err(Error::new(format!("[documents] document is not locked: {id}"))),
        };
        let handle = session.handle().expect("handle");
        let this = self.this.upgrade().expect("kernel");
        let s = session.clone();
        let run = async move {
            let unlock = handle.security().unlock(serde_json::json!({ "password": password }));
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
        Ok(operation.map(|_| Ok(())))
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
    /// `createKernel`. Fails where the TypeScript throws: a bad plugin list,
    /// or a workspace capability whose constructor throws.
    pub fn new(
        engine: Rc<dyn Engine>,
        plugins: Vec<AnyPlugin>,
        options: KernelOptions,
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
        let release_closed_sessions = options.release_closed_sessions;
        let inner = Rc::new_cyclic(|this| Inner {
            this: this.clone(),
            engine,
            store,
            plan,
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
            if plugin.scope != PluginScope::Document {
                let lease = inner.store.lease(
                    &plugin.id,
                    plugin.reducer.clone(),
                    (plugin.initial_state)(),
                    plugin.to_json.clone(),
                    None,
                );
                inner.workspace_leases.borrow_mut().push((Rc::as_ptr(plugin), lease));
            }
        }
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
                    if let Err(error) = start.await
                        && !is_cancelled(&error)
                        && !was_failed
                    {
                        (this.report)(&error);
                    }
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

impl Documents {
    fn inner(&self) -> Result<Rc<Inner>, Error> {
        self.0.upgrade().ok_or_else(|| Error::new("[kernel] used after the kernel was dropped"))
    }

    pub fn open(&self, input: OpenSource, options: OpenOptions) -> Promise<Value> {
        match self.inner() {
            Ok(inner) => inner.open_document(input, options),
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
            let (Some(session), Some((source, options))) = (session, recorded) else {
                return Err(Error::new(format!(
                    "[documents] \"{id}\" is not a failed open; nothing to retry"
                )));
            };
            inner.close_session(&session).await?;
            let reopened = inner.open_document(source, options);
            // `return openDocument(...)` from an async function adopts it.
            rt::tick().await;
            reopened.await
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
            match inner.reserve_and_open(doc.source, options) {
                Ok((id, done)) => {
                    done.ignore();
                    ids.push(id);
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
            return Err(Error::new(
                "[documents] setOrder() needs a permutation of the current order",
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

    /// The complete document as PDF bytes. `mode` goes to the engine only
    /// when one is given.
    pub fn save(&self, id: Option<&str>, mode: Option<Value>) -> Promise<Value> {
        let Ok(inner) = self.inner() else {
            return Promise::rejected(Error::new("[documents] no document to save"));
        };
        match inner.document_handle_of(id) {
            None => Promise::rejected(Error::new("[documents] no document to save")),
            Some(handle) => {
                let options = match mode {
                    Some(mode) => serde_json::json!({ "mode": mode }),
                    None => crate::undefined(),
                };
                handle.call("download", None, vec![options])
            }
        }
    }

    /// Just the document's layer.
    pub fn save_layer(&self, id: Option<&str>) -> Promise<Value> {
        let Ok(inner) = self.inner() else {
            return Promise::rejected(Error::new("[documents] no document to download"));
        };
        match inner.document_handle_of(id) {
            None => Promise::rejected(Error::new("[documents] no document to download")),
            Some(handle) if !handle.has("downloadLayer") => Promise::rejected(Error::new(
                "[documents] this engine cannot export a layer (open with a layer on the local engine)",
            )),
            Some(handle) => handle.call("downloadLayer", None, Vec::new()),
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
