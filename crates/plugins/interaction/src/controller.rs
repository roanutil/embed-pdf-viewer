//! `controller.ts`: the interaction hub.
//!
//! Tools, handlers, the captured-gesture owner and cursor claims are runtime
//! registries here; the model holds only the active tool, the tool stack and
//! the resolved cursor, so UI can react.
//!
//! Cursor arbitration, top to bottom: the highest-priority claim (hover
//! feedback), then over a page the tool's declared cursor, then over a gap
//! the tool's `gapCursor`. The winning keyword (claim or base, never gap) is
//! restyled through the armed tool's skin when it maps that keyword.
//!
//! No `RefCell` borrow is held across a call into a handler or a listener:
//! handlers call back into the hub (`claimCursor`) from inside a dispatch.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::{Controller, Error, Unsubscribe};

use crate::callbacks::Unregister;
use crate::contract::{
    ActivateToolOptions, Cursor, CursorChangedEvent, GestureEvent, InteractionHandler, Phase,
    PointerKind, PointerSample, RegisterHandlerOptions, RegisterToolOptions, Tool,
    ToolChangedEvent, ToolCursorSkin, ToolId,
};
use crate::model::{InteractionAction, InteractionState};

struct Claim {
    cursor: Cursor,
    priority: f64,
}

type Handler = Rc<dyn InteractionHandler>;

fn same(a: &Handler, b: &Handler) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

struct Hub {
    cx: Controller<InteractionState, InteractionAction>,
    /// A `Map<ToolId, Tool>`: insertion order, replacement in place. The
    /// serial stands in for the registered object's identity.
    tools: RefCell<Vec<(Tool, u64)>>,
    next_serial: Cell<u64>,
    handlers: RefCell<Vec<Handler>>,
    /// A `Map<InteractionHandler, string>`, keyed by identity.
    handler_sources: RefCell<Vec<(Handler, String)>>,
    /// A `Map<string, Claim>`: re-claiming a token keeps its place.
    claims: RefCell<Vec<(String, Claim)>>,
    cursor_skins: RefCell<Vec<(ToolId, ToolCursorSkin)>>,
    owner: RefCell<Option<Handler>>,
    /// Whether the last dispatched sample hit a page; gaps fall back to `gapCursor`.
    over_page: Cell<bool>,
    tool_changed: EventHookSource<ToolChangedEvent>,
    gesture_started: EventHookSource<GestureEvent>,
    gesture_ended: EventHookSource<GestureEvent>,
    gesture_cancelled: EventHookSource<GestureEvent>,
    cursor_changed: EventHookSource<CursorChangedEvent>,
}

impl Hub {
    fn release(&self) {
        self.handlers.borrow_mut().clear();
        self.handler_sources.borrow_mut().clear();
        self.owner.borrow_mut().take();
    }

    fn set_tool(&self, tool: Tool) -> u64 {
        let serial = self.next_serial.get();
        self.next_serial.set(serial + 1);
        let mut tools = self.tools.borrow_mut();
        match tools.iter_mut().find(|(t, _)| t.id == tool.id) {
            Some(slot) => *slot = (tool, serial),
            None => tools.push((tool, serial)),
        }
        serial
    }
}

/// `InteractionHostCapability`: the public `InteractionCapability` plus the
/// host lens (`dispatch_pointer`, `would_claim_touch`, `register_handler`,
/// `claim_cursor`, `get_cursor`, `on_cursor_changed`). The TypeScript hands
/// out one object under one token and types it two ways; Rust has the one
/// type. Cheap to clone; clones are the same hub.
#[derive(Clone)]
pub struct InteractionCapability(Rc<Hub>);

/// `createInteractionController(ctx, builtinTools, configTools)`.
pub fn create_interaction_controller(
    cx: Controller<InteractionState, InteractionAction>,
    builtin_tools: Vec<Tool>,
    config_tools: Vec<Tool>,
) -> Result<(InteractionCapability, Option<ConnectFn>), Error> {
    let hub = Rc::new(Hub {
        tool_changed: cx.event_source(),
        gesture_started: cx.event_source(),
        gesture_ended: cx.event_source(),
        gesture_cancelled: cx.event_source(),
        cursor_changed: cx.event_source(),
        cx,
        tools: RefCell::new(Vec::new()),
        next_serial: Cell::new(0),
        handlers: RefCell::new(Vec::new()),
        handler_sources: RefCell::new(Vec::new()),
        claims: RefCell::new(Vec::new()),
        cursor_skins: RefCell::new(Vec::new()),
        owner: RefCell::new(None),
        over_page: Cell::new(false),
    });
    for tool in builtin_tools.into_iter().chain(config_tools) {
        hub.set_tool(tool);
    }
    // Rust only: handlers hold capabilities that hold this hub, so the
    // registries are released when the instance closes, as a garbage
    // collector would reclaim the cycle.
    let weak = Rc::downgrade(&hub);
    hub.cx.defer(Box::new(move || {
        if let Some(hub) = weak.upgrade() {
            hub.release();
        }
        Ok(None)
    }));
    Ok((InteractionCapability(hub), None))
}

impl InteractionCapability {
    fn state(&self) -> Rc<InteractionState> {
        self.0.cx.state()
    }

    fn tool_of(&self, id: &str) -> Tool {
        self.get_tool(id).unwrap_or_else(|| Tool::new(id, "default", []))
    }

    fn skin_lookup(&self, tool_id: &str, keyword: &str) -> Option<Cursor> {
        let skins = self.0.cursor_skins.borrow();
        let (_, skin) = skins.iter().find(|(id, _)| id == tool_id)?;
        skin.get(keyword).cloned()
    }

    fn resolve_cursor(&self) -> Cursor {
        let top = {
            let claims = self.0.claims.borrow();
            let mut top: Option<&Claim> = None;
            for (_, claim) in claims.iter() {
                if top.is_none_or(|t| claim.priority > t.priority) {
                    top = Some(claim);
                }
            }
            top.map(|c| c.cursor.clone())
        };
        let tool = self.get_active_tool();
        if let Some(top) = top {
            return self.skin_lookup(&tool.id, &top).unwrap_or(top);
        }
        if !self.0.over_page.get() {
            return tool.gap_cursor.unwrap_or_else(|| "default".to_string());
        }
        self.skin_lookup(&tool.id, &tool.cursor).unwrap_or(tool.cursor)
    }

    fn sync_cursor(&self) {
        let next = self.resolve_cursor();
        if next == self.state().cursor {
            return;
        }
        self.0.cx.dispatch(InteractionAction::SetCursor { cursor: next.clone() });
        self.0.cursor_changed.emit(&CursorChangedEvent { cursor: next });
    }

    /// Lens scoping: a handler registered with a `source` only sees samples
    /// stamped with it, or unstamped ones.
    fn eligible(&self, source: Option<&str>) -> Vec<Handler> {
        let tool = self.get_active_tool();
        let scoped: Vec<Handler> = {
            let handlers = self.0.handlers.borrow();
            let sources = self.0.handler_sources.borrow();
            handlers
                .iter()
                .filter(|h| {
                    let hs = sources.iter().find(|(k, _)| same(k, h)).map(|(_, s)| s.as_str());
                    hs.is_none() || source.is_none() || hs == source
                })
                .cloned()
                .collect()
        };
        let mut live: Vec<Handler> = scoped.into_iter().filter(|h| h.enabled_for(&tool)).collect();
        // `sort((a, b) => b.priority - a.priority)`: stable, and a NaN
        // difference compares equal.
        live.sort_by(|a, b| {
            let d = b.priority() - a.priority();
            if d < 0.0 {
                std::cmp::Ordering::Less
            } else if d > 0.0 {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        live
    }

    fn gesture_of(handler: &Handler, sample: &PointerSample) -> GestureEvent {
        GestureEvent {
            handler_id: handler.id().to_string(),
            page: sample.page.map(|hit| hit.page),
            pointer_type: sample.pointer_type.unwrap_or(PointerKind::Mouse),
        }
    }

    fn arm(
        &self,
        id: &str,
        action: InteractionAction,
        options: Option<ActivateToolOptions>,
    ) -> Result<(), Error> {
        if !self.has_tool(id) {
            return Err(plugin_error(
                code::NOT_FOUND,
                "interaction",
                format!("unknown tool '{id}'"),
                None,
                None,
            ));
        }
        let previous_tool_id = self.state().active_tool_id.clone();
        self.0.owner.borrow_mut().take();
        // The previous tool's hover claims die with it; handlers re-claim on hover.
        self.0.claims.borrow_mut().clear();
        self.0.cx.dispatch(action);
        self.sync_cursor();
        self.0.tool_changed.emit(&ToolChangedEvent {
            tool_id: id.to_string(),
            previous_tool_id,
            tool: self.tool_of(id),
            payload: options.and_then(|o| o.payload),
        });
        Ok(())
    }

    // ── public ────────────────────────────────────────────────────────────

    /// The armed tool. An id nobody registered reads back as a neutral tool
    /// with no tags.
    pub fn get_active_tool(&self) -> Tool {
        self.tool_of(&self.state().active_tool_id)
    }

    pub fn get_active_tool_id(&self) -> ToolId {
        self.state().active_tool_id.clone()
    }

    /// The tool configured for a freshly opened document.
    pub fn get_default_tool_id(&self) -> ToolId {
        self.state().default_tool_id.clone()
    }

    pub fn list_tools(&self) -> Vec<Tool> {
        self.0.tools.borrow().iter().map(|(t, _)| t.clone()).collect()
    }

    pub fn get_tool(&self, id: &str) -> Option<Tool> {
        self.0.tools.borrow().iter().find(|(t, _)| t.id == id).map(|(t, _)| t.clone())
    }

    pub fn has_tool(&self, id: &str) -> bool {
        self.0.tools.borrow().iter().any(|(t, _)| t.id == id)
    }

    /// Does the active tool enable a behaviour tag such as `text-select`.
    pub fn active_tool_enables(&self, behavior: &str) -> bool {
        self.get_active_tool().enables(behavior)
    }

    /// Arm a tool. Errs `not-found` for an unknown id.
    pub fn activate_tool(
        &self,
        id: &str,
        options: Option<ActivateToolOptions>,
    ) -> Result<(), Error> {
        self.arm(id, InteractionAction::SetTool { tool_id: id.to_string() }, options)
    }

    /// Arm the configured default. Errs `not-found` when nobody registered it.
    pub fn activate_default_tool(&self) -> Result<(), Error> {
        let id = self.get_default_tool_id();
        self.arm(&id, InteractionAction::SetTool { tool_id: id.clone() }, None)
    }

    /// Temporarily arm a tool (hold space to pan); `pop_tool` restores the previous one.
    pub fn push_tool(&self, id: &str, options: Option<ActivateToolOptions>) -> Result<(), Error> {
        self.arm(id, InteractionAction::PushTool { tool_id: id.to_string() }, options)
    }

    /// Restore the tool under the top of the stack; nothing when the stack
    /// is empty. Errs `not-found` when that tool was unregistered meanwhile.
    pub fn pop_tool(&self) -> Result<(), Error> {
        let Some(previous) = self.state().tool_stack.last().cloned() else {
            return Ok(());
        };
        self.arm(&previous, InteractionAction::PopTool, None)
    }

    /// Reskin a tool's cursor keywords at runtime; `None` removes the skin.
    pub fn set_tool_cursor(&self, id: &str, skin: Option<ToolCursorSkin>) {
        {
            let mut skins = self.0.cursor_skins.borrow_mut();
            match skin {
                None => skins.retain(|(k, _)| k != id),
                Some(skin) => match skins.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = skin,
                    None => skins.push((id.to_string(), skin)),
                },
            }
        }
        self.sync_cursor();
    }

    /// Add a tool. A duplicate id errs `conflict` unless `replace`; the
    /// remover only removes this registration (and the tool's skin).
    pub fn register_tool(
        &self,
        tool: Tool,
        options: RegisterToolOptions,
    ) -> Result<Unregister, Error> {
        if self.has_tool(&tool.id) && !options.replace {
            return Err(plugin_error(
                code::CONFLICT,
                "interaction",
                format!(
                    "tool '{}' is already registered; pass {{ replace: true }} to replace it",
                    tool.id
                ),
                None,
                None,
            ));
        }
        let id = tool.id.clone();
        let serial = self.0.set_tool(tool);
        let hub = Rc::downgrade(&self.0);
        Ok(Unregister::new(move || {
            let Some(hub) = hub.upgrade() else { return };
            // Own this registration only: a later replacement is not ours to remove.
            let mut tools = hub.tools.borrow_mut();
            let Some(i) = tools.iter().position(|(t, s)| t.id == id && *s == serial) else {
                return;
            };
            tools.remove(i);
            hub.cursor_skins.borrow_mut().retain(|(k, _)| *k != id);
        }))
    }

    pub fn on_tool_changed(
        &self,
        listener: impl Fn(&ToolChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.tool_changed.on(listener)
    }

    pub fn on_gesture_started(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_started.on(listener)
    }

    pub fn on_gesture_ended(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_ended.on(listener)
    }

    pub fn on_gesture_cancelled(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_cancelled.on(listener)
    }

    // ── host ──────────────────────────────────────────────────────────────

    /// Register a pointer handler; `source` scopes it to one lens.
    pub fn register_handler(
        &self,
        handler: Rc<dyn InteractionHandler>,
        options: RegisterHandlerOptions,
    ) -> Unregister {
        self.0.handlers.borrow_mut().push(handler.clone());
        if let Some(source) = options.source {
            let mut sources = self.0.handler_sources.borrow_mut();
            match sources.iter_mut().find(|(k, _)| same(k, &handler)) {
                Some(slot) => slot.1 = source,
                None => sources.push((handler.clone(), source)),
            }
        }
        let hub = Rc::downgrade(&self.0);
        Unregister::new(move || {
            let Some(hub) = hub.upgrade() else { return };
            {
                let mut handlers = hub.handlers.borrow_mut();
                if let Some(i) = handlers.iter().position(|h| same(h, &handler)) {
                    handlers.remove(i);
                }
            }
            hub.handler_sources.borrow_mut().retain(|(k, _)| !same(k, &handler));
            let mut owner = hub.owner.borrow_mut();
            if owner.as_ref().is_some_and(|o| same(o, &handler)) {
                *owner = None;
            }
        })
    }

    /// Push or clear (`None`) a cursor claim on the priority stack. The
    /// TypeScript priority defaults to 0.
    pub fn claim_cursor(&self, token: &str, cursor: Option<&str>, priority: f64) {
        {
            let mut claims = self.0.claims.borrow_mut();
            match cursor {
                None => claims.retain(|(k, _)| k != token),
                Some(cursor) => {
                    let claim = Claim { cursor: cursor.to_string(), priority };
                    match claims.iter_mut().find(|(k, _)| k == token) {
                        Some(slot) => slot.1 = claim,
                        None => claims.push((token.to_string(), claim)),
                    }
                }
            }
        }
        self.sync_cursor();
    }

    /// The resolved cursor.
    pub fn get_cursor(&self) -> Cursor {
        self.state().cursor.clone()
    }

    pub fn on_cursor_changed(
        &self,
        listener: impl Fn(&CursorChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.cursor_changed.on(listener)
    }

    /// Touch-arbitration pre-flight: would any eligible handler claim this
    /// contact. Pure.
    pub fn would_claim_touch(&self, sample: &PointerSample) -> bool {
        self.eligible(sample.source.as_deref()).iter().any(|h| h.claims_touch(sample))
    }

    /// Pointer ingress: a source calls it for every normalized event.
    pub fn dispatch_pointer(&self, sample: &PointerSample) {
        let now_over_page = sample.page.is_some();
        if now_over_page != self.0.over_page.get() {
            self.0.over_page.set(now_over_page);
            self.sync_cursor(); // crossing a page edge re-resolves the base cursor
        }
        match sample.phase {
            Phase::Down => {
                self.0.owner.borrow_mut().take();
                for h in self.eligible(sample.source.as_deref()) {
                    if h.on_down(sample) {
                        *self.0.owner.borrow_mut() = Some(h.clone());
                        self.0.gesture_started.emit(&Self::gesture_of(&h, sample));
                        break;
                    }
                }
            }
            Phase::Move => {
                let owner = self.0.owner.borrow().clone();
                match owner {
                    Some(owner) => owner.on_move(sample),
                    None => {
                        for h in self.eligible(sample.source.as_deref()) {
                            h.on_hover(sample);
                        }
                    }
                }
            }
            Phase::Cancel => {
                // Abort, don't commit: on_cancel falls back to on_up.
                let owner = self.0.owner.borrow_mut().take();
                if let Some(owner) = owner {
                    owner.on_cancel(sample);
                    self.0.gesture_cancelled.emit(&Self::gesture_of(&owner, sample));
                }
            }
            Phase::Up => {
                let owner = self.0.owner.borrow_mut().take();
                if let Some(owner) = owner {
                    owner.on_up(sample);
                    self.0.gesture_ended.emit(&Self::gesture_of(&owner, sample));
                }
            }
        }
    }

    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript, where a closed capability is unreachable; a foreign
    /// handle outlives the close, checks this before every call, and ends
    /// its event streams when it aborts.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
}
