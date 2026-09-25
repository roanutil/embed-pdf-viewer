//! `capability.ts`: selectors (pure reads) and intents (dispatch). No UI,
//! no engine.

use epdf_kernel::Ctx;

use crate::types::{OpenSurfaceOptions, ShellAction, ShellState, SurfaceProps};

#[derive(Clone)]
pub struct ShellCapability {
    ctx: Ctx<ShellState, ShellAction>,
}

impl ShellCapability {
    pub(crate) fn new(ctx: Ctx<ShellState, ShellAction>) -> Self {
        ShellCapability { ctx }
    }

    // ── selectors ──

    pub fn is_open(&self, id: &str) -> bool {
        self.ctx.state().surfaces.get(id).is_some_and(|s| s.open)
    }

    pub fn surface_props(&self, id: &str) -> Option<SurfaceProps> {
        self.ctx.state().surfaces.get(id).and_then(|s| s.props.clone())
    }

    /// Ids of all open surfaces.
    pub fn open_surfaces(&self) -> Vec<String> {
        self.ctx
            .state()
            .surfaces
            .iter()
            .filter(|(_, s)| s.open)
            .map(|(id, _)| id.to_string())
            .collect()
    }

    pub fn open_menus(&self) -> Vec<String> {
        self.ctx.state().open_menus.clone()
    }

    pub fn is_menu_open(&self, id: &str) -> bool {
        self.ctx.state().open_menus.iter().any(|m| m == id)
    }

    // ── intents ──

    pub fn open(&self, id: &str, opts: Option<OpenSurfaceOptions>) {
        let opts = opts.unwrap_or_default();
        self.ctx.dispatch(ShellAction::OpenSurface {
            id: id.to_string(),
            exclusive: opts.exclusive,
            props: opts.props,
        });
    }

    pub fn close(&self, id: &str) {
        self.ctx.dispatch(ShellAction::CloseSurface { id: id.to_string() });
    }

    pub fn toggle(&self, id: &str, opts: Option<OpenSurfaceOptions>) {
        let opts = opts.unwrap_or_default();
        self.ctx.dispatch(ShellAction::ToggleSurface {
            id: id.to_string(),
            exclusive: opts.exclusive,
            props: opts.props,
        });
    }

    pub fn open_menu(&self, id: &str) {
        self.ctx.dispatch(ShellAction::OpenMenu { id: id.to_string() });
    }

    pub fn close_menu(&self, id: &str) {
        self.ctx.dispatch(ShellAction::CloseMenu { id: id.to_string() });
    }

    pub fn toggle_menu(&self, id: &str) {
        let action = if self.ctx.state().open_menus.iter().any(|m| m == id) {
            ShellAction::CloseMenu { id: id.to_string() }
        } else {
            ShellAction::OpenMenu { id: id.to_string() }
        };
        self.ctx.dispatch(action);
    }

    pub fn close_all_menus(&self) {
        self.ctx.dispatch(ShellAction::CloseAllMenus);
    }
}
