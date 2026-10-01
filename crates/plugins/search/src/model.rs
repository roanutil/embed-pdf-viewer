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
    /// Each hit shared: a page's results add pointers, not copies of every
    /// hit so far (a hit holds its segments and snippet).
    pub hits: Rc<Vec<Rc<SearchHit>>>,
    /// Page object number to the hits on that page. A bucket is replaced
    /// only when its page gains hits. The TypeScript's record orders integer
    /// keys ascending, as a `BTreeMap` does.
    pub hits_by_page: Rc<BTreeMap<PageObjectNumber, Rc<Vec<Rc<SearchHit>>>>>,
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
    Append { hits: Vec<Rc<SearchHit>>, scanned: f64, total: f64 },
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
            // A page's bucket is replaced once per page these hits touch,
            // in their order, not once per hit.
            let mut hits_by_page = (*state.hits_by_page).clone();
            let mut start = 0;
            while start < added.len() {
                let page = added[start].page.page_object_number;
                let end = added[start..]
                    .iter()
                    .position(|hit| hit.page.page_object_number != page)
                    .map_or(added.len(), |n| start + n);
                let bucket = hits_by_page.entry(page).or_default();
                let mut next = Vec::with_capacity(bucket.len() + end - start);
                next.extend(bucket.iter().cloned());
                next.extend(added[start..end].iter().cloned());
                *bucket = Rc::new(next);
                start = end;
            }
            let mut hits = Vec::with_capacity(state.hits.len() + added.len());
            hits.extend(state.hits.iter().cloned());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: u32, char_start: f64) -> Rc<SearchHit> {
        Rc::new(SearchHit {
            page: PageRef { page_object_number: page },
            page_index: page as usize,
            char_start,
            char_count: 3.0,
            segments: Vec::new(),
            bounds: None,
            snippet: None,
        })
    }

    fn append(state: &SearchState, hits: Vec<Rc<SearchHit>>) -> SearchState {
        reduce_search(state, &SearchAction::Append { hits, scanned: 1.0, total: 2.0 }).unwrap()
    }

    fn starts(hits: &[Rc<SearchHit>]) -> Vec<f64> {
        hits.iter().map(|h| h.char_start).collect()
    }

    #[test]
    fn an_append_keeps_order_in_the_list_and_in_each_page_bucket() {
        let state = append(&initial_search_state(), vec![hit(7, 0.0), hit(7, 1.0)]);
        // Page 9 between two runs of page 7: page 7's bucket gets both.
        let state = append(&state, vec![hit(7, 2.0), hit(9, 0.0), hit(7, 3.0)]);
        assert_eq!(starts(&state.hits), vec![0.0, 1.0, 2.0, 0.0, 3.0]);
        assert_eq!(starts(&state.hits_by_page[&7]), vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(starts(&state.hits_by_page[&9]), vec![0.0]);
        assert_eq!(state.active_index, 0);
    }

    #[test]
    fn an_append_shares_the_hits_it_already_had() {
        let before = append(&initial_search_state(), vec![hit(7, 0.0)]);
        let after = append(&before, vec![hit(9, 0.0)]);
        assert!(Rc::ptr_eq(&before.hits[0], &after.hits[0]));
        assert!(Rc::ptr_eq(&before.hits_by_page[&7], &after.hits_by_page[&7]));
    }
}
