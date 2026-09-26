//! `model.ts`: the search slice and its pure reducer.

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};

use crate::contract::*;

/// `SearchState`. `hits` and each page bucket are shared with every state
/// that did not change them, as the TypeScript's spreads share them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchState {
    /// The stored search intent; `None` when idle.
    pub query: Option<SearchQuery>,
    pub status: SearchStatus,
    pub operation_id: Option<String>,
    pub hits: Rc<Vec<SearchHit>>,
    /// Page object number to the hits on that page. A bucket is replaced
    /// only when its page gains hits. The TypeScript's record orders integer
    /// keys ascending, as a `BTreeMap` does.
    pub hits_by_page: Rc<BTreeMap<PageObjectNumber, Rc<Vec<SearchHit>>>>,
    /// Index into `hits`, `-1` for none.
    pub active_index: isize,
    pub progress: SearchProgress,
    pub error: Option<PluginErrorInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SearchAction {
    #[serde(rename = "START", rename_all = "camelCase")]
    Start { query: SearchQuery, operation_id: String },
    #[serde(rename = "APPEND")]
    Append { hits: Vec<SearchHit>, scanned: f64, total: f64 },
    #[serde(rename = "COMPLETE")]
    Complete,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "ERROR")]
    Error { error: PluginErrorInfo },
    #[serde(rename = "SET_ACTIVE")]
    SetActive { index: isize },
    #[serde(rename = "CLEAR")]
    Clear,
}

/// `initialSearchState()`.
pub fn initial_search_state() -> SearchState {
    SearchState {
        query: None,
        status: SearchStatus::Idle,
        operation_id: None,
        hits: Rc::new(Vec::new()),
        hits_by_page: Rc::new(BTreeMap::new()),
        active_index: -1,
        progress: SearchProgress { scanned: 0.0, total: 0.0 },
        error: None,
    }
}

/// `reduceSearch`: pure. `None` is the TypeScript's `return state`, which
/// only `SET_ACTIVE` to the index already active takes.
pub fn reduce_search(state: &SearchState, action: &SearchAction) -> Option<SearchState> {
    Some(match action {
        SearchAction::Start { query, operation_id } => SearchState {
            query: Some(query.clone()),
            status: SearchStatus::Searching,
            operation_id: Some(operation_id.clone()),
            ..initial_search_state()
        },
        SearchAction::Append { hits: added, scanned, total } => {
            let progress = SearchProgress { scanned: *scanned, total: *total };
            if added.is_empty() {
                return Some(SearchState { progress, ..state.clone() });
            }
            let mut hits_by_page = (*state.hits_by_page).clone();
            for hit in added {
                let bucket = hits_by_page.entry(hit.page.page_object_number).or_default();
                let mut next = (**bucket).clone();
                next.push(hit.clone());
                *bucket = Rc::new(next);
            }
            let mut hits = (*state.hits).clone();
            hits.extend(added.iter().cloned());
            SearchState {
                hits: Rc::new(hits),
                hits_by_page: Rc::new(hits_by_page),
                // The first hit becomes active so "1/N" reads right;
                // navigation, not the stream, moves the camera.
                active_index: if state.active_index == -1 { 0 } else { state.active_index },
                progress,
                ..state.clone()
            }
        }
        SearchAction::Complete => SearchState { status: SearchStatus::Complete, ..state.clone() },
        SearchAction::Cancelled => SearchState { status: SearchStatus::Cancelled, ..state.clone() },
        SearchAction::Error { error } => {
            SearchState { status: SearchStatus::Error, error: Some(error.clone()), ..state.clone() }
        }
        SearchAction::SetActive { index } => {
            if state.active_index == *index {
                return None;
            }
            SearchState { active_index: *index, ..state.clone() }
        }
        SearchAction::Clear => initial_search_state(),
    })
}

/// `pagesWithHits(state)`: the first hit's page of every non-empty bucket,
/// in key order.
pub fn pages_with_hits(state: &SearchState) -> Vec<PageRef> {
    state.hits_by_page.values().filter_map(|hits| hits.first()).map(|hit| hit.page).collect()
}
