//! `reducer.ts`.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::types::{SearchAction, SearchProgress, SearchState, SearchStatus};

pub fn initial_search_state() -> SearchState {
    SearchState {
        query: None,
        status: SearchStatus::Idle,
        hits: Rc::new(Vec::new()),
        hits_by_page: Rc::new(BTreeMap::new()),
        active_index: -1,
        progress: SearchProgress { scanned: 0.0, total: 0.0 },
        error: None,
    }
}

/// Pure. Every arm builds a new state; `None` (the TypeScript's `return
/// state`) is only for an action outside the union, which the enum rules
/// out.
pub fn search_reducer(state: &SearchState, action: &SearchAction) -> Option<SearchState> {
    Some(match action {
        SearchAction::Start { query } => SearchState {
            query: Some(query.clone()),
            status: SearchStatus::Searching,
            ..initial_search_state()
        },
        SearchAction::Append { hits: added, scanned, total } => {
            let progress = SearchProgress { scanned: *scanned, total: *total };
            if added.is_empty() {
                return Some(SearchState { progress, ..state.clone() });
            }
            let mut hits = (*state.hits).clone();
            hits.extend(added.iter().cloned());
            let mut hits_by_page = (*state.hits_by_page).clone();
            for (i, hit) in hits.iter().enumerate().skip(state.hits.len()) {
                hits_by_page.entry(hit.pon).or_default().push(i);
            }
            SearchState {
                hits: Rc::new(hits),
                hits_by_page: Rc::new(hits_by_page),
                // The first hit becomes active (so "1/N" reads right);
                // navigation, not the stream, moves the camera.
                active_index: if state.active_index == -1 { 0 } else { state.active_index },
                progress,
                ..state.clone()
            }
        }
        SearchAction::Complete => SearchState { status: SearchStatus::Complete, ..state.clone() },
        SearchAction::Error { message } => SearchState {
            status: SearchStatus::Error,
            error: Some(message.clone()),
            ..state.clone()
        },
        SearchAction::SetActive { index } => SearchState { active_index: *index, ..state.clone() },
        SearchAction::Clear => initial_search_state(),
    })
}
