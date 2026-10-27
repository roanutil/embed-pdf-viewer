//! `model.ts`: the page-edit session state, which pages are selected. A
//! plain list of refs, changed only by the transitions here; the controller
//! reads it over the document's pages, so a page that went leaves the
//! selection.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::geometry::PageRef;
use serde::Serialize;

/// `PageEditState`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PageEditState {
    /// The selected pages, in the order they were selected.
    pub selected: Vec<PageRef>,
}

/// `initialPageEditState()`.
pub fn initial_page_edit_state() -> PageEditState {
    PageEditState::default()
}

/// `selected(state, refs)`: the selection becomes `refs`, each page once.
/// The same selection returns `state` itself, so it writes nothing.
pub fn selected(state: Rc<PageEditState>, refs: &[PageRef]) -> Rc<PageEditState> {
    let mut seen = HashSet::with_capacity(refs.len());
    let unique: Vec<PageRef> = refs.iter().copied().filter(|page| seen.insert(*page)).collect();
    if unique == state.selected {
        return state;
    }
    Rc::new(PageEditState { selected: unique })
}
