//! `PluginDef`, the contexts a plugin receives, and the typed layer plugin
//! crates write against.
//!
//! The kernel works with the erased [`PluginDef`]: state is `Rc<dyn Any>`,
//! actions are `&dyn Action`, capabilities are `Rc<dyn Any>`. A plugin crate
//! uses [`define`] and gets [`Ctx`] and [`EffectCtx`], typed over its own
//! state and action, with the downcasts done once, here.

use std::any::Any;
use std::marker::PhantomData;
use std::rc::{Rc, Weak};

use serde::Serialize;
use serde_json::Value;

use crate::action::Action;
use crate::engine::{DocumentHandle, Engine};
use crate::error::Error;
use crate::rt::Promise;
use crate::scope::{Scope, Teardown};
use crate::store::{SliceJson, SliceReducer, SliceState, Store, Unsubscribe};
use crate::token::{AnyToken, Token};
use crate::types::{CoreState, DocumentMeta, PluginScope};

pub type CapabilityFn = Rc<dyn Fn(PluginContext) -> Result<Rc<dyn Any>, Error>>;
/// `init`: `Ok(None)` is a synchronous init (still one turn, as `await
/// undefined` is), `Ok(Some)` one to await, `Err` a synchronous throw.
pub type InitFn = Rc<dyn Fn(PluginContext) -> Result<Option<Promise<()>>, Error>>;
pub type EffectsFn = Rc<dyn Fn(EffectContext) -> Result<(), Error>>;

pub struct PluginDef {
    pub id: String,
    pub token: Option<AnyToken>,
    pub scope: PluginScope,
    pub requires: Vec<AnyToken>,
    pub optional: Vec<AnyToken>,
    pub initial_state: Rc<dyn Fn() -> SliceState>,
    pub reducer: SliceReducer,
    pub to_json: SliceJson,
    pub capability: Option<CapabilityFn>,
    pub init: Option<InitFn>,
    pub effects: Option<EffectsFn>,
}

pub type AnyPlugin = Rc<PluginDef>;

// ── what a context needs from the kernel ──────────────────────────────────────

pub(crate) trait Services {
    fn engine(&self) -> Rc<dyn Engine>;
    fn store(&self) -> Rc<Store>;
    fn workspace_scope(&self) -> Scope;
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
    fn id(&self) -> String;
    fn handle(&self) -> Option<Rc<dyn DocumentHandle>>;
    fn staged_meta(&self) -> Option<DocumentMeta>;
    fn scope(&self) -> Scope;
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
#[derive(Clone)]
pub struct PluginContext {
    services: Weak<dyn Services>,
    plugin_id: Rc<str>,
    key: Rc<str>,
    document_id: Option<Rc<str>>,
    doc: Option<Rc<dyn DocumentHandle>>,
    session: Option<Rc<dyn SessionView>>,
}

fn gone() -> Error {
    Error::new("[kernel] used after the kernel was dropped")
}

impl PluginContext {
    pub(crate) fn new(
        services: Weak<dyn Services>,
        plugin: &PluginDef,
        session: Option<Rc<dyn SessionView>>,
    ) -> Self {
        let document_id = session.as_ref().map(|s| Rc::<str>::from(s.id()));
        let doc = match &session {
            Some(session) => session.handle(),
            None => services.upgrade().and_then(|s| s.document_handle(None)),
        };
        PluginContext {
            key: slice_key(&plugin.id, document_id.as_deref()).into(),
            plugin_id: plugin.id.as_str().into(),
            services,
            document_id,
            doc,
            session,
        }
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

    /// `ctx.getState()`, erased. `None` once the slice is gone.
    pub fn state_any(&self) -> Option<SliceState> {
        self.services()?.store().get_slice(&self.key)
    }

    pub fn dispatch_any(&self, action: &dyn Action) {
        if let Some(services) = self.services() {
            services.observe_dispatch(&self.key, action);
            services.store().dispatch_to(&self.key, action);
        }
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
        self.services().ok_or_else(gone)?.resolve(token, self.document_id())
    }

    pub fn for_document_any(
        &self,
        token: &AnyToken,
        document_id: &str,
    ) -> Result<Rc<dyn Any>, Error> {
        self.services().ok_or_else(gone)?.resolve(token, Some(document_id))
    }

    pub fn try_get_any(&self, token: &AnyToken) -> Result<Option<Rc<dyn Any>>, Error> {
        match self.services() {
            Some(services) => services.try_resolve_internal(token, self.document_id()),
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
        let action_type = action_type.to_string();
        let unsubscribe = match self.ctx.services() {
            Some(services) => {
                services.store().subscribe_action(Rc::new(move |action: &dyn Action| {
                    if action.action_type() == action_type { handler(action) } else { Ok(()) }
                }))
            }
            None => Unsubscribe::noop(),
        };
        if let Some(scope) = self.ctx.own_scope() {
            let release = unsubscribe.clone();
            scope.defer_fn(move || release.call());
        }
        unsubscribe
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
    capability: Option<CapabilityFn>,
    init: Option<InitFn>,
    effects: Option<EffectsFn>,
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
        capability: None,
        init: None,
        effects: None,
    }
}

impl<S: Serialize + 'static, A: Action> PluginBuilder<S, A> {
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

    pub fn build(self) -> AnyPlugin {
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
        Rc::new(PluginDef {
            id: self.id,
            token: self.token,
            scope: self.scope,
            requires: self.requires,
            optional: self.optional,
            initial_state,
            reducer,
            to_json,
            capability: self.capability,
            init: self.init,
            effects: self.effects,
        })
    }
}
