//! `reducer.ts`.

use crate::types::{SelectionAction, SelectionState};

pub fn initial_selection_state() -> SelectionState {
    SelectionState::default()
}

/// Pure. `None` is the TypeScript `return state`.
pub fn selection_reducer(
    state: &SelectionState,
    action: &SelectionAction,
) -> Option<SelectionState> {
    match action {
        SelectionAction::PageLoaded { pon } => {
            if state.loaded.get(pon).copied().unwrap_or(false) {
                return None;
            }
            let mut loaded = state.loaded.clone();
            loaded.insert(*pon, true);
            Some(SelectionState { loaded, ..state.clone() })
        }
        SelectionAction::Set { selection, segments } => Some(SelectionState {
            selection: Some(*selection),
            segments: segments.clone(),
            ..state.clone()
        }),
        SelectionAction::Clear => {
            if state.selection.is_none() && state.segments.is_empty() {
                return None;
            }
            Some(SelectionState { selection: None, segments: Default::default(), ..state.clone() })
        }
        SelectionAction::SetHighlightHidden { hidden } => (state.highlight_hidden != *hidden)
            .then(|| SelectionState { highlight_hidden: *hidden, ..state.clone() }),
        SelectionAction::SetSelecting { selecting } => (state.selecting != *selecting)
            .then(|| SelectionState { selecting: *selecting, ..state.clone() }),
    }
}
