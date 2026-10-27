//! `state.ts`: the history page's state table as code, what
//! `useHistoryState()` returns in every framework. What the undo and redo
//! buttons read.

use crate::controller::HistoryCapability;
use crate::model::ChangeLabel;

/// What `historyState` reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HistoryStateView {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<ChangeLabel>,
    pub redo_label: Option<ChangeLabel>,
}

/// `historyState`: the read over a document's capability, or the empty state
/// (nothing to undo or redo) without one.
pub fn history_state(history: Option<&HistoryCapability>) -> HistoryStateView {
    match history {
        Some(history) => HistoryStateView {
            can_undo: history.can_undo(),
            can_redo: history.can_redo(),
            undo_label: history.get_undo_label(),
            redo_label: history.get_redo_label(),
        },
        None => HistoryStateView::default(),
    }
}
