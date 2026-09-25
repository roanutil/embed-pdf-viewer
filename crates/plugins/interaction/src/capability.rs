//! `capability.ts`: the interaction hub.
//!
//! Tools, handlers, the captured-gesture owner and the cursor claims live
//! here (runtime registries, not serializable state); the slice holds only
//! the active tool id and the resolved cursor. `dispatch` is the router: on
//! down, the first eligible handler by priority to return true owns the
//! gesture; move and up go to it; with no owner, move is hover.
//!
//! Cursor arbitration, top to bottom: the highest-priority claim, then over
//! a page the tool's declared cursor, then over a gap the tool's `gapCursor`
//! (default the neutral arrow). The winning keyword (claim or base, never
//! gap) is restyled through the armed tool's skin when it maps it.
//!
//! No `RefCell` borrow is held across a call into a handler or callback:
//! the selection handler, for one, calls back into the hub (`setCursor`)
//! from inside `dispatch`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_kernel::{Ctx, Error};

use crate::callbacks::{CallbackSet, Unregister, Unsubscriber};
use crate::types::{
    Cursor, InteractionAction, InteractionHandler, InteractionState, Phase, PointerSample, Tool,
    ToolCursorSkin, ToolId,
};

struct Claim {
    cursor: Cursor,
    priority: f64,
}

type Handler = Rc<dyn InteractionHandler>;

fn same(a: &Handler, b: &Handler) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

#[derive(Default)]
struct Hub {
    /// A `Map<ToolId, Tool>`: insertion order, replacement in place.
    tools: RefCell<Vec<Tool>>,
    handlers: RefCell<Vec<Handler>>,
    /// A `Map<InteractionHandler, string>`, keyed by identity.
    handler_sources: RefCell<Vec<(Handler, String)>>,
    /// A `Map<string, Claim>`: re-claiming a token keeps its place.
    claims: RefCell<Vec<(String, Claim)>>,
    cursor_skins: RefCell<Vec<(ToolId, ToolCursorSkin)>>,
    tool_cbs: CallbackSet,
    owner: RefCell<Option<Handler>>,
    /// Whether the last dispatched sample hit a page.
    over_page: Cell<bool>,
}

impl Hub {
    fn release(&self) {
        self.handlers.borrow_mut().clear();
        self.handler_sources.borrow_mut().clear();
        self.owner.borrow_mut().take();
        self.tool_cbs.clear();
    }
}

/// `InteractionCapability`. Cheap to clone; clones are the same hub.
#[derive(Clone)]
pub struct InteractionCapability {
    ctx: Ctx<InteractionState, InteractionAction>,
    hub: Rc<Hub>,
}

/// `createInteractionCapability(ctx, builtinTools)`.
pub fn create_interaction_capability(
    ctx: Ctx<InteractionState, InteractionAction>,
    builtin_tools: Vec<Tool>,
) -> InteractionCapability {
    let hub = Rc::new(Hub::default());
    for tool in builtin_tools {
        set_tool(&mut hub.tools.borrow_mut(), tool);
    }
    // Rust only: handlers hold capabilities that hold this hub, so the
    // registries are released when the document closes, as a garbage
    // collector would reclaim the cycle.
    let weak = Rc::downgrade(&hub);
    ctx.cleanup_fn(move || {
        if let Some(hub) = weak.upgrade() {
            hub.release();
        }
    });
    InteractionCapability { ctx, hub }
}

fn set_tool(tools: &mut Vec<Tool>, tool: Tool) {
    match tools.iter_mut().find(|t| t.id == tool.id) {
        Some(slot) => *slot = tool,
        None => tools.push(tool),
    }
}

impl InteractionCapability {
    fn tool_of(&self, id: &str) -> Tool {
        self.hub
            .tools
            .borrow()
            .iter()
            .find(|t| t.id == id)
            .cloned()
            .unwrap_or_else(|| Tool::new(id, "default", []))
    }

    fn skin_lookup(&self, tool_id: &str, keyword: &str) -> Option<Cursor> {
        let skins = self.hub.cursor_skins.borrow();
        let (_, skin) = skins.iter().find(|(id, _)| id == tool_id)?;
        skin.get(keyword).cloned()
    }

    fn resolve_cursor(&self) -> Cursor {
        let top = {
            let claims = self.hub.claims.borrow();
            let mut top: Option<&Claim> = None;
            for (_, claim) in claims.iter() {
                if top.is_none_or(|t| claim.priority > t.priority) {
                    top = Some(claim);
                }
            }
            top.map(|c| c.cursor.clone())
        };
        let tool = self.active_tool();
        if let Some(top) = top {
            return self.skin_lookup(&tool.id, &top).unwrap_or(top);
        }
        if !self.hub.over_page.get() {
            return tool.gap_cursor.unwrap_or_else(|| "default".to_string());
        }
        self.skin_lookup(&tool.id, &tool.cursor).unwrap_or(tool.cursor)
    }

    fn sync_cursor(&self) {
        let next = self.resolve_cursor();
        if next != self.ctx.state().cursor {
            self.ctx.dispatch(InteractionAction::SetCursor { cursor: next });
        }
    }

    /// Lens scoping filters on a definite mismatch only: an unstamped
    /// sample still routes everywhere.
    fn eligible(&self, source: Option<&str>) -> Vec<Handler> {
        let tool = self.active_tool();
        let scoped: Vec<Handler> = {
            let handlers = self.hub.handlers.borrow();
            let sources = self.hub.handler_sources.borrow();
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

    // ── selectors ──

    pub fn active_tool(&self) -> Tool {
        self.tool_of(&self.ctx.state().active_tool_id)
    }

    pub fn active_tool_id(&self) -> ToolId {
        self.ctx.state().active_tool_id.clone()
    }

    pub fn cursor(&self) -> Cursor {
        self.ctx.state().cursor.clone()
    }

    pub fn tools(&self) -> Vec<Tool> {
        self.hub.tools.borrow().clone()
    }

    // ── tool intents ──

    pub fn activate_tool(&self, id: &str) -> Result<(), Error> {
        if !self.hub.tools.borrow().iter().any(|t| t.id == id) {
            return Err(Error::new(format!("[interaction] unknown tool '{id}'")));
        }
        self.hub.owner.borrow_mut().take();
        // The previous tool's hover claims go; handlers re-claim on hover.
        self.hub.claims.borrow_mut().clear();
        self.ctx.dispatch(InteractionAction::SetTool { tool_id: id.to_string() });
        self.sync_cursor();
        self.hub.tool_cbs.for_each();
        Ok(())
    }

    /// Fires after the active tool changes.
    pub fn on_tool_change(&self, callback: impl Fn() + 'static) -> Unsubscriber {
        self.hub.tool_cbs.add(Rc::new(callback))
    }

    /// `onToolChange` with a callback the caller keeps (so adding it twice
    /// is one subscription, as with a JavaScript function).
    pub fn on_tool_change_rc(&self, callback: Rc<dyn Fn()>) -> Unsubscriber {
        self.hub.tool_cbs.add(callback)
    }

    /// Reskin a tool's cursor keywords; `None` removes the skin.
    pub fn set_tool_cursor(&self, id: &str, skin: Option<ToolCursorSkin>) {
        {
            let mut skins = self.hub.cursor_skins.borrow_mut();
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

    // ── registries ──

    pub fn register_tool(&self, tool: Tool) -> Unregister {
        let id = tool.id.clone();
        set_tool(&mut self.hub.tools.borrow_mut(), tool);
        let hub = self.hub.clone();
        Unregister::new(move || {
            hub.tools.borrow_mut().retain(|t| t.id != id);
            hub.cursor_skins.borrow_mut().retain(|(k, _)| *k != id);
        })
    }

    /// Register a pointer handler; `source` scopes it to one lens.
    pub fn register_handler(&self, handler: Handler, source: Option<String>) -> Unregister {
        self.hub.handlers.borrow_mut().push(handler.clone());
        if let Some(source) = source {
            let mut sources = self.hub.handler_sources.borrow_mut();
            match sources.iter_mut().find(|(k, _)| same(k, &handler)) {
                Some(slot) => slot.1 = source,
                None => sources.push((handler.clone(), source)),
            }
        }
        let hub = self.hub.clone();
        Unregister::new(move || {
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

    // ── cursor claim stack ──

    /// Claim the cursor under `token` (`None` releases it). The TypeScript
    /// priority defaults to 0.
    pub fn set_cursor(&self, token: &str, cursor: Option<&str>, priority: f64) {
        {
            let mut claims = self.hub.claims.borrow_mut();
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

    // ── pointer ingress ──

    /// The touch pre-flight: would an eligible handler claim this contact?
    pub fn would_claim_touch(&self, sample: &PointerSample) -> bool {
        self.eligible(sample.source.as_deref()).iter().any(|h| h.claims_touch(sample))
    }

    pub fn dispatch(&self, sample: &PointerSample) {
        let now_over_page = sample.page.is_some();
        if now_over_page != self.hub.over_page.get() {
            self.hub.over_page.set(now_over_page);
            self.sync_cursor(); // crossing a page edge re-resolves the base cursor
        }
        match sample.phase {
            Phase::Down => {
                self.hub.owner.borrow_mut().take();
                for h in self.eligible(sample.source.as_deref()) {
                    if h.on_down(sample) {
                        *self.hub.owner.borrow_mut() = Some(h);
                        break;
                    }
                }
            }
            Phase::Move => {
                let owner = self.hub.owner.borrow().clone();
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
                // Abort, don't commit; on_cancel falls back to on_up.
                let owner = self.hub.owner.borrow_mut().take();
                if let Some(owner) = owner {
                    owner.on_cancel(sample);
                }
            }
            Phase::Up => {
                let owner = self.hub.owner.borrow().clone();
                if let Some(owner) = owner {
                    owner.on_up(sample);
                }
                self.hub.owner.borrow_mut().take();
            }
        }
    }
}
