//! `model.ts`: the search session (the query, the hits found so far, also
//! grouped by page, the active hit and progress) and its pure named
//! transitions. The controller applies them with `update_state`.
//!
//! Each transition takes the state's `Rc` and returns one: the same `Rc`
//! where the TypeScript returns `state` itself, so the store sees no change.

use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use serde::{Deserialize, Serialize};

use crate::contract::*;

/// One page's hits, or all of them; each hit shared, so a slice adds
/// pointers, not copies of every hit so far.
pub type HitList = Rc<Vec<Rc<SearchHit>>>;

/// `SearchState`. `hits`, each page bucket and `progress` are shared with
/// every state that did not change them, as the TypeScript's spreads share
/// them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchState {
    /// The query being searched, as the engine matches it. `None` when idle.
    pub query: Option<SearchQuery>,
    pub status: SearchStatus,
    /// The run that owns the session, so a run that lost it (superseded,
    /// cleared) leaves it alone.
    pub operation_id: Option<String>,
    pub hits: HitList,
    /// Page object number to the hits on that page; a page's list is
    /// replaced only when it gains hits. The TypeScript's record orders
    /// integer keys ascending, as a `BTreeMap` does.
    pub hits_by_page: Rc<BTreeMap<PageObjectNumber, HitList>>,
    /// Index into `hits`, `-1` when no hit is active.
    pub active_hit_index: isize,
    /// Kept by identity: `onProgressChanged` fires when this `Rc` changes.
    pub progress: Rc<SearchProgress>,
    pub error: Option<PluginErrorInfo>,
}

thread_local! {
    static NO_PROGRESS_RC: Rc<SearchProgress> = Rc::new(SearchProgress::default());
}

/// `NO_PROGRESS`: the progress of no search, one object for every session
/// that hasn't searched a page yet.
pub fn no_progress() -> Rc<SearchProgress> {
    NO_PROGRESS_RC.with(Rc::clone)
}

/// `initialSearchState()`.
pub fn initial_search_state() -> SearchState {
    SearchState {
        query: None,
        status: SearchStatus::Idle,
        operation_id: None,
        hits: Rc::new(Vec::new()),
        hits_by_page: Rc::new(BTreeMap::new()),
        active_hit_index: -1,
        progress: no_progress(),
        error: None,
    }
}

/// `startSession(state, query, operationId)`: a new scan starts, and
/// everything from the previous session is dropped.
pub fn start_session(
    _state: &Rc<SearchState>,
    query: &SearchQuery,
    operation_id: &str,
) -> Rc<SearchState> {
    Rc::new(SearchState {
        query: Some(query.clone()),
        status: SearchStatus::Searching,
        operation_id: Some(operation_id.to_string()),
        ..initial_search_state()
    })
}

/// `appendHits(state, hits, slice)`: a slice of hits arrived. The first hit
/// becomes active so "1 of N" reads right; moving the camera is
/// navigation's job. Progress with the same counts keeps its object, and an
/// empty slice that moves nothing keeps the state.
pub fn append_hits(
    state: &Rc<SearchState>,
    added: &[Rc<SearchHit>],
    slice: SearchProgress,
) -> Rc<SearchState> {
    let progress = if *state.progress == slice { state.progress.clone() } else { Rc::new(slice) };
    if added.is_empty() {
        if Rc::ptr_eq(&progress, &state.progress) {
            return state.clone();
        }
        return Rc::new(SearchState { progress, ..(**state).clone() });
    }
    // A page's list is replaced once per page these hits touch, in their
    // order, not once per hit.
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
    Rc::new(SearchState {
        hits: Rc::new(hits),
        hits_by_page: Rc::new(hits_by_page),
        active_hit_index: if state.active_hit_index == -1 { 0 } else { state.active_hit_index },
        progress,
        ..(**state).clone()
    })
}

/// `completeSession(state)`.
pub fn complete_session(state: &Rc<SearchState>) -> Rc<SearchState> {
    Rc::new(SearchState { status: SearchStatus::Complete, ..(**state).clone() })
}

/// `cancelSession(state)`: the hits found so far stay.
pub fn cancel_session(state: &Rc<SearchState>) -> Rc<SearchState> {
    Rc::new(SearchState { status: SearchStatus::Cancelled, ..(**state).clone() })
}

/// `failSession(state, error)`.
pub fn fail_session(state: &Rc<SearchState>, error: &PluginErrorInfo) -> Rc<SearchState> {
    Rc::new(SearchState {
        status: SearchStatus::Error,
        error: Some(error.clone()),
        ..(**state).clone()
    })
}

/// `setActiveHit(state, index)`: unchecked, as the TypeScript's is.
pub fn set_active_hit(state: &Rc<SearchState>, index: isize) -> Rc<SearchState> {
    if state.active_hit_index == index {
        return state.clone();
    }
    Rc::new(SearchState { active_hit_index: index, ..(**state).clone() })
}

/// `clearSession(state)`: back to idle, or the same state when idle.
pub fn clear_session(state: &Rc<SearchState>) -> Rc<SearchState> {
    if state.status == SearchStatus::Idle {
        return state.clone();
    }
    Rc::new(initial_search_state())
}

/// `indexOfHit(hits, hit)`: the same hit, or else the hit that covers the
/// same characters of the same page, so a hit kept from before the search
/// ran again still finds its place. `-1` when it isn't there.
pub fn index_of_hit(hits: &[Rc<SearchHit>], hit: &Rc<SearchHit>) -> isize {
    let found = hits.iter().position(|candidate| Rc::ptr_eq(candidate, hit)).or_else(|| {
        hits.iter().position(|candidate| {
            candidate.start == hit.start
                && candidate.count == hit.count
                && candidate.page == hit.page
        })
    });
    found.map_or(-1, |i| i as isize)
}

/// `pagesWithHits(state, pages)`: the pages that have at least one hit, in
/// document order. `pages` is the document's page list, whose order moving
/// a page changes (object numbers don't follow it).
pub fn pages_with_hits(state: &SearchState, pages: &[PageRef]) -> Vec<PageRef> {
    pages
        .iter()
        .filter(|page| {
            state.hits_by_page.get(&page.page_object_number).is_some_and(|hits| !hits.is_empty())
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: u32, start: f64) -> Rc<SearchHit> {
        Rc::new(SearchHit {
            page: PageRef::new(page),
            page_index: page as usize,
            start,
            count: 3.0,
            segments: Vec::new(),
            bounds: None,
            snippet: None,
        })
    }

    fn slice(pages_searched: f64) -> SearchProgress {
        SearchProgress { pages_searched, page_count: 2.0 }
    }

    fn starts(hits: &[Rc<SearchHit>]) -> Vec<f64> {
        hits.iter().map(|h| h.start).collect()
    }

    #[test]
    fn an_append_keeps_order_in_the_list_and_in_each_page_bucket() {
        let state =
            append_hits(&Rc::new(initial_search_state()), &[hit(7, 0.0), hit(7, 1.0)], slice(1.0));
        // Page 9 between two runs of page 7: page 7's bucket gets both.
        let state = append_hits(&state, &[hit(7, 2.0), hit(9, 0.0), hit(7, 3.0)], slice(1.0));
        assert_eq!(starts(&state.hits), vec![0.0, 1.0, 2.0, 0.0, 3.0]);
        assert_eq!(starts(&state.hits_by_page[&7]), vec![0.0, 1.0, 2.0, 3.0]);
        assert_eq!(starts(&state.hits_by_page[&9]), vec![0.0]);
        assert_eq!(state.active_hit_index, 0);
    }

    #[test]
    fn an_append_shares_the_hits_it_already_had() {
        let before = append_hits(&Rc::new(initial_search_state()), &[hit(7, 0.0)], slice(1.0));
        let after = append_hits(&before, &[hit(9, 0.0)], slice(1.0));
        assert!(Rc::ptr_eq(&before.hits[0], &after.hits[0]));
        assert!(Rc::ptr_eq(&before.hits_by_page[&7], &after.hits_by_page[&7]));
        // The same counts keep the progress object.
        assert!(Rc::ptr_eq(&before.progress, &after.progress));
    }

    #[test]
    fn an_empty_append_that_moves_nothing_keeps_the_state() {
        let state = Rc::new(initial_search_state());
        assert!(Rc::ptr_eq(&state, &append_hits(&state, &[], SearchProgress::default())));
        let moved = append_hits(&state, &[], slice(1.0));
        assert!(!Rc::ptr_eq(&state, &moved));
        assert_eq!(moved.active_hit_index, -1);
    }

    #[test]
    fn an_index_of_hit_finds_an_equal_range_when_the_object_is_gone() {
        let hits = vec![hit(7, 0.0), hit(9, 4.0)];
        assert_eq!(index_of_hit(&hits, &hits[1]), 1);
        assert_eq!(index_of_hit(&hits, &hit(9, 4.0)), 1);
        assert_eq!(index_of_hit(&hits, &hit(9, 5.0)), -1);
    }
}
