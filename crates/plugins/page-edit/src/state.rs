//! `state.ts`: the pages page's state table as code, what
//! `usePageEditState()` returns in every framework.

use std::rc::Rc;

use epdf_kernel::geometry::PageRef;

use crate::controller::PageEditCapability;

/// What `pageEditState` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageEditStateView {
    pub selected: Rc<Vec<PageRef>>,
    pub can_edit: bool,
    pub can_extract: bool,
}

/// `pageEditState`: the read over a document's capability, or the empty
/// state (nothing selected, nothing allowed) without one.
pub fn page_edit_state(page_edit: Option<&PageEditCapability>) -> PageEditStateView {
    match page_edit {
        Some(page_edit) => PageEditStateView {
            selected: page_edit.get_selected(),
            can_edit: page_edit.can_edit(),
            can_extract: page_edit.can_extract(),
        },
        None => PageEditStateView::default(),
    }
}
