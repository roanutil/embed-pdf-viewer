//! `state.ts`: the attachment page's state table as code, what
//! `useAttachmentState()` returns in every framework. Reading the state
//! loads the attachments: the adapter that reads it calls
//! [`attachment_state_ensure`] first (`ensure`), so the first reader of a
//! document starts the read.

use std::rc::Rc;

use epdf_kernel::rt::Promise;

use crate::contract::{AttachedFile, OperationOptions, ResourceStatus};
use crate::controller::AttachmentCapability;

/// What `attachmentState` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentStateView {
    pub files: Rc<Vec<AttachedFile>>,
    pub selected: Rc<Vec<AttachedFile>>,
    pub status: ResourceStatus,
    pub can_add: bool,
    pub can_download: bool,
}

impl Default for AttachmentStateView {
    /// `empty`: what a view shows without a document.
    fn default() -> Self {
        AttachmentStateView {
            files: Rc::default(),
            selected: Rc::default(),
            status: ResourceStatus::Idle,
            can_add: false,
            can_download: false,
        }
    }
}

/// `attachmentState`'s `read`: the read over a document's capability, or
/// the `empty` state without one.
pub fn attachment_state(attachment: Option<&AttachmentCapability>) -> AttachmentStateView {
    let Some(attachment) = attachment else { return AttachmentStateView::default() };
    AttachmentStateView {
        files: attachment.list(),
        selected: attachment.get_selected(),
        status: attachment.get_status(),
        can_add: attachment.can_add(),
        can_download: attachment.can_download(),
    }
}

/// `attachmentState`'s `ensure: (attachment) => attachment.ensureLoaded()`.
pub fn attachment_state_ensure(attachment: &AttachmentCapability) -> Promise<()> {
    attachment.ensure_loaded(OperationOptions::default())
}
