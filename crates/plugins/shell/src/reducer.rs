//! `reducer.ts`.

use crate::record::JsRecord;
use crate::types::{ShellAction, ShellState, SurfaceProps, SurfaceState};

pub fn initial_shell_state() -> ShellState {
    ShellState { surfaces: JsRecord::new(), open_menus: Vec::new() }
}

/// Close every open surface sharing the exclusivity tag.
fn close_exclusive(
    surfaces: &JsRecord<SurfaceState>,
    exclusive: &str,
    except: &str,
) -> JsRecord<SurfaceState> {
    surfaces
        .iter()
        .map(|(id, s)| {
            let close = s.open && s.exclusive.as_deref() == Some(exclusive) && id != except;
            (
                id.to_string(),
                if close { SurfaceState { open: false, ..s.clone() } } else { s.clone() },
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
        SurfaceState { open: true, exclusive: exclusive.cloned(), props: props.cloned() },
    );
    ShellState { surfaces, ..state.clone() }
}

fn close_surface(state: &ShellState, id: &str) -> Option<ShellState> {
    let existing = state.surfaces.get(id).filter(|s| s.open)?;
    let mut surfaces = state.surfaces.clone();
    surfaces.insert(id, SurfaceState { open: false, ..existing.clone() });
    Some(ShellState { surfaces, ..state.clone() })
}

/// `None` is `return state`.
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
    }
}
