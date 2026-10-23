//! `controller.ts`: the interaction hub.
//!
//! Tools, handlers, the captured-gesture owner and cursor claims are
//! registries here; the state holds only the active tool, the tool stack and
//! the resolved cursor, so UI can react. Registering or removing a tool
//! wakes readers with `notify`.
//!
//! Cursor arbitration, top to bottom: the highest-priority claim (hover
//! feedback), then over a page the tool's declared cursor, then over a gap
//! the tool's `gap_cursor`. The winning keyword is replaced through the
//! active tool's cursors (`set_tool_cursor`) when they name that keyword.
//!
//! No `RefCell` borrow is held across a call into a handler, a tool or a
//! listener: they call back into the hub (`claim_cursor`, `activate_tool`)
//! from inside a dispatch.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

use crate::callbacks::Unregister;
use crate::contract::{
    Cursor, CursorChangedEvent, GestureEvent, InteractionConfig, InteractionHandler,
    InteractionSettings, InteractionSettingsChanged, Phase, PointerKind, PointerSample,
    RegisterHandlerOptions, RegisterToolOptions, Tool, ToolChangedEvent, ToolCursors, ToolId,
    ToolPointerEvent, sample_point_on,
};
use crate::model::{self, InteractionState};
use crate::settings::ToolTable;

struct Claim {
    cursor: Cursor,
    priority: f64,
}

/// A tool's own pointer methods take a press before every handler a plugin registers.
const OWN_POINTER_PRIORITY: f64 = 1000.0;

type Handler = Rc<dyn InteractionHandler>;
/// `listTools`'s memo: the registry version it was built at, and the list.
type ListedTools = Option<(u64, Rc<[Rc<Tool>]>)>;

fn same(a: &Handler, b: &Handler) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

struct Hub {
    cx: Controller<InteractionState, Value>,
    settings: InstanceSettings,
    table: Rc<ToolTable>,
    /// A `Map<ToolId, HostTool>`: insertion order; a replaced id moves to the end.
    tools: RefCell<Vec<Rc<Tool>>>,
    /// The handler each tool with its own pointer methods brings.
    own_handlers: RefCell<Vec<(Rc<Tool>, Handler)>>,
    /// Bumped on every tool registry change: the input of the tool list.
    tools_version: Cell<u64>,
    listed: RefCell<ListedTools>,
    handlers: RefCell<Vec<Handler>>,
    /// A `Map<InteractionHandler, string>`, keyed by identity.
    handler_sources: RefCell<Vec<(Handler, String)>>,
    /// A `Map<string, Claim>`: claiming a token again keeps its place.
    claims: RefCell<Vec<(String, Claim)>>,
    tool_cursors: RefCell<Vec<(ToolId, ToolCursors)>>,
    owner: RefCell<Option<Handler>>,
    /// The gesture `owner` took, as its events report it.
    gesture: RefCell<Option<GestureEvent>>,
    /// Whether the last dispatched sample hit a page: gaps fall back to `gap_cursor`.
    over_page: Cell<bool>,
    /// The tools the `tools` setting registered, removed when it changes.
    setting_tools: RefCell<Vec<Rc<Tool>>>,
    tool_changed: EventHookSource<ToolChangedEvent>,
    gesture_started: EventHookSource<GestureEvent>,
    gesture_ended: EventHookSource<GestureEvent>,
    gesture_cancelled: EventHookSource<GestureEvent>,
    cursor_changed: EventHookSource<CursorChangedEvent>,
}

/// `InteractionHostCapability`: the public `InteractionCapability` plus the
/// host lens (`dispatch_pointer`, `would_claim_touch`, `register_handler`,
/// `claim_cursor`, `has_cursor_claim`, `get_cursor`, `on_cursor_changed`,
/// `active_tool_enables`). Cheap to clone; clones are the same hub.
#[derive(Clone)]
pub struct InteractionCapability(Rc<Hub>);

/// `createInteractionController(ctx, builtinTools)`. `table` is the
/// registration's tool table (see `settings.rs`).
pub(crate) fn create_interaction_controller(
    cx: Controller<InteractionState, Value>,
    builtin_tools: Vec<Rc<Tool>>,
    table: Rc<ToolTable>,
) -> Result<(InteractionCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    let hub = Rc::new(Hub {
        tool_changed: cx.event_source(),
        gesture_started: cx.event_source(),
        gesture_ended: cx.event_source(),
        gesture_cancelled: cx.event_source(),
        cursor_changed: cx.event_source(),
        settings,
        table,
        cx,
        tools: RefCell::default(),
        own_handlers: RefCell::default(),
        tools_version: Cell::new(0),
        listed: RefCell::default(),
        handlers: RefCell::default(),
        handler_sources: RefCell::default(),
        claims: RefCell::default(),
        tool_cursors: RefCell::default(),
        owner: RefCell::default(),
        gesture: RefCell::default(),
        over_page: Cell::new(false),
        setting_tools: RefCell::default(),
    });
    let cap = InteractionCapability(hub.clone());

    let weak = Rc::downgrade(&hub);
    hub.cx.on_state_change(move |next, previous| {
        if previous.cursor != next.cursor
            && let Some(hub) = weak.upgrade()
        {
            hub.cursor_changed.emit(&CursorChangedEvent { cursor: next.cursor.clone() });
        }
        Ok(())
    });

    for tool in builtin_tools {
        cap.add_tool(tool, true)?;
    }
    // The `tools` setting's tools, registered over the built-in ones; a
    // change of the setting swaps them for the new list.
    let setting_tools = cap.get_settings().tools;
    for tool in setting_tools {
        let stored = cap.add_tool(tool, true)?;
        hub.setting_tools.borrow_mut().push(stored);
    }
    let weak = Rc::downgrade(&hub);
    let off = hub.settings.events.on(move |event| {
        let Some(hub) = weak.upgrade() else { return Ok(()) };
        if !event.changed.iter().any(|c| c == "tools") {
            return Ok(());
        }
        let cap = InteractionCapability(hub);
        let previous = std::mem::take(&mut *cap.0.setting_tools.borrow_mut());
        for tool in previous {
            cap.remove_registration(&tool);
        }
        for tool in cap.get_settings().tools {
            let stored = cap.add_tool(tool, true)?;
            cap.0.setting_tools.borrow_mut().push(stored);
        }
        cap.tools_changed();
        Ok(())
    });
    hub.cx.listen(off);

    let default_tool = cap.get_default_tool_id();
    hub.cx.update_state(|state| model::activate_tool(&state, &default_tool));
    cap.sync_cursor();

    // Rust only: handlers hold capabilities that hold this hub, so the
    // handler registries are released when the instance closes, as a
    // garbage collector would reclaim the cycle.
    let weak = Rc::downgrade(&hub);
    hub.cx.defer(Box::new(move || {
        if let Some(hub) = weak.upgrade() {
            hub.release();
        }
        Ok(None)
    }));
    Ok((cap, None))
}

impl Hub {
    fn release(&self) {
        self.handlers.borrow_mut().clear();
        self.handler_sources.borrow_mut().clear();
        self.own_handlers.borrow_mut().clear();
        self.owner.borrow_mut().take();
        self.gesture.borrow_mut().take();
    }

    fn remove_handler(&self, handler: &Handler) {
        {
            let mut handlers = self.handlers.borrow_mut();
            if let Some(index) = handlers.iter().position(|h| same(h, handler)) {
                handlers.remove(index);
            }
        }
        self.handler_sources.borrow_mut().retain(|(h, _)| !same(h, handler));
        let mut owner = self.owner.borrow_mut();
        if owner.as_ref().is_some_and(|o| same(o, handler)) {
            *owner = None;
            self.gesture.borrow_mut().take();
        }
    }

    fn add_handler(&self, handler: Handler, source: Option<String>) {
        self.handlers.borrow_mut().push(handler.clone());
        if let Some(source) = source {
            let mut sources = self.handler_sources.borrow_mut();
            match sources.iter_mut().find(|(h, _)| same(h, &handler)) {
                Some(slot) => slot.1 = source,
                None => sources.push((handler, source)),
            }
        }
    }

    /// `removeTool`: take a tool out of the registry, with the handler its
    /// pointer methods brought.
    fn remove_tool(&self, tool: &Rc<Tool>) {
        self.tools.borrow_mut().retain(|t| t.id != tool.id);
        let handler = {
            let mut own = self.own_handlers.borrow_mut();
            let index = own.iter().position(|(t, _)| Rc::ptr_eq(t, tool));
            index.map(|i| own.remove(i).1)
        };
        if let Some(handler) = handler {
            self.remove_handler(&handler);
        }
    }
}

impl InteractionCapability {
    fn state(&self) -> Rc<InteractionState> {
        self.0.cx.state()
    }

    /// The registered tool, or the neutral stand-in an unregistered id reads as.
    fn tool_of(&self, id: &str) -> Rc<Tool> {
        self.get_tool(id).unwrap_or_else(|| Rc::new(Tool::new(id, "default", [])))
    }

    fn cursor_for(&self, tool_id: &str, keyword: &str) -> Option<Cursor> {
        let cursors = self.0.tool_cursors.borrow();
        let (_, cursors) = cursors.iter().find(|(id, _)| id == tool_id)?;
        cursors.get(keyword).cloned()
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
            return self.cursor_for(&tool.id, &top).unwrap_or(top);
        }
        if !self.0.over_page.get() {
            return tool.gap_cursor.clone().unwrap_or_else(|| "default".to_string());
        }
        self.cursor_for(&tool.id, &tool.cursor).unwrap_or_else(|| tool.cursor.clone())
    }

    fn sync_cursor(&self) {
        let cursor = self.resolve_cursor();
        self.0.cx.update_state(|state| model::set_cursor(&state, &cursor));
    }

    /// `addTool`: put a tool in the registry without waking readers, and
    /// return the stored tool. A taken id is refused unless `replace`.
    fn add_tool(&self, tool: Rc<Tool>, replace: bool) -> Result<Rc<Tool>, Error> {
        let taken = self.get_tool(&tool.id);
        if taken.is_some() && !replace {
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
        if let Some(taken) = taken {
            self.0.remove_tool(&taken);
        }
        self.0.tools.borrow_mut().push(tool.clone());
        if let Some(handler) = own_pointer_handler(&tool) {
            self.0.own_handlers.borrow_mut().push((tool.clone(), handler.clone()));
            self.0.add_handler(handler, None);
        }
        Ok(tool)
    }

    fn tools_changed(&self) {
        self.0.tools_version.set(self.0.tools_version.get() + 1);
        self.0.cx.notify();
    }

    /// `removerOf(tool)()`: remove this registration only, never a later one
    /// with its id.
    fn remove_registration(&self, tool: &Rc<Tool>) {
        if !self.get_tool(&tool.id).is_some_and(|t| Rc::ptr_eq(&t, tool)) {
            return;
        }
        self.0.remove_tool(tool);
        self.0.tool_cursors.borrow_mut().retain(|(id, _)| *id != tool.id);
        self.tools_changed();
    }

    fn remover_of(&self, tool: Rc<Tool>) -> Unregister {
        let hub = Rc::downgrade(&self.0);
        Unregister::new(move || {
            if let Some(hub) = hub.upgrade() {
                InteractionCapability(hub).remove_registration(&tool);
            }
        })
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
                    let own = sources.iter().find(|(k, _)| same(k, h)).map(|(_, s)| s.as_str());
                    own.is_none() || source.is_none() || own == source
                })
                .cloned()
                .collect()
        };
        let mut live: Vec<Handler> = scoped.into_iter().filter(|h| h.enabled_for(&tool)).collect();
        // `sort((left, right) => right.priority - left.priority)`: stable,
        // and a NaN difference compares equal.
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

    /// `arm`: activate a tool through `transition`. Every activation is
    /// announced, including activating the active tool again.
    fn arm(
        &self,
        id: &str,
        transition: impl FnOnce(&Rc<InteractionState>) -> Rc<InteractionState>,
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
        self.0.gesture.borrow_mut().take();
        // The previous tool's hover claims go with it; handlers claim again on hover.
        self.0.claims.borrow_mut().clear();
        self.0.cx.update_state(|state| transition(&state));
        self.sync_cursor();
        self.0.tool_changed.emit(&ToolChangedEvent { tool_id: id.to_string(), previous_tool_id });
        Ok(())
    }

    // ── settings ──────────────────────────────────────────────────────────

    /// The settings in effect: the same tools until a setting changes.
    pub fn get_settings(&self) -> InteractionSettings {
        self.0.table.decode(&self.0.settings.get())
    }

    /// Merge `changes` into the settings, for every document; `tools`
    /// replaces the list whole. Fires `on_settings_changed` once when a
    /// setting changed.
    pub fn update_settings(&self, changes: &InteractionConfig) {
        let store = &self.0.settings.store;
        store.update_settings(&self.0.table.encode(changes));
        self.0.table.prune(store);
    }

    /// Go back to what the app registered, not to the defaults.
    pub fn reset_settings(&self) {
        let store = &self.0.settings.store;
        store.reset_settings();
        self.0.table.prune(store);
    }

    /// Fires once for each call that changed a setting. This instance's
    /// listeners go when its document closes.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&InteractionSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let table = Rc::downgrade(&self.0.table);
        self.0.settings.events.on(move |event| {
            let Some(table) = table.upgrade() else { return Ok(()) };
            listener(&InteractionSettingsChanged {
                settings: table.decode(&event.settings),
                changed: event.changed.clone(),
            })
        })
    }

    // ── public ────────────────────────────────────────────────────────────

    /// The active tool. An id nobody registered reads back as a neutral
    /// tool with no tags.
    pub fn get_active_tool(&self) -> Rc<Tool> {
        self.tool_of(&self.state().active_tool_id)
    }

    pub fn get_active_tool_id(&self) -> ToolId {
        self.state().active_tool_id.clone()
    }

    /// The tool a document opens with: the `defaultTool` setting.
    pub fn get_default_tool_id(&self) -> ToolId {
        self.get_settings().default_tool
    }

    /// Every tool, in the order they were added, the built-in ones first.
    /// The same list until a tool is added or removed.
    pub fn list_tools(&self) -> Rc<[Rc<Tool>]> {
        let version = self.0.tools_version.get();
        if let Some((at, listed)) = &*self.0.listed.borrow()
            && *at == version
        {
            return listed.clone();
        }
        let listed: Rc<[Rc<Tool>]> = self.0.tools.borrow().iter().cloned().collect();
        *self.0.listed.borrow_mut() = Some((version, listed.clone()));
        listed
    }

    pub fn get_tool(&self, id: &str) -> Option<Rc<Tool>> {
        self.0.tools.borrow().iter().find(|t| t.id == id).cloned()
    }

    pub fn has_tool(&self, id: &str) -> bool {
        self.0.tools.borrow().iter().any(|t| t.id == id)
    }

    /// Make a tool the active one, and forget the tools `push_tool` saved.
    /// Fires `on_tool_changed`. Errs `not-found` for an unregistered id.
    pub fn activate_tool(&self, id: &str) -> Result<(), Error> {
        self.arm(id, |state| model::activate_tool(state, id))
    }

    /// Make the default tool the active one. Fires `on_tool_changed`.
    pub fn activate_default_tool(&self) -> Result<(), Error> {
        let id = self.get_default_tool_id();
        self.arm(&id, |state| model::activate_tool(state, &id))
    }

    /// Make a tool the active one for a moment, remembering the active one
    /// for `pop_tool`. Errs `not-found` for an unregistered id.
    pub fn push_tool(&self, id: &str) -> Result<(), Error> {
        self.arm(id, |state| model::push_tool(state, id))
    }

    /// Go back to the tool active before the last `push_tool`; nothing to go
    /// back to does nothing. Errs `not-found` when that tool was removed.
    pub fn pop_tool(&self) -> Result<(), Error> {
        let Some(previous) = self.state().tool_stack.last().cloned() else {
            return Ok(());
        };
        self.arm(&previous, model::pop_tool)
    }

    /// Replace a tool's cursors by name while the app runs; `None` brings
    /// the tool's own back.
    pub fn set_tool_cursor(&self, id: &str, cursors: Option<ToolCursors>) {
        {
            let mut all = self.0.tool_cursors.borrow_mut();
            match cursors {
                None => all.retain(|(k, _)| k != id),
                Some(cursors) => match all.iter_mut().find(|(k, _)| k == id) {
                    Some(slot) => slot.1 = cursors,
                    None => all.push((id.to_string(), cursors)),
                },
            }
        }
        self.sync_cursor();
    }

    /// Add a tool, and return what removes it again. Errs `conflict` for an
    /// id that's taken unless `replace`, which moves the id to the end of
    /// the list; the remover removes this registration only.
    pub fn register_tool(
        &self,
        tool: impl Into<Rc<Tool>>,
        options: RegisterToolOptions,
    ) -> Result<Unregister, Error> {
        let stored = self.add_tool(tool.into(), options.replace)?;
        self.tools_changed();
        Ok(self.remover_of(stored))
    }

    /// A tool became active, including activating the active tool again.
    pub fn on_tool_changed(
        &self,
        listener: impl Fn(&ToolChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.tool_changed.on(listener)
    }

    /// A press began a gesture that a handler took.
    pub fn on_gesture_started(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_started.on(listener)
    }

    /// That gesture ended with a release.
    pub fn on_gesture_ended(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_ended.on(listener)
    }

    /// That gesture was cancelled.
    pub fn on_gesture_cancelled(
        &self,
        listener: impl Fn(&GestureEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.gesture_cancelled.on(listener)
    }

    // ── host ──────────────────────────────────────────────────────────────

    /// Whether the active tool turns on a behavior tag such as `text-select`.
    pub fn active_tool_enables(&self, behavior: &str) -> bool {
        self.get_active_tool().enables(behavior)
    }

    /// Register a pointer handler; `source` scopes it to one lens.
    pub fn register_handler(
        &self,
        handler: Rc<dyn InteractionHandler>,
        options: RegisterHandlerOptions,
    ) -> Unregister {
        self.0.add_handler(handler.clone(), options.source);
        let hub: Weak<Hub> = Rc::downgrade(&self.0);
        Unregister::new(move || {
            if let Some(hub) = hub.upgrade() {
                hub.remove_handler(&handler);
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

    /// Whether a hover claim holds the cursor: the pointer is over something
    /// a click acts on instead of the tool.
    pub fn has_cursor_claim(&self) -> bool {
        !self.0.claims.borrow().is_empty()
    }

    /// The resolved cursor.
    pub fn get_cursor(&self) -> Cursor {
        self.state().cursor.clone()
    }

    /// The resolved cursor changed.
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
            self.sync_cursor(); // crossing a page edge resolves the base cursor again
        }
        match sample.phase {
            Phase::Down => {
                self.0.owner.borrow_mut().take();
                self.0.gesture.borrow_mut().take();
                for handler in self.eligible(sample.source.as_deref()) {
                    if handler.on_down(sample) {
                        let gesture = GestureEvent {
                            tool_id: self.state().active_tool_id.clone(),
                            page: sample.page.map(|hit| hit.page),
                            pointer_type: sample.pointer_type.unwrap_or(PointerKind::Mouse),
                        };
                        *self.0.owner.borrow_mut() = Some(handler);
                        *self.0.gesture.borrow_mut() = Some(gesture.clone());
                        self.0.gesture_started.emit(&gesture);
                        break;
                    }
                }
            }
            Phase::Move => {
                let owner = self.0.owner.borrow().clone();
                match owner {
                    Some(owner) => owner.on_move(sample),
                    None => {
                        for handler in self.eligible(sample.source.as_deref()) {
                            handler.on_hover(sample);
                        }
                    }
                }
            }
            Phase::Up | Phase::Cancel => {
                let captured = self.0.owner.borrow_mut().take();
                let taken = self.0.gesture.borrow_mut().take();
                let (Some(captured), Some(taken)) = (captured, taken) else { return };
                if sample.phase == Phase::Cancel {
                    // Abort, don't commit: on_cancel falls back to on_up.
                    captured.on_cancel(sample);
                    self.0.gesture_cancelled.emit(&taken);
                } else {
                    captured.on_up(sample);
                    self.0.gesture_ended.emit(&taken);
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

/// `ownPointerHandler(tool)`: the handler a tool's own pointer methods
/// bring, or `None` for a tool without them. It is live only while that
/// tool is active, and turns samples into [`ToolPointerEvent`]s in page
/// coordinates. A gesture keeps the page it started on, so a drag that
/// leaves the page goes on in that page's coordinates.
fn own_pointer_handler(tool: &Rc<Tool>) -> Option<Handler> {
    if !tool.handles_pointer() {
        return None;
    }
    Some(Rc::new(OwnPointerHandler {
        id: format!("tool:{}", tool.id),
        tool: tool.clone(),
        home: Cell::new(None),
        last: Cell::new(None),
    }))
}

struct OwnPointerHandler {
    id: String,
    tool: Rc<Tool>,
    home: Cell<Option<PageRef>>,
    last: Cell<Option<ToolPointerEvent>>,
}

fn event_of(
    page: PageRef,
    point: epdf_geometry::Point,
    sample: &PointerSample,
) -> ToolPointerEvent {
    ToolPointerEvent {
        page,
        point,
        modifiers: sample.modifiers,
        pointer_type: sample.pointer_type.unwrap_or(PointerKind::Mouse),
    }
}

impl OwnPointerHandler {
    /// The sample on the gesture's page, or the last event when the source
    /// can't project it there.
    fn follow(&self, sample: &PointerSample) -> Option<ToolPointerEvent> {
        let home = self.home.get()?;
        if let Some(point) = sample_point_on(sample, home) {
            self.last.set(Some(event_of(home, point, sample)));
        }
        self.last.get()
    }

    fn end(&self, sample: &PointerSample) {
        let event = self.follow(sample);
        self.home.set(None);
        self.last.set(None);
        if let (Some(event), Some(up)) = (event, &self.tool.on_pointer_up) {
            up(&event);
        }
    }
}

impl InteractionHandler for OwnPointerHandler {
    fn id(&self) -> &str {
        &self.id
    }
    fn priority(&self) -> f64 {
        OWN_POINTER_PRIORITY
    }
    fn enabled_for(&self, active: &Tool) -> bool {
        std::ptr::eq(active, Rc::as_ptr(&self.tool))
    }
    fn on_down(&self, sample: &PointerSample) -> bool {
        let (Some(hit), Some(down)) = (&sample.page, &self.tool.on_pointer_down) else {
            return false;
        };
        let event = event_of(hit.page, hit.point, sample);
        if !down(&event) {
            return false;
        }
        self.home.set(Some(hit.page));
        self.last.set(Some(event));
        true
    }
    fn on_move(&self, sample: &PointerSample) {
        if let (Some(event), Some(on_move)) = (self.follow(sample), &self.tool.on_pointer_move) {
            on_move(&event);
        }
    }
    fn on_up(&self, sample: &PointerSample) {
        self.end(sample);
    }
    fn on_cancel(&self, sample: &PointerSample) {
        self.end(sample);
    }
    fn on_hover(&self, sample: &PointerSample) {
        if let (Some(hit), Some(hover)) = (&sample.page, &self.tool.on_hover) {
            hover(&event_of(hit.page, hit.point, sample));
        }
    }
}
