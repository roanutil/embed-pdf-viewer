//! `model.ts`: the shell slice, which surfaces are open (with their tag and
//! props) and the menu stack.

use serde::{Deserialize, Serialize};

use crate::contract::{ShellSnapshot, SurfaceProps};
use crate::record::JsRecord;

/// One surface's record (the id is the key).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceRecord {
    pub open: bool,
    /// The exclusivity tag the surface was opened with, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclusive: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub props: Option<SurfaceProps>,
}

/// `ShellState`: the same shape as the snapshot it hands out.
pub type ShellState = ShellSnapshot;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ShellAction {
    #[serde(rename = "SHELL/OPEN_SURFACE")]
    OpenSurface {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<SurfaceProps>,
    },
    #[serde(rename = "SHELL/CLOSE_SURFACE")]
    CloseSurface { id: String },
    #[serde(rename = "SHELL/TOGGLE_SURFACE")]
    ToggleSurface {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        exclusive: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        props: Option<SurfaceProps>,
    },
    #[serde(rename = "SHELL/SET_SURFACE_PROPS")]
    SetSurfaceProps { id: String, props: SurfaceProps },
    #[serde(rename = "SHELL/CLOSE_ALL_SURFACES")]
    CloseAllSurfaces,
    #[serde(rename = "SHELL/OPEN_MENU")]
    OpenMenu { id: String },
    #[serde(rename = "SHELL/CLOSE_MENU")]
    CloseMenu { id: String },
    #[serde(rename = "SHELL/CLOSE_ALL_MENUS")]
    CloseAllMenus,
    #[serde(rename = "SHELL/APPLY_SNAPSHOT")]
    ApplySnapshot { snapshot: ShellSnapshot },
}

/// `initialShellState()`.
pub fn initial_shell_state() -> ShellState {
    ShellState { surfaces: JsRecord::new(), open_menus: Vec::new() }
}

/// Close every open surface sharing the exclusivity tag.
fn close_exclusive(
    surfaces: &JsRecord<SurfaceRecord>,
    exclusive: &str,
    except: &str,
) -> JsRecord<SurfaceRecord> {
    surfaces
        .iter()
        .map(|(id, s)| {
            let close = s.open && s.exclusive.as_deref() == Some(exclusive) && id != except;
            (
                id.to_string(),
                if close { SurfaceRecord { open: false, ..s.clone() } } else { s.clone() },
            )
        })
        .collect()
}

fn open_surface(
    state: &ShellState,
    id: &str,
    exclusive: Option<&String>,
    props: Option<&SurfaceProps>,
) -> ShellState {
    // `exclusive ? ... : ...`: the empty tag is falsy.
    let mut surfaces = match exclusive.filter(|e| !e.is_empty()) {
        Some(exclusive) => close_exclusive(&state.surfaces, exclusive, id),
        None => state.surfaces.clone(),
    };
    surfaces.insert(
        id,
        SurfaceRecord { open: true, exclusive: exclusive.cloned(), props: props.cloned() },
    );
    ShellState { surfaces, ..state.clone() }
}

fn close_surface(state: &ShellState, id: &str) -> Option<ShellState> {
    let existing = state.surfaces.get(id).filter(|s| s.open)?;
    let mut surfaces = state.surfaces.clone();
    surfaces.insert(id, SurfaceRecord { open: false, ..existing.clone() });
    Some(ShellState { surfaces, ..state.clone() })
}

/// `shellReducer(state, action)`. `None` is `return state`.
pub fn shell_reducer(state: &ShellState, action: &ShellAction) -> Option<ShellState> {
    match action {
        ShellAction::OpenSurface { id, exclusive, props } => {
            Some(open_surface(state, id, exclusive.as_ref(), props.as_ref()))
        }
        ShellAction::CloseSurface { id } => close_surface(state, id),
        ShellAction::ToggleSurface { id, exclusive, props } => {
            if state.surfaces.get(id).is_some_and(|s| s.open) {
                close_surface(state, id)
            } else {
                Some(open_surface(state, id, exclusive.as_ref(), props.as_ref()))
            }
        }
        ShellAction::SetSurfaceProps { id, props } => {
            let existing = state.surfaces.get(id)?;
            let mut surfaces = state.surfaces.clone();
            surfaces.insert(id, SurfaceRecord { props: Some(props.clone()), ..existing.clone() });
            Some(ShellState { surfaces, ..state.clone() })
        }
        ShellAction::CloseAllSurfaces => {
            if !state.surfaces.iter().any(|(_, s)| s.open) {
                return None;
            }
            let surfaces = state
                .surfaces
                .iter()
                .map(|(id, s)| (id.to_string(), SurfaceRecord { open: false, ..s.clone() }))
                .collect();
            Some(ShellState { surfaces, ..state.clone() })
        }
        ShellAction::OpenMenu { id } => {
            if state.open_menus.contains(id) {
                return None;
            }
            let mut open_menus = state.open_menus.clone();
            open_menus.push(id.clone());
            Some(ShellState { open_menus, ..state.clone() })
        }
        ShellAction::CloseMenu { id } => {
            if !state.open_menus.contains(id) {
                return None;
            }
            let open_menus = state.open_menus.iter().filter(|m| *m != id).cloned().collect();
            Some(ShellState { open_menus, ..state.clone() })
        }
        ShellAction::CloseAllMenus => {
            if state.open_menus.is_empty() {
                None
            } else {
                Some(ShellState { open_menus: Vec::new(), ..state.clone() })
            }
        }
        ShellAction::ApplySnapshot { snapshot } => Some(snapshot.clone()),
    }
}
