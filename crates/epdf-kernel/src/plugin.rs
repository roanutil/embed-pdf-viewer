//! `PluginDef`, the contexts a plugin receives, and the typed layer plugin
//! crates write against.
//!
//! The kernel works with the erased [`PluginDef`]: state is `Rc<dyn Any>`,
//! actions are `&dyn Action`, capabilities are `Rc<dyn Any>`. A plugin crate
//! uses [`define`] and gets [`Ctx`] and [`EffectCtx`], typed over its own
//! state and action, with the downcasts done once, here.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use serde::Serialize;
use serde_json::Value;

use crate::action::Action;
use crate::engine::{DocumentHandle, Engine};
use crate::error::{Error, code, plugin_error};
use crate::event_hook::EventHookSource;
use crate::geometry::{PageRef, PageSpace, PdfEdges};
use crate::guarded::{GuardedLifetime, guard_handle};
use crate::lanes::LatestLane;
use crate::rt::{self, Promise};
use crate::scope::{Scope, Teardown};
use crate::serial_queue::SerialQueue;
use crate::signal::AbortSignal;
use crate::store::{
    Report, SliceJson, SliceLease, SliceReducer, SliceState, SliceText, Store, Unsubscribe,
};
use crate::token::{AnyToken, Token};
use crate::types::{CoreState, DocumentMeta, PluginScope};

pub type CapabilityFn = Rc<dyn Fn(PluginContext) -> Result<Rc<dyn Any>, Error>>;
/// `init`: `Ok(None)` is a synchronous init (still one turn, as `await
/// undefined` is), `Ok(Some)` one to await, `Err` a synchronous throw.
pub type InitFn = Rc<dyn Fn(PluginContext) -> Result<Option<Promise<()>>, Error>>;
pub type EffectsFn = Rc<dyn Fn(EffectContext) -> Result<(), Error>>;
/// A `connect` half: runs once every dependency is constructed. A throw
/// fails the open (document) or the start (workspace).
pub type ConnectFn = Rc<dyn Fn() -> Result<(), Error>>;
/// What `create` returns: the instance's api and, optionally, its connect.
pub struct Created {
    pub api: Rc<dyn Any>,
    pub connect: Option<ConnectFn>,
}
/// `create`: the controller hook, run once per instance.
pub type CreateFn = Rc<dyn Fn(ControllerContext) -> Result<Created, Error>>;

pub struct PluginDef {
    pub settings: Option<crate::settings::SettingsDeclaration>,
    pub id: String,
    pub token: Option<AnyToken>,
    pub scope: PluginScope,
    pub requires: Vec<AnyToken>,
    pub optional: Vec<AnyToken>,
    pub initial_state: Rc<dyn Fn() -> SliceState>,
    pub reducer: SliceReducer,
    pub to_json: SliceJson,
    /// The slice's text written directly (`PluginBuilder::slice_text`).
    pub to_text: Option<SliceText>,
    pub capability: Option<CapabilityFn>,
    pub init: Option<InitFn>,
    pub effects: Option<EffectsFn>,
    /// The v3 controller hook. A plugin declares either `create` or
    /// `capability` and `effects`.
    pub create: Option<CreateFn>,
}

pub type AnyPlugin = Rc<PluginDef>;

// ── what a context needs from the kernel ──────────────────────────────────────

pub(crate) trait Services {
    fn clock(&self) -> Rc<dyn crate::clock::HostClock>;
    fn settings(&self, plugin_id: &str) -> Option<crate::settings::Settings>;
    fn engine(&self) -> Rc<dyn Engine>;
    fn store(&self) -> Rc<Store>;
    fn workspace_scope(&self) -> Scope;
    fn workspace_lease(&self, plugin: &PluginDef) -> Option<SliceLease>;
    fn report(&self) -> Report;
    fn resolve(&self, token: &AnyToken, document_id: Option<&str>) -> Result<Rc<dyn Any>, Error>;
    fn try_resolve_internal(
        &self,
        token: &AnyToken,
        document_id: Option<&str>,
    ) -> Result<Option<Rc<dyn Any>>, Error>;
    fn document_handle(&self, document_id: Option<&str>) -> Option<Rc<dyn DocumentHandle>>;
    fn observe_dispatch(&self, slice: &str, action: &dyn Action);
}

/// The slice of a document session a context needs.
pub(crate) trait SessionView {
    fn changes(&self) -> crate::changes::DocumentChanges;
    fn events(&self) -> EventHookSource<Value>;
    fn add_mirror(&self, plugin_id: &str, start: Rc<dyn Fn()>);
    fn id(&self) -> String;
    /// Unique per open; a reopened id gets a new one.
    fn instance_id(&self) -> String;
    fn handle(&self) -> Option<Rc<dyn DocumentHandle>>;
    fn staged_meta(&self) -> Option<DocumentMeta>;
    fn scope(&self) -> Scope;
    fn lease(&self, plugin: &PluginDef) -> Option<SliceLease>;
}

pub fn slice_key(plugin_id: &str, document_id: Option<&str>) -> String {
    match document_id {
        Some(document_id) => format!("{plugin_id}::{document_id}"),
        None => plugin_id.to_string(),
    }
}

/// `PluginContext`. Cheap to clone. The document id and handle are
/// snapshots taken when the kernel built the context, as in TypeScript: a
/// workspace capability built before any document opened sees no handle.
///
/// State goes through the instance's [`SliceLease`]: once the instance
/// closes, a dispatch is dropped (and reported, once per context) and a read
/// answers the last state.
#[derive(Clone)]
pub struct PluginContext {
    services: Weak<dyn Services>,
    plugin_id: Rc<str>,
    key: Rc<str>,
    document_id: Option<Rc<str>>,
    doc: Option<Rc<dyn DocumentHandle>>,
    session: Option<Rc<dyn SessionView>>,
    lease: SliceLease,
    declared: Rc<Vec<AnyToken>>,
    reported_closed: Rc<Cell<bool>>,
}

fn gone() -> Error {
    Error::new("[kernel] used after the kernel was dropped")
}

/// The tokens a plugin may resolve: its `requires` and `optional`, its own
/// token, and the documents token.
fn declared_tokens(plugin: &PluginDef) -> Vec<AnyToken> {
    let mut declared = vec![crate::kernel::DOCUMENTS.any().clone()];
    declared.extend(plugin.requires.iter().cloned());
    declared.extend(plugin.optional.iter().cloned());
    if let Some(token) = &plugin.token {
        declared.push(token.clone());
    }
    declared
}

impl PluginContext {
    /// `createPluginContext`. Fails when the instance holds no lease yet:
    /// a document capability resolved before bring-up acquired its slices.
    pub(crate) fn new(
        services: Weak<dyn Services>,
        plugin: &PluginDef,
        session: Option<Rc<dyn SessionView>>,
    ) -> Result<Self, Error> {
        let document_id = session.as_ref().map(|s| Rc::<str>::from(s.id()));
        let live = services.upgrade().ok_or_else(gone)?;
        let lease = match &session {
            Some(session) => session.lease(plugin),
            None => live.workspace_lease(plugin),
        };
        let Some(lease) = lease else {
            return Err(Error::new(format!(
                "[kernel] no state lease for plugin \"{}\" ({})",
                plugin.id,
                document_id.as_deref().unwrap_or("workspace")
            )));
        };
        let doc = match &session {
            Some(session) => session.handle(),
            None => live.document_handle(None),
        };
        Ok(PluginContext {
            key: slice_key(&plugin.id, document_id.as_deref()).into(),
            plugin_id: plugin.id.as_str().into(),
            services,
            document_id,
            doc,
            session,
            lease,
            declared: Rc::new(declared_tokens(plugin)),
            reported_closed: Rc::new(Cell::new(false)),
        })
    }

    /// The development guard: resolving a token the plugin did not declare
    /// is refused. The Rust kernel always runs it, as the vectors were
    /// generated in development.
    fn guard(&self, token: &AnyToken) -> Result<(), Error> {
        if self.declared.contains(token) {
            return Ok(());
        }
        Err(Error::new(format!(
            "[kernel] plugin \"{}\" resolved capability \"{}\" without declaring it; \
             add the token to its `requires` or `optional` list.",
            self.plugin_id,
            token.name()
        )))
    }

    fn services(&self) -> Option<Rc<dyn Services>> {
        self.services.upgrade()
    }

    pub fn id(&self) -> &str {
        &self.plugin_id
    }

    pub fn slice_key(&self) -> &str {
        &self.key
    }

    pub fn engine(&self) -> Option<Rc<dyn Engine>> {
        self.services().map(|s| s.engine())
    }

    /// The bound document (document-scoped plugins only).
    pub fn document_id(&self) -> Option<&str> {
        self.document_id.as_deref()
    }

    /// `ctx.doc`.
    pub fn doc(&self) -> Option<Rc<dyn DocumentHandle>> {
        self.doc.clone()
    }

    /// `ctx.documentHandle(id?)`.
    pub fn document_handle(&self, document_id: Option<&str>) -> Option<Rc<dyn DocumentHandle>> {
        let services = self.services()?;
        match (document_id, &self.session) {
            (Some(id), _) => services.document_handle(Some(id)),
            (None, Some(session)) => session.handle(),
            (None, None) => services.document_handle(None),
        }
    }

    /// `ctx.getState()`, erased: the lease's state, the last one once the
    /// instance closed. Always `Some`; the option is for the typed layer.
    pub fn state_any(&self) -> Option<SliceState> {
        Some(self.lease.read())
    }

    /// The instance's lease.
    pub fn lease(&self) -> &SliceLease {
        &self.lease
    }

    pub fn dispatch_any(&self, action: &dyn Action) {
        let Some(services) = self.services() else { return };
        services.observe_dispatch(&self.key, action);
        if self.lease.commit(action) || self.reported_closed.get() {
            return;
        }
        // Reported once per context: a retained callback dispatching into a
        // closed instance is a bug worth surfacing, not a storm worth logging.
        self.reported_closed.set(true);
        (services.report())(&plugin_error(
            code::INSTANCE_CLOSED,
            &self.plugin_id,
            format!(
                "dispatch of \"{}\" after the instance closed ({}); dropped",
                action.action_type(),
                self.lease.instance_id()
            ),
            None,
            None,
        ));
    }

    pub fn subscribe(&self, listener: Rc<crate::store::ChangeListener>) -> Unsubscribe {
        match self.services() {
            Some(services) => services.store().subscribe(listener),
            None => Unsubscribe::noop(),
        }
    }

    pub fn core(&self) -> Rc<CoreState> {
        match self.services() {
            Some(services) => services.store().core(),
            None => Rc::new(CoreState::default()),
        }
    }

    /// `ctx.document()`: the bound document's meta (staged during bring-up),
    /// or for a workspace plugin the active document's.
    pub fn document(&self) -> Option<DocumentMeta> {
        let core = self.core();
        match &self.session {
            Some(session) => {
                core.documents.get(&session.id()).cloned().or_else(|| session.staged_meta())
            }
            None => core.active_id.as_ref().and_then(|id| core.documents.get(id).cloned()),
        }
    }

    pub fn get_any(&self, token: &AnyToken) -> Result<Rc<dyn Any>, Error> {
        self.guard(token)?;
        self.services().ok_or_else(gone)?.resolve(token, self.document_id())
    }

    pub fn for_document_any(
        &self,
        token: &AnyToken,
        document_id: &str,
    ) -> Result<Rc<dyn Any>, Error> {
        self.guard(token)?;
        self.services().ok_or_else(gone)?.resolve(token, Some(document_id))
    }

    pub fn try_get_any(&self, token: &AnyToken) -> Result<Option<Rc<dyn Any>>, Error> {
        self.guard(token)?;
        match self.services() {
            Some(services) => services.try_resolve_internal(token, self.document_id()),
            None => Ok(None),
        }
    }

    /// `ctx.tryForDocument`: `forDocument` for an optional dependency.
    pub fn try_for_document_any(
        &self,
        token: &AnyToken,
        document_id: &str,
    ) -> Result<Option<Rc<dyn Any>>, Error> {
        self.guard(token)?;
        match self.services() {
            Some(services) => services.try_resolve_internal(token, Some(document_id)),
            None => Ok(None),
        }
    }

    pub fn try_for_document<C: Clone + 'static>(
        &self,
        token: &Token<C>,
        document_id: &str,
    ) -> Result<Option<C>, Error> {
        match self.try_for_document_any(token.any(), document_id)? {
            Some(any) => downcast(any, token.name()).map(Some),
            None => Ok(None),
        }
    }

    pub fn get<C: Clone + 'static>(&self, token: &Token<C>) -> Result<C, Error> {
        downcast(self.get_any(token.any())?, token.name())
    }

    pub fn for_document<C: Clone + 'static>(
        &self,
        token: &Token<C>,
        document_id: &str,
    ) -> Result<C, Error> {
        downcast(self.for_document_any(token.any(), document_id)?, token.name())
    }

    /// `ctx.tryGet`: `None` for no provider or no bring-up/ready document. A
    /// capability constructor that throws is a bug and is returned as `Err`.
    pub fn try_get<C: Clone + 'static>(&self, token: &Token<C>) -> Result<Option<C>, Error> {
        match self.try_get_any(token.any())? {
            Some(any) => downcast(any, token.name()).map(Some),
            None => Ok(None),
        }
    }

    /// `ctx.cleanup(fn)`: document-scoped teardowns run when the document
    /// closes, workspace ones when the kernel is destroyed.
    pub fn cleanup(&self, teardown: Teardown) {
        let scope = match &self.session {
            Some(session) => Some(session.scope()),
            None => self.services().map(|s| s.workspace_scope()),
        };
        if let Some(scope) = scope {
            scope.defer(teardown);
        }
    }

    pub fn cleanup_fn(&self, teardown: impl FnOnce() + 'static) {
        self.cleanup(Box::new(move || {
            teardown();
            Ok(None)
        }));
    }

    fn own_scope(&self) -> Option<Scope> {
        match &self.session {
            Some(session) => Some(session.scope()),
            None => self.services().map(|s| s.workspace_scope()),
        }
    }
}

pub fn downcast<C: Clone + 'static>(any: Rc<dyn Any>, name: &str) -> Result<C, Error> {
    any.downcast_ref::<C>().cloned().ok_or_else(|| {
        Error::new(format!("[kernel] capability \"{name}\" is not the type its token names"))
    })
}

/// `EffectContext`: a plugin context plus `watch` and `onAction`, whose
/// subscriptions the owning scope releases.
#[derive(Clone)]
pub struct EffectContext {
    pub ctx: PluginContext,
}

impl std::ops::Deref for EffectContext {
    type Target = PluginContext;
    fn deref(&self) -> &PluginContext {
        &self.ctx
    }
}

impl EffectContext {
    /// `ctx.watch(select, handler, isEqual)`: `handler(next, previous)` after
    /// any state change that moves `select()` by `is_equal`. The TypeScript
    /// default is `Object.is`; a port picks the comparison that fires on the
    /// same changes (value equality for primitives, `Rc::ptr_eq` for a
    /// state object the reducer replaces).
    pub fn watch<R: Clone + 'static>(
        &self,
        select: impl Fn() -> R + 'static,
        is_equal: impl Fn(&R, &R) -> bool + 'static,
        handler: impl Fn(&R, &R) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let previous = std::cell::RefCell::new(select());
        let unsubscribe = self.ctx.subscribe(Rc::new(move || {
            let next = select();
            let prior = previous.borrow().clone();
            if !is_equal(&prior, &next) {
                *previous.borrow_mut() = next.clone();
                handler(&next, &prior)?;
            }
            Ok(())
        }));
        if let Some(scope) = self.ctx.own_scope() {
            let release = unsubscribe.clone();
            scope.defer_fn(move || release.call());
        }
        unsubscribe
    }

    /// `ctx.onAction(type, handler)`.
    pub fn on_action(
        &self,
        action_type: &str,
        handler: impl Fn(&dyn Action) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let unsubscribe = match self.ctx.services() {
            Some(services) => services.store().subscribe_action_of(action_type, Rc::new(handler)),
            None => Unsubscribe::noop(),
        };
        if let Some(scope) = self.ctx.own_scope() {
            let release = unsubscribe.clone();
            scope.defer_fn(move || release.call());
        }
        unsubscribe
    }
}

// ── the controller context (`controller.ts`) ─────────────────────────────────

/// The context a plugin's `create` receives: the plugin context plus the
/// members the v3 authoring pattern needs. Everything here is bound to one
/// instance's lifetime, so a controller written as plain async code is
/// lifetime-safe by construction. Cheap to clone.
#[derive(Clone)]
pub struct ControllerContext(Rc<ControllerInner>);

struct ControllerInner {
    ctx: PluginContext,
    services: Weak<dyn Services>,
    instance_id: String,
    capability: String,
    lifetime: AbortSignal,
    scope: Scope,
    session: Option<Rc<dyn SessionView>>,
    guarded: RefCell<Option<Rc<dyn DocumentHandle>>>,
    queues: RefCell<HashMap<String, SerialQueue>>,
    lanes: RefCell<HashMap<String, LatestLane>>,
    clock: crate::clock::PluginClock,
}

impl std::ops::Deref for ControllerContext {
    type Target = PluginContext;
    fn deref(&self) -> &PluginContext {
        &self.0.ctx
    }
}

impl ControllerContext {
    /// `createControllerContext(services, plugin, session, lifetime, scope)`.
    pub(crate) fn new(
        services: Weak<dyn Services>,
        plugin: &PluginDef,
        session: Option<Rc<dyn SessionView>>,
        lifetime: AbortSignal,
        scope: Scope,
    ) -> Result<Self, Error> {
        let ctx = PluginContext::new(services.clone(), plugin, session.clone())?;
        let instance_id = match &session {
            Some(session) => session.instance_id(),
            None => format!("workspace:{}", plugin.id),
        };
        Ok(ControllerContext(Rc::new(ControllerInner {
            ctx,
            services: services.clone(),
            instance_id,
            capability: plugin.id.clone(),
            clock: crate::clock::PluginClock::new(
                services.upgrade().ok_or_else(gone)?.clock(),
                lifetime.clone(),
            ),
            lifetime,
            scope,
            session,
            guarded: RefCell::new(None),
            queues: RefCell::new(HashMap::new()),
            lanes: RefCell::new(HashMap::new()),
        })))
    }

    pub fn plugin_context(&self) -> &PluginContext {
        &self.0.ctx
    }

    /// Unique per open of this document; `workspace:<id>` for a workspace plugin.
    pub fn instance_id(&self) -> &str {
        &self.0.instance_id
    }

    /// The instance lifetime: aborted when the document closes, or for a
    /// workspace plugin when the kernel is destroyed.
    pub fn lifetime(&self) -> &AbortSignal {
        &self.0.lifetime
    }

    fn report(&self) -> Report {
        match self.0.services.upgrade() {
            Some(services) => services.report(),
            None => Rc::new(|_: &Error| {}),
        }
    }

    /// `ctx.doc`: the guarded handle. A workspace plugin has no bound
    /// document; reading it there is an error, as is a session without a
    /// handle yet.
    pub fn doc(&self) -> Result<Rc<dyn DocumentHandle>, Error> {
        let Some(session) = &self.0.session else {
            return Err(Error::new(format!(
                "[kernel] workspace plugin \"{}\" has no bound document; use ctx.forDocument(token, documentId).",
                self.0.capability
            )));
        };
        let Some(handle) = session.handle() else {
            return Err(plugin_error(
                code::NOT_READY,
                &self.0.capability,
                format!("document {} has no engine handle yet", session.id()),
                None,
                None,
            ));
        };
        let mut guarded = self.0.guarded.borrow_mut();
        Ok(guarded
            .get_or_insert_with(|| {
                guard_handle(
                    handle,
                    GuardedLifetime {
                        signal: self.0.lifetime.clone(),
                        instance_id: self.0.instance_id.clone(),
                    },
                    &self.0.capability,
                )
            })
            .clone())
    }

    pub fn clock(&self) -> crate::clock::PluginClock {
        self.0.clock.clone()
    }

    pub fn allows(&self, permission: &str) -> bool {
        self.doc().is_ok_and(|d| {
            if permission == "annotations:create" {
                d.security().allows_annotation_create()
            } else {
                d.security().allows(permission)
            }
        })
    }
    pub fn assert_allowed(&self, permission: &str, operation: &str) -> Result<(), Error> {
        if self.allows(permission) {
            Ok(())
        } else {
            Err(plugin_error(
                "permission-denied",
                &self.0.capability,
                format!("{operation} requires {permission}"),
                None,
                None,
            )
            .with("permission", permission))
        }
    }
    pub fn allows_field(&self, action: &str, field: &Value) -> bool {
        self.doc().is_ok_and(|d| {
            d.security().allows_field(action, &serde_json::json!({"groupId":field["groupId"]}))
        })
    }
    pub fn assert_allowed_field(
        &self,
        action: &str,
        field: &Value,
        operation: &str,
    ) -> Result<(), Error> {
        if self.allows_field(action, field) {
            return Ok(());
        }
        let permission = if let Some(group) = field["groupId"].as_str() {
            format!("fields:{action}:group={group}")
        } else if action == "fill" {
            "doc.forms.fill".into()
        } else {
            "doc.sign".into()
        };
        Err(plugin_error(
            "permission-denied",
            &self.0.capability,
            format!("{operation} requires {permission}"),
            None,
            None,
        )
        .with("permission", permission))
    }

    pub fn notify(&self) {
        if self.0.ctx.lease.live() {
            if let Some(s) = self.0.services.upgrade() {
                s.store().notify();
            }
        }
    }

    pub fn settings(&self) -> Result<crate::settings::InstanceSettings, Error> {
        self.0
            .services
            .upgrade()
            .and_then(|s| s.settings(&self.0.capability))
            .map(|s| s.for_instance(&self.0.scope))
            .ok_or_else(|| {
                Error::new(format!(
                    "[kernel] plugin \"{}\" declares no settings",
                    self.0.capability
                ))
            })
    }

    pub fn changes(&self) -> Result<crate::changes::ChangeQueue, Error> {
        self.0
            .session
            .as_ref()
            .map(|s| s.changes().for_plugin(&self.0.capability))
            .ok_or_else(|| Error::new("[kernel] workspace plugin has no document changes"))
    }

    fn mirror_environment(&self) -> Result<crate::mirror::MirrorEnvironment, Error> {
        let doc = self.doc()?;
        let session = self.0.session.as_ref().ok_or_else(gone)?;
        let services = self.0.services.upgrade().ok_or_else(gone)?;
        let store = services.store();
        let prefix = self.0.ctx.slice_key().to_string();
        let scope = self.0.scope.clone();
        let lifetime = self.0.lifetime.clone();
        let instance = self.0.instance_id.clone();
        let notify = self.clone();
        Ok(crate::mirror::MirrorEnvironment {
            doc: Some(doc),
            lifetime: lifetime.clone(),
            changes: session.changes(),
            events: session.events(),
            cell: Rc::new(move |name, initial| {
                let lease = store.lease(
                    &format!("{prefix}/{name}"),
                    Rc::new(|_, _| None),
                    Rc::new(initial),
                    Rc::new(|v| v.downcast_ref::<Value>().unwrap().clone()),
                    Some(instance.clone()),
                );
                let revoke = lease.clone();
                scope.defer_fn(move || revoke.revoke());
                if lifetime.aborted() {
                    lease.revoke();
                }
                lease
            }),
            notify: Rc::new(move || notify.notify()),
            report: services.report(),
        })
    }

    pub fn mirror(&self, spec: crate::mirror::MirrorSpec) -> Result<crate::mirror::Mirror, Error> {
        let mirror = crate::mirror::Mirror::new(spec, self.mirror_environment()?);
        let start = mirror.clone();
        self.0
            .session
            .as_ref()
            .unwrap()
            .add_mirror(&self.0.capability, Rc::new(move || start.start()));
        Ok(mirror)
    }

    pub fn page_mirror(
        &self,
        spec: crate::page_mirror::PageMirrorSpec,
    ) -> Result<crate::page_mirror::PageMirror, Error> {
        Ok(crate::page_mirror::PageMirror::new(spec, self.mirror_environment()?))
    }

    /// `ctx.events.source()`: a capability event, disposed with the instance.
    pub fn event_source<T: 'static>(&self) -> EventHookSource<T> {
        let hook = EventHookSource::new(Some(self.report()));
        let dispose = hook.clone();
        self.0.scope.defer_fn(move || dispose.dispose());
        hook
    }

    /// `ctx.listen(source, listener)`: the caller subscribes and hands over
    /// the unsubscribe, which the instance runs when it closes.
    pub fn listen(&self, unsubscribe: Unsubscribe) {
        self.0.scope.defer_fn(move || unsubscribe.call());
    }

    /// `ctx.waitFor(predicate, {signal})`: resolves when the predicate holds,
    /// checked now and on every store change; rejects `operation-cancelled`
    /// on the signal, `instance-closed` at close, or with what the predicate
    /// threw.
    pub fn wait_for(
        &self,
        predicate: impl Fn() -> Result<bool, Error> + 'static,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let (promise, resolver) = Promise::<()>::pending();
        let done = Rc::new(Cell::new(false));
        let unsubscribe: Rc<RefCell<Option<Unsubscribe>>> = Rc::default();
        let registrations: Rc<RefCell<(Option<u64>, Option<u64>)>> = Rc::default();
        let lifetime = self.0.lifetime.clone();
        let capability = self.0.capability.clone();
        let finish: Rc<dyn Fn(Result<(), Error>)> = {
            let (done, unsubscribe, registrations) =
                (done.clone(), unsubscribe.clone(), registrations.clone());
            let (lifetime, signal) = (lifetime.clone(), signal.clone());
            Rc::new(move |outcome| {
                if done.get() {
                    return;
                }
                done.set(true);
                if let Some(off) = unsubscribe.borrow_mut().take() {
                    off.call();
                }
                let (on_close, on_cancel) = *registrations.borrow();
                if let Some(id) = on_close {
                    lifetime.remove(id);
                }
                if let (Some(signal), Some(id)) = (&signal, on_cancel) {
                    signal.remove(id);
                }
                resolver.settle(outcome);
            })
        };
        let on_close = {
            let (finish, capability) = (finish.clone(), capability.clone());
            move || {
                finish(Err(plugin_error(
                    code::INSTANCE_CLOSED,
                    &capability,
                    "closed while waiting",
                    None,
                    None,
                )))
            }
        };
        let on_cancel = {
            let (finish, capability) = (finish.clone(), capability.clone());
            move || {
                finish(Err(plugin_error(
                    code::OPERATION_CANCELLED,
                    &capability,
                    "cancelled while waiting",
                    None,
                    None,
                )))
            }
        };
        let predicate = Rc::new(predicate);
        let check: Rc<dyn Fn()> = {
            let finish = finish.clone();
            Rc::new(move || match predicate() {
                Err(error) => finish(Err(error)),
                Ok(true) => finish(Ok(())),
                Ok(false) => {}
            })
        };
        let listener = check.clone();
        *unsubscribe.borrow_mut() = Some(self.0.ctx.subscribe(Rc::new(move || {
            listener();
            Ok(())
        })));
        if lifetime.aborted() {
            on_close();
            return promise;
        }
        if signal.as_ref().is_some_and(AbortSignal::aborted) {
            on_cancel();
            return promise;
        }
        let close_id = lifetime.on_abort(Rc::new(move |_| on_close()));
        let cancel_id = signal.as_ref().map(|s| s.on_abort(Rc::new(move |_| on_cancel())));
        *registrations.borrow_mut() = (Some(close_id), cancel_id);
        check();
        promise
    }

    /// `ctx.serialQueue(key)`: one queue per key, per instance.
    pub fn serial_queue(&self, key: &str) -> SerialQueue {
        self.0.queues.borrow_mut().entry(key.to_string()).or_default().clone()
    }

    /// `ctx.latest(key)`: one lane per key, per instance.
    pub fn latest(&self, key: &str) -> LatestLane {
        self.0
            .lanes
            .borrow_mut()
            .entry(key.to_string())
            .or_insert_with(|| LatestLane::new(self.0.lifetime.clone(), &self.0.capability, key))
            .clone()
    }

    /// `ctx.acquire(get, dispose)`: a resource whose disposal the instance
    /// owns. One that lands after close is disposed and not returned.
    pub fn acquire<R: Clone + 'static>(
        &self,
        get: impl FnOnce(AbortSignal) -> Result<Promise<R>, Error> + 'static,
        dispose: Rc<dyn Fn(R) -> Result<Option<Promise<()>>, Error>>,
    ) -> Promise<R> {
        let lifetime = self.0.lifetime.clone();
        let scope = self.0.scope.clone();
        let capability = self.0.capability.clone();
        Promise::spawn(async move {
            let value = get(lifetime.clone())?.await?;
            if lifetime.aborted() {
                match dispose(value)? {
                    None => rt::tick().await,
                    Some(disposal) => disposal.await?,
                }
                return Err(plugin_error(
                    code::INSTANCE_CLOSED,
                    &capability,
                    "closed while acquiring a resource",
                    None,
                    None,
                ));
            }
            let held = value.clone();
            scope.defer(Box::new(move || dispose(held)));
            Ok(value)
        })
    }

    fn meta(&self) -> Option<DocumentMeta> {
        let session = self.0.session.as_ref()?;
        let core = self.0.ctx.core();
        core.documents.get(&session.id()).cloned().or_else(|| session.staged_meta())
    }

    fn page_of(&self, page: PageRef) -> Option<Value> {
        let meta = self.meta()?;
        meta.pages.as_array()?.iter().find(|p| page.names(p)).cloned()
    }

    fn not_in_document(&self, page: PageRef) -> Error {
        plugin_error(
            code::NOT_FOUND,
            &self.0.capability,
            format!("page {} is not in this document", page.page_object_number),
            None,
            None,
        )
    }

    /// `ctx.geometry.tryForPage(ref)`: the page's space, or `None` for a
    /// page this document does not have.
    pub fn try_for_page(&self, page: PageRef) -> Option<PageSpace> {
        let layout = self.page_of(page)?;
        let crop: PdfEdges =
            serde_json::from_value(layout.get("boxes")?.get("crop")?.clone()).ok()?;
        Some(PageSpace::new(crop))
    }

    /// `ctx.geometry.forPage(ref)`: `not-found` for a foreign ref.
    pub fn for_page(&self, page: PageRef) -> Result<PageSpace, Error> {
        self.try_for_page(page).ok_or_else(|| self.not_in_document(page))
    }

    /// `ctx.assertPageRef(ref)`.
    pub fn assert_page_ref(&self, page: PageRef) -> Result<(), Error> {
        match self.page_of(page) {
            Some(_) => Ok(()),
            None => Err(self.not_in_document(page)),
        }
    }

    /// `ctx.getPage(ref)`: the registry entry, or `None`.
    pub fn get_page(&self, page: PageRef) -> Option<Value> {
        self.page_of(page)
    }

    /// `ctx.cleanup(fn)` on the instance scope.
    pub fn defer(&self, teardown: Teardown) {
        self.0.scope.defer(teardown);
    }
}

/// A controller context typed over the plugin's state and action.
pub struct Controller<S, A> {
    pub cx: ControllerContext,
    _types: PhantomData<fn() -> (S, A)>,
}

impl<S, A> Clone for Controller<S, A> {
    fn clone(&self) -> Self {
        Controller { cx: self.cx.clone(), _types: PhantomData }
    }
}

impl<S, A> std::ops::Deref for Controller<S, A> {
    type Target = ControllerContext;
    fn deref(&self) -> &ControllerContext {
        &self.cx
    }
}

impl<S: 'static, A: Action> Controller<S, A> {
    pub fn new(cx: ControllerContext) -> Self {
        Controller { cx, _types: PhantomData }
    }

    /// `ctx.getState()`.
    pub fn state(&self) -> Rc<S> {
        self.cx
            .state_any()
            .and_then(|s| s.downcast::<S>().ok())
            .unwrap_or_else(|| panic!("[{}] state is not the type its plugin names", self.cx.id()))
    }

    /// Apply a pure session transition without dispatching a reducer action.
    pub fn update_state(&self, transition: impl FnOnce(Rc<S>) -> Rc<S>) -> bool {
        self.cx.lease().write(transition(self.state()))
    }

    pub fn on_state_change(
        &self,
        listener: impl Fn(Rc<S>, Rc<S>) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let state = self.state();
        let previous = Rc::new(RefCell::new(state));
        let cx = self.clone();
        let off = self.cx.subscribe(Rc::new(move || {
            let next = cx.state();
            let before = previous.borrow().clone();
            if !Rc::ptr_eq(&before, &next) {
                *previous.borrow_mut() = next.clone();
                listener(next, before)?;
            }
            Ok(())
        }));
        self.cx.listen(off.clone());
        off
    }

    pub fn dispatch(&self, action: A) {
        self.cx.dispatch_any(&action);
    }
}

// ── the typed layer ───────────────────────────────────────────────────────────

/// A plugin context typed over the plugin's state `S` and action `A`.
pub struct Ctx<S, A> {
    pub ctx: PluginContext,
    _types: PhantomData<fn() -> (S, A)>,
}

impl<S, A> Clone for Ctx<S, A> {
    fn clone(&self) -> Self {
        Ctx { ctx: self.ctx.clone(), _types: PhantomData }
    }
}

impl<S, A> std::ops::Deref for Ctx<S, A> {
    type Target = PluginContext;
    fn deref(&self) -> &PluginContext {
        &self.ctx
    }
}

impl<S: 'static, A: Action> Ctx<S, A> {
    pub fn new(ctx: PluginContext) -> Self {
        Ctx { ctx, _types: PhantomData }
    }

    /// `ctx.getState()`. `None` only after the slice was removed.
    pub fn try_state(&self) -> Option<Rc<S>> {
        self.ctx.state_any()?.downcast::<S>().ok()
    }

    /// `ctx.getState()`, for the (common) code that runs only while the
    /// slice exists. Panics otherwise, which in TypeScript is reading a
    /// member of `undefined`.
    pub fn state(&self) -> Rc<S> {
        self.try_state()
            .unwrap_or_else(|| panic!("[{}] state read after its slice was removed", self.ctx.id()))
    }

    pub fn dispatch(&self, action: A) {
        self.ctx.dispatch_any(&action);
    }
}

/// An effect context typed over the plugin's state and action.
pub struct EffectCtx<S, A> {
    pub fx: EffectContext,
    pub ctx: Ctx<S, A>,
}

impl<S, A> Clone for EffectCtx<S, A> {
    fn clone(&self) -> Self {
        EffectCtx { fx: self.fx.clone(), ctx: self.ctx.clone() }
    }
}

impl<S, A> std::ops::Deref for EffectCtx<S, A> {
    type Target = Ctx<S, A>;
    fn deref(&self) -> &Ctx<S, A> {
        &self.ctx
    }
}

impl<S: 'static, A: Action> EffectCtx<S, A> {
    pub fn watch<R: Clone + 'static>(
        &self,
        select: impl Fn() -> R + 'static,
        is_equal: impl Fn(&R, &R) -> bool + 'static,
        handler: impl Fn(&R, &R) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.fx.watch(select, is_equal, handler)
    }

    pub fn on_action(
        &self,
        action_type: &str,
        handler: impl Fn(&dyn Action) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.fx.on_action(action_type, handler)
    }
}

type TypedReducer<S, A> = Rc<dyn Fn(&S, &A) -> Option<S>>;

/// `definePlugin`, typed. `S` is the slice state, `A` the action union.
pub struct PluginBuilder<S, A> {
    id: String,
    token: Option<AnyToken>,
    scope: PluginScope,
    requires: Vec<AnyToken>,
    optional: Vec<AnyToken>,
    initial_state: Option<Rc<dyn Fn() -> S>>,
    reduce: Option<TypedReducer<S, A>>,
    slice_text: Option<Rc<dyn Fn(&S) -> String>>,
    capability: Option<CapabilityFn>,
    init: Option<InitFn>,
    effects: Option<EffectsFn>,
    create: Option<CreateFn>,
    settings: Option<crate::settings::SettingsDeclaration>,
}

pub fn define<S, A>(id: impl Into<String>) -> PluginBuilder<S, A> {
    PluginBuilder {
        id: id.into(),
        token: None,
        scope: PluginScope::Workspace,
        requires: Vec::new(),
        optional: Vec::new(),
        initial_state: None,
        reduce: None,
        slice_text: None,
        capability: None,
        init: None,
        effects: None,
        create: None,
        settings: None,
    }
}

impl<S: Serialize + 'static, A: Action> PluginBuilder<S, A> {
    pub fn settings(mut self, declaration: crate::settings::SettingsDeclaration) -> Self {
        self.settings = Some(declaration);
        self
    }

    pub fn scope(mut self, scope: PluginScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn document_scoped(self) -> Self {
        self.scope(PluginScope::Document)
    }

    pub fn requires(mut self, token: &AnyToken) -> Self {
        self.requires.push(token.clone());
        self
    }

    pub fn optional(mut self, token: &AnyToken) -> Self {
        self.optional.push(token.clone());
        self
    }

    pub fn initial_state(mut self, initial: impl Fn() -> S + 'static) -> Self {
        self.initial_state = Some(Rc::new(initial));
        self
    }

    /// The pure reducer. `None` is `return state`.
    /// Write the slice's JSON text directly. It must be byte for byte what
    /// `js::to_value(state).to_string()` gives; the store uses it in place
    /// of building the whole value.
    pub fn slice_text(mut self, text: impl Fn(&S) -> String + 'static) -> Self {
        self.slice_text = Some(Rc::new(text));
        self
    }

    pub fn reduce(mut self, reduce: impl Fn(&S, &A) -> Option<S> + 'static) -> Self {
        self.reduce = Some(Rc::new(reduce));
        self
    }

    /// The token and the capability built for it.
    pub fn capability<C: 'static>(
        mut self,
        token: &Token<C>,
        build: impl Fn(Ctx<S, A>) -> Result<C, Error> + 'static,
    ) -> Self {
        self.token = Some(token.any().clone());
        self.capability =
            Some(Rc::new(move |ctx| build(Ctx::new(ctx)).map(|c| Rc::new(c) as Rc<dyn Any>)));
        self
    }

    /// The token and the v3 controller built for it: `build` returns the api
    /// and, optionally, the `connect` half.
    pub fn create<C: 'static>(
        mut self,
        token: &Token<C>,
        build: impl Fn(Controller<S, A>) -> Result<(C, Option<ConnectFn>), Error> + 'static,
    ) -> Self {
        self.token = Some(token.any().clone());
        self.create = Some(Rc::new(move |cx| {
            let (api, connect) = build(Controller::new(cx))?;
            Ok(Created { api: Rc::new(api), connect })
        }));
        self
    }

    pub fn init(
        mut self,
        init: impl Fn(Ctx<S, A>) -> Result<Option<Promise<()>>, Error> + 'static,
    ) -> Self {
        self.init = Some(Rc::new(move |ctx| init(Ctx::new(ctx))));
        self
    }

    pub fn effects(
        mut self,
        effects: impl Fn(EffectCtx<S, A>) -> Result<(), Error> + 'static,
    ) -> Self {
        self.effects = Some(Rc::new(move |fx: EffectContext| {
            let ctx = Ctx::new(fx.ctx.clone());
            effects(EffectCtx { fx, ctx })
        }));
        self
    }

    /// Panics for a reducer without an [`initial_state`](Self::initial_state)
    /// (unless the state is a JSON [`Value`]): the slice would start as
    /// `{}`, which is not an `S`, so the reducer could never run.
    pub fn build(self) -> AnyPlugin {
        assert!(
            self.reduce.is_none()
                || self.initial_state.is_some()
                || std::any::TypeId::of::<S>() == std::any::TypeId::of::<Value>(),
            "[{}] a plugin with a reducer needs an initial_state",
            self.id
        );
        let initial_state: Rc<dyn Fn() -> SliceState> = match self.initial_state {
            Some(initial) => Rc::new(move || Rc::new(initial()) as SliceState),
            // `plugin.initialState ?? {}`
            None => Rc::new(|| Rc::new(Value::Object(Default::default())) as SliceState),
        };
        let reducer: SliceReducer = match self.reduce {
            Some(reduce) => Rc::new(move |state: &SliceState, action: &dyn Action| {
                let state = state.downcast_ref::<S>()?;
                let action = action.downcast::<A>()?;
                reduce(state, action).map(|next| Rc::new(next) as SliceState)
            }),
            None => Rc::new(|_: &SliceState, _: &dyn Action| None),
        };
        let to_json: SliceJson = Rc::new(|state: &dyn Any| {
            if let Some(state) = state.downcast_ref::<S>() {
                crate::js::to_value(state)
            } else if let Some(value) = state.downcast_ref::<Value>() {
                value.clone()
            } else {
                Value::Null
            }
        });
        let to_text: Option<SliceText> = self.slice_text.map(|text| {
            Rc::new(move |state: &dyn Any| state.downcast_ref::<S>().map(|state| text(state)))
                as SliceText
        });
        Rc::new(PluginDef {
            settings: self.settings,
            id: self.id,
            token: self.token,
            scope: self.scope,
            requires: self.requires,
            optional: self.optional,
            initial_state,
            reducer,
            to_json,
            to_text,
            capability: self.capability,
            init: self.init,
            effects: self.effects,
            create: self.create,
        })
    }
}
