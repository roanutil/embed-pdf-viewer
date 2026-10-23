//! `state.ts`: the selection page's state table as code, what
//! `useSelectionState()` returns in every framework. The geometry is not
//! here: a layer reads its page's segments with `list_segments(page)`.

use epdf_kernel::geometry::PageRef;

use crate::contract::TextRange;
use crate::controller::SelectionCapability;

/// What `selectionState` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionStateView {
    pub has_selection: bool,
    pub is_selecting: bool,
    pub range: Option<TextRange>,
    pub pages: Vec<PageRef>,
}

/// `selectionState`: the read over a document's capability, or the empty
/// state without one.
pub fn selection_state(selection: Option<&SelectionCapability>) -> SelectionStateView {
    match selection {
        Some(selection) => SelectionStateView {
            has_selection: selection.has_selection(),
            is_selecting: selection.is_selecting(),
            range: selection.get_range(),
            pages: selection.list_selected_pages(),
        },
        None => SelectionStateView {
            has_selection: false,
            is_selecting: false,
            range: None,
            pages: Vec::new(),
        },
    }
}
