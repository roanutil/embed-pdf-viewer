//! `controller.ts`: reads, verbs that apply the pure transitions in
//! [`model`](crate::model), and the surface and menu events. The events are
//! derived in one place, from each state change, so a new verb can never
//! forget to announce.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, Error};
use serde_json::Value;

use crate::contract::{
    MenuClosedEvent, MenuOpenedEvent, OpenSurfaceOptions, ShellSnapshot, SurfaceClosedEvent,
    SurfaceOpenedEvent, SurfaceProps, SurfaceState,
};
use crate::model::{self, ShellState, SurfaceRecord, Surfaces};
use crate::record::is_inherited;

/// What `state().surfaces[id]` finds: an own record, an `Object.prototype`
/// member (the same object every time), or nothing.
#[derive(Clone)]
enum Lookup {
    Own(Rc<SurfaceRecord>),
    Inherited,
    Missing,
}

impl Lookup {
    /// `Object.is` over the one input.
    fn same(&self, other: &Lookup) -> bool {
        match (self, other) {
            (Lookup::Own(a), Lookup::Own(b)) => Rc::ptr_eq(a, b),
            (Lookup::Inherited, Lookup::Inherited) | (Lookup::Missing, Lookup::Missing) => true,
            _ => false,
        }
    }
}

type SurfaceView = Option<Rc<SurfaceState>>;
type OpenSurfaces = Rc<Vec<Rc<SurfaceState>>>;

/// The identity memos (`memoByKey`, `memo`). Each holds the `Rc` inputs it
/// compares, so an input can't be freed and its address reused while the
/// cache still names it.
#[derive(Default)]
struct Memos {
    /// `surfaceOf`, per id; unbounded, as the TypeScript's has no
    /// `maxEntries`.
    surfaces: HashMap<String, (Lookup, SurfaceView)>,
    open_surfaces: Option<(Rc<Surfaces>, OpenSurfaces)>,
    snapshot: Option<(Rc<ShellState>, Rc<ShellSnapshot>)>,
}

struct Inner {
    cx: Controller<ShellState, Value>,
    surface_opened: EventHookSource<SurfaceOpenedEvent>,
    surface_closed: EventHookSource<SurfaceClosedEvent>,
    menu_opened: EventHookSource<MenuOpenedEvent>,
    menu_closed: EventHookSource<MenuClosedEvent>,
    memos: RefCell<Memos>,
}

/// `ShellCapability`: which surfaces (panels, dialogs) and menus of one
/// document are open. The app draws them; the plugin only holds their
/// state.
#[derive(Clone)]
pub struct ShellCapability(Rc<Inner>);

impl ShellCapability {
    fn state(&self) -> Rc<ShellState> {
        self.0.cx.state()
    }

    /// `ctx.state.update(transition, ...args)`.
    fn update(&self, transition: impl FnOnce(&Rc<ShellState>) -> Rc<ShellState>) {
        self.0.cx.update_state(|state| transition(&state));
    }

    /// Aborted when the document closes. The foreign handle, which can
    /// outlive the close, checks it before every call and ends its event
    /// streams when it aborts.
    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }

    /// `surfaceOf`: a surface's public shape, the same object until its
    /// record changes.
    fn surface_of(&self, id: &str) -> SurfaceView {
        let state = self.state();
        let lookup = match state.surfaces.get(id) {
            Some(record) => Lookup::Own(record.clone()),
            None if is_inherited(id) => Lookup::Inherited,
            None => Lookup::Missing,
        };
        if let Some((inputs, result)) = self.0.memos.borrow().surfaces.get(id)
            && inputs.same(&lookup)
        {
            return result.clone();
        }
        let result = match &lookup {
            Lookup::Own(record) => {
                Some(Rc::new(SurfaceState { id: id.to_string(), record: Some(record.clone()) }))
            }
            // `{ id, ...fn }`: a function has no own enumerable members.
            Lookup::Inherited => Some(Rc::new(SurfaceState { id: id.to_string(), record: None })),
            Lookup::Missing => None,
        };
        self.0.memos.borrow_mut().surfaces.insert(id.to_string(), (lookup, result.clone()));
        result
    }

    // ── surfaces ──

    /// Whether a surface is open.
    pub fn is_open(&self, id: &str) -> bool {
        self.state().surfaces.get(id).is_some_and(|surface| surface.open)
    }

    /// Open flag, exclusivity tag, props and focus request; `None` for a
    /// surface never opened. The same `Rc` until the surface's record
    /// changes. An id naming an `Object.prototype` member is found, as a
    /// JavaScript record lookup finds it, and is `{ id }` alone.
    pub fn get_surface(&self, id: &str) -> Option<Rc<SurfaceState>> {
        self.surface_of(id)
    }

    /// The open surfaces, in JavaScript property order. The same `Rc` while
    /// no surface record changes.
    pub fn list_open_surfaces(&self) -> OpenSurfaces {
        let surfaces = self.state().surfaces.clone();
        if let Some((inputs, result)) = &self.0.memos.borrow().open_surfaces
            && Rc::ptr_eq(inputs, &surfaces)
        {
            return result.clone();
        }
        let result: OpenSurfaces = Rc::new(
            surfaces
                .iter()
                .filter(|(_, surface)| surface.open)
                .filter_map(|(id, _)| self.surface_of(id))
                .collect(),
        );
        self.0.memos.borrow_mut().open_surfaces = Some((surfaces, result.clone()));
        result
    }

    /// Open a surface. `exclusive` closes the other open surfaces of that
    /// group, `props` travel with it, and `focus` raises its
    /// `focusRequest`, also when it is open already. Fires
    /// `onSurfaceOpened` when it was closed, and `onSurfaceClosed` for each
    /// surface it closes.
    pub fn open(&self, id: &str, options: Option<OpenSurfaceOptions>) {
        let options = options.unwrap_or_default();
        self.update(|state| model::open_surface(state, id, &options));
    }

    /// Close a surface. Fires `onSurfaceClosed` when it was open.
    pub fn close(&self, id: &str) {
        self.update(|state| model::close_surface(state, id));
    }

    /// Open a closed surface, with `options` as `open` takes them, or close
    /// an open one.
    pub fn toggle(&self, id: &str, options: Option<OpenSurfaceOptions>) {
        let options = options.unwrap_or_default();
        self.update(|state| model::toggle_surface(state, id, &options));
    }

    /// Change a surface's props without reopening it.
    pub fn update_surface_props(&self, id: &str, props: SurfaceProps) {
        self.update(|state| model::set_surface_props(state, id, &props));
    }

    /// Close every surface.
    pub fn close_all(&self) {
        self.update(model::close_all_surfaces);
    }

    // ── menus ──

    /// Whether a menu is open.
    pub fn is_menu_open(&self, id: &str) -> bool {
        self.state().open_menus.iter().any(|menu| menu == id)
    }

    /// The open menus, in the order they opened (the last one on top). The
    /// same `Rc` while none opens or closes.
    pub fn list_open_menus(&self) -> Rc<Vec<String>> {
        self.state().open_menus.clone()
    }

    /// Open a menu, leaving the others open. Fires `onMenuOpened`.
    pub fn open_menu(&self, id: &str) {
        self.update(|state| model::open_menu(state, id));
    }

    /// Close a menu. Fires `onMenuClosed` when it was open.
    pub fn close_menu(&self, id: &str) {
        self.update(|state| model::close_menu(state, id));
    }

    /// Open a closed menu, or close an open one.
    pub fn toggle_menu(&self, id: &str) {
        self.update(|state| model::toggle_menu(state, id));
    }

    /// Close every menu, for a click outside or Escape.
    pub fn close_all_menus(&self) {
        self.update(model::close_all_menus);
    }

    // ── persistence ──

    /// Everything the plugin holds, as plain data you can store;
    /// `focusRequest` is left out. The same `Rc` while the state is.
    pub fn get_snapshot(&self) -> Rc<ShellSnapshot> {
        let state = self.state();
        if let Some((inputs, result)) = &self.0.memos.borrow().snapshot
            && Rc::ptr_eq(inputs, &state)
        {
            return result.clone();
        }
        let result = Rc::new(model::snapshot_of(&state));
        self.0.memos.borrow_mut().snapshot = Some((state, result.clone()));
        result
    }

    /// Put back what `get_snapshot()` returned, every `focusRequest` at 0.
    /// Fires the events of what opens and closes.
    pub fn apply_snapshot(&self, snapshot: &ShellSnapshot) {
        self.update(|state| model::apply_snapshot(state, snapshot));
    }

    // ── events ──

    /// A surface opened, by `open`, `toggle` or `apply_snapshot`.
    pub fn on_surface_opened(
        &self,
        listener: impl Fn(&SurfaceOpenedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.surface_opened.on(listener)
    }

    /// A surface closed, by `close`, `toggle`, `close_all`, an `exclusive`
    /// open or `apply_snapshot`.
    pub fn on_surface_closed(
        &self,
        listener: impl Fn(&SurfaceClosedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.surface_closed.on(listener)
    }

    /// A menu opened.
    pub fn on_menu_opened(
        &self,
        listener: impl Fn(&MenuOpenedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.menu_opened.on(listener)
    }

    /// A menu closed.
    pub fn on_menu_closed(
        &self,
        listener: impl Fn(&MenuClosedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.menu_closed.on(listener)
    }
}

/// The events of one state change: a surface event per id whose open flag
/// changed (`previous`'s ids, then `next`'s new ones), then a closed event
/// per menu that left the stack, then an opened event per menu that joined.
fn announce(
    previous: &ShellState,
    next: &ShellState,
    surface_opened: &EventHookSource<SurfaceOpenedEvent>,
    surface_closed: &EventHookSource<SurfaceClosedEvent>,
    menu_opened: &EventHookSource<MenuOpenedEvent>,
    menu_closed: &EventHookSource<MenuClosedEvent>,
) {
    if !Rc::ptr_eq(&previous.surfaces, &next.surfaces) {
        let mut ids: Vec<&str> = previous.surfaces.keys().collect();
        for id in next.surfaces.keys() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for id in ids {
            let was_open = previous.surfaces.get(id).is_some_and(|surface| surface.open);
            let is_open = next.surfaces.get(id).is_some_and(|surface| surface.open);
            if was_open == is_open {
                continue;
            }
            if is_open {
                let props = next.surfaces.get(id).and_then(|surface| surface.props.clone());
                surface_opened.emit(&SurfaceOpenedEvent { id: id.to_string(), props });
            } else {
                surface_closed.emit(&SurfaceClosedEvent { id: id.to_string() });
            }
        }
    }
    if !Rc::ptr_eq(&previous.open_menus, &next.open_menus) {
        for id in previous.open_menus.iter() {
            if !next.open_menus.contains(id) {
                menu_closed.emit(&MenuClosedEvent { id: id.clone() });
            }
        }
        for id in next.open_menus.iter() {
            if !previous.open_menus.contains(id) {
                menu_opened.emit(&MenuOpenedEvent { id: id.clone() });
            }
        }
    }
}

/// `createShellController(ctx)`: the api; the shell has no `connect`.
pub fn create_shell_controller(cx: Controller<ShellState, Value>) -> ShellCapability {
    let surface_opened = cx.event_source::<SurfaceOpenedEvent>();
    let surface_closed = cx.event_source::<SurfaceClosedEvent>();
    let menu_opened = cx.event_source::<MenuOpenedEvent>();
    let menu_closed = cx.event_source::<MenuClosedEvent>();
    {
        let (surface_opened, surface_closed, menu_opened, menu_closed) = (
            surface_opened.clone(),
            surface_closed.clone(),
            menu_opened.clone(),
            menu_closed.clone(),
        );
        cx.on_state_change(move |next, previous| {
            announce(
                &previous,
                &next,
                &surface_opened,
                &surface_closed,
                &menu_opened,
                &menu_closed,
            );
            Ok(())
        });
    }
    ShellCapability(Rc::new(Inner {
        cx,
        surface_opened,
        surface_closed,
        menu_opened,
        menu_closed,
        memos: RefCell::default(),
    }))
}
