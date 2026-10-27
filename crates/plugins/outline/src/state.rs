//! `state.ts`: the outline page's state table as code, what
//! `useOutlineState()` returns in every framework. The adapter that reads
//! it calls [`outline_state_ensure`] first (`ensure`): the first reader of a
//! document starts the read.

use std::rc::Rc;

use epdf_kernel::rt::Promise;
use serde_json::Value;

use crate::contract::{OperationOptions, OutlineItemRef, OutlineRow, ResourceStatus};
use crate::controller::OutlineCapability;

/// What `outlineState` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineStateView {
    pub items: Rc<Vec<Value>>,
    pub rows: Rc<Vec<OutlineRow>>,
    pub current: Option<OutlineItemRef>,
    pub selected: Rc<Vec<OutlineItemRef>>,
    pub status: ResourceStatus,
    pub incomplete: bool,
    pub can_edit: bool,
}

impl Default for OutlineStateView {
    /// `empty`: what a view shows without a document.
    fn default() -> Self {
        OutlineStateView {
            items: Rc::default(),
            rows: Rc::default(),
            current: None,
            selected: Rc::default(),
            status: ResourceStatus::Idle,
            incomplete: false,
            can_edit: false,
        }
    }
}

/// `outlineState`'s `read`: the read over a document's capability, or the
/// `empty` state without one.
pub fn outline_state(outline: Option<&OutlineCapability>) -> OutlineStateView {
    let Some(outline) = outline else { return OutlineStateView::default() };
    OutlineStateView {
        items: outline.list(),
        rows: outline.list_rows(),
        current: outline.get_current_item(),
        selected: outline.get_selected(),
        status: outline.get_status(),
        incomplete: outline.is_incomplete(),
        can_edit: outline.can_edit(),
    }
}

/// `outlineState`'s `ensure: (outline) => outline.ensureLoaded()`.
pub fn outline_state_ensure(outline: &OutlineCapability) -> Promise<()> {
    outline.ensure_loaded(OperationOptions::default())
}
