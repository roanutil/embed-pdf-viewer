//! `state.ts`: the search page's state table as code, what
//! `useSearchState()` returns in every framework. The matches themselves are
//! not here: `list_hits` reads them, so a view that only shows the count
//! doesn't redraw for every match that arrives.

use std::rc::Rc;

use crate::contract::{PluginErrorInfo, SearchHit, SearchProgress, SearchQuery, SearchStatus};
use crate::controller::SearchCapability;
use crate::model::no_progress;

/// What `searchState` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchStateView {
    pub query: Option<SearchQuery>,
    pub status: SearchStatus,
    pub hit_count: usize,
    pub active_hit_index: isize,
    pub active_hit: Option<Rc<SearchHit>>,
    pub progress: Rc<SearchProgress>,
    pub error: Option<PluginErrorInfo>,
}

/// `searchState`: the read over a document's capability, or the empty state
/// (idle, no hits, `NO_PROGRESS`) without one.
pub fn search_state(search: Option<&SearchCapability>) -> SearchStateView {
    match search {
        Some(search) => SearchStateView {
            query: search.get_query(),
            status: search.get_status(),
            hit_count: search.get_hit_count(None),
            active_hit_index: search.get_active_hit_index(),
            active_hit: search.get_active_hit(),
            progress: search.get_progress(),
            error: search.get_error(),
        },
        None => SearchStateView {
            query: None,
            status: SearchStatus::Idle,
            hit_count: 0,
            active_hit_index: -1,
            active_hit: None,
            progress: no_progress(),
            error: None,
        },
    }
}
