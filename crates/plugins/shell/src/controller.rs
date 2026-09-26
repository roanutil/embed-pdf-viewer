//! `controller.ts`: selectors (pure reads), intents (dispatch), and the one
//! place the surface and menu events fire. Every write goes through a
//! diffing dispatch, so an open or close can never be forgotten by a new
//! verb.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::{Controller, Error};

use crate::contract::{
    MenuEvent, OpenSurfaceOptions, ShellSnapshot, SurfaceEvent, SurfaceProps, SurfaceState,
};
use crate::model::{ShellAction, ShellState};
use crate::record::is_inherited;

struct Inner {
    cx: Controller<ShellState, ShellAction>,
    surface_opened: EventHookSource<SurfaceEvent>,
    surface_closed: EventHookSource<SurfaceEvent>,
    menu_opened: EventHookSource<MenuEvent>,
    menu_closed: EventHookSource<MenuEvent>,
}

/// `ShellCapability` (and `ShellHostCapability`, which equals it today).
#[derive(Clone)]
pub struct ShellCapability(Rc<Inner>);

impl ShellCapability {
    fn state(&self) -> Rc<ShellState> {
        self.0.cx.state()
    }

    /// `emitDiffs(before, after)`: an event per surface whose open flag
    /// changed (ids of `before`, then new ids of `after`), then per menu
    /// closed, then per menu opened.
    fn emit_diffs(&self, before: &ShellState, after: &ShellState) {
        let mut ids: Vec<&str> = before.surfaces.keys().collect();
        for id in after.surfaces.keys() {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        for id in ids {
            let was = before.surfaces.get(id).is_some_and(|s| s.open);
            let now = after.surfaces.get(id).is_some_and(|s| s.open);
            if was == now {
                continue;
            }
            let event = SurfaceEvent {
                id: id.to_string(),
                props: after.surfaces.get(id).and_then(|s| s.props.clone()),
            };
            if now {
                self.0.surface_opened.emit(&event);
            } else {
                self.0.surface_closed.emit(&event);
            }
        }
        for id in &before.open_menus {
            if !after.open_menus.contains(id) {
                self.0.menu_closed.emit(&MenuEvent { id: id.clone() });
            }
        }
        for id in &after.open_menus {
            if !before.open_menus.contains(id) {
                self.0.menu_opened.emit(&MenuEvent { id: id.clone() });
            }
        }
    }

    fn dispatch(&self, action: ShellAction) {
        let before = self.state();
        self.0.cx.dispatch(action);
        let after = self.state();
        if !Rc::ptr_eq(&before, &after) {
            self.emit_diffs(&before, &after);
        }
    }

    // ── surfaces ──

    pub fn is_open(&self, id: &str) -> bool {
        self.state().surfaces.get(id).is_some_and(|s| s.open)
    }

    /// Open flag, exclusivity tag and props; `None` for a surface never
    /// opened. An id naming an `Object.prototype` member is found, as a
    /// JavaScript record lookup finds it, and has no flag, tag or props.
    pub fn get_surface(&self, id: &str) -> Option<SurfaceState> {
        let state = self.state();
        let Some(record) = state.surfaces.get(id) else {
            return is_inherited(id).then(|| SurfaceState {
                id: id.to_string(),
                open: None,
                exclusive: None,
                props: None,
            });
        };
        Some(SurfaceState {
            id: id.to_string(),
            open: Some(record.open),
            exclusive: record.exclusive.clone(),
            props: record.props.clone(),
        })
    }

    /// Open surfaces, in JavaScript property order.
    pub fn list_open_surfaces(&self) -> Vec<SurfaceState> {
        let state = self.state();
        state
            .surfaces
            .iter()
            .filter(|(_, s)| s.open)
            .map(|(id, s)| SurfaceState {
                id: id.to_string(),
                open: Some(s.open),
                exclusive: s.exclusive.clone(),
                props: s.props.clone(),
            })
            .collect()
    }

    pub fn open(&self, id: &str, options: Option<OpenSurfaceOptions>) {
        let options = options.unwrap_or_default();
        self.dispatch(ShellAction::OpenSurface {
            id: id.to_string(),
            exclusive: options.exclusive,
            props: options.props,
        });
    }

    pub fn close(&self, id: &str) {
        self.dispatch(ShellAction::CloseSurface { id: id.to_string() });
    }

    pub fn toggle(&self, id: &str, options: Option<OpenSurfaceOptions>) {
        let options = options.unwrap_or_default();
        self.dispatch(ShellAction::ToggleSurface {
            id: id.to_string(),
            exclusive: options.exclusive,
            props: options.props,
        });
    }

    /// Change a surface's props without reopening it.
    pub fn update_surface_props(&self, id: &str, props: SurfaceProps) {
        self.dispatch(ShellAction::SetSurfaceProps { id: id.to_string(), props });
    }

    /// Close every surface.
    pub fn close_all(&self) {
        self.dispatch(ShellAction::CloseAllSurfaces);
    }

    // ── menus ──

    pub fn is_menu_open(&self, id: &str) -> bool {
        self.state().open_menus.iter().any(|m| m == id)
    }

    /// Open menus in opening order.
    pub fn list_open_menus(&self) -> Vec<String> {
        self.state().open_menus.clone()
    }

    pub fn open_menu(&self, id: &str) {
        self.dispatch(ShellAction::OpenMenu { id: id.to_string() });
    }

    pub fn close_menu(&self, id: &str) {
        self.dispatch(ShellAction::CloseMenu { id: id.to_string() });
    }

    pub fn toggle_menu(&self, id: &str) {
        let action = if self.is_menu_open(id) {
            ShellAction::CloseMenu { id: id.to_string() }
        } else {
            ShellAction::OpenMenu { id: id.to_string() }
        };
        self.dispatch(action);
    }

    pub fn close_all_menus(&self) {
        self.dispatch(ShellAction::CloseAllMenus);
    }

    // ── persistence ──

    pub fn get_snapshot(&self) -> ShellSnapshot {
        (*self.state()).clone()
    }

    pub fn apply_snapshot(&self, snapshot: ShellSnapshot) {
        self.dispatch(ShellAction::ApplySnapshot { snapshot });
    }

    // ── events ──

    pub fn on_surface_opened(
        &self,
        listener: impl Fn(&SurfaceEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.surface_opened.on(listener)
    }

    pub fn on_surface_closed(
        &self,
        listener: impl Fn(&SurfaceEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.surface_closed.on(listener)
    }

    pub fn on_menu_opened(
        &self,
        listener: impl Fn(&MenuEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.menu_opened.on(listener)
    }

    pub fn on_menu_closed(
        &self,
        listener: impl Fn(&MenuEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.menu_closed.on(listener)
    }
}

/// `createShellController(ctx)`: the api; the shell has no `connect`.
pub fn create_shell_controller(cx: Controller<ShellState, ShellAction>) -> ShellCapability {
    // The TypeScript reports a failing listener to `console.error`.
    let report: Rc<dyn Fn(&Error)> = Rc::new(|error| eprintln!("[shell] listener failed: {error}"));
    let inner = Inner {
        surface_opened: EventHookSource::new(Some(report.clone())),
        surface_closed: EventHookSource::new(Some(report.clone())),
        menu_opened: EventHookSource::new(Some(report.clone())),
        menu_closed: EventHookSource::new(Some(report)),
        cx,
    };
    let hooks = (
        inner.surface_opened.clone(),
        inner.surface_closed.clone(),
        inner.menu_opened.clone(),
        inner.menu_closed.clone(),
    );
    inner.cx.cleanup_fn(move || {
        hooks.0.dispose();
        hooks.1.dispose();
        hooks.2.dispose();
        hooks.3.dispose();
    });
    ShellCapability(Rc::new(inner))
}
