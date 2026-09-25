//! `reducer.ts`.

use crate::types::{LinkAction, LinkState};

pub fn initial_link_state() -> LinkState {
    LinkState::default()
}

/// Pure. Engine-backed page cache only: the annotation-backed mode reads
/// the annotation plugin's model and never touches this state. `None` is
/// the TypeScript's `return state`.
pub fn link_reducer(state: &LinkState, action: &LinkAction) -> Option<LinkState> {
    match action {
        LinkAction::SetPage { pon, items } => {
            let mut pages = state.pages.clone();
            pages.insert(*pon, items.clone());
            Some(LinkState { pages })
        }
        LinkAction::DropPage { pon } => {
            if !state.pages.contains_key(pon) {
                return None;
            }
            let mut pages = state.pages.clone();
            pages.remove(pon);
            Some(LinkState { pages })
        }
    }
}
