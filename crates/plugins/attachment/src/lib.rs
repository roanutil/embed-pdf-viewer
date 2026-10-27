//! `packages/plugin/attachment` in Rust: the files a document carries, in
//! its attachments (`/EmbeddedFiles`) and in file attachment annotations,
//! as one list; adding and removing attachments as undoable changes, and
//! saving any file.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`names`], `sync` (`sync/files.ts`), [`controller`], [`state`], and the
//! wiring of `attachment.plugin.ts` in [`attachment_plugin`]. The
//! attachments are a kernel mirror, read on demand: nothing reads them when
//! a document opens.

mod sync;

pub mod contract;
pub mod controller;
pub mod model;
pub mod names;
pub mod state;

#[cfg(feature = "harness")]
pub mod harness;

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define};
use epdf_plugin_annotation::ANNOTATION;
use epdf_plugin_stage::STAGE;
use serde_json::Value;

pub use contract::*;
pub use controller::{AttachmentCapability, create_attachment_controller};
pub use model::{AttachmentState, free_name, initial_attachment_state};
pub use state::{AttachmentStateView, attachment_state, attachment_state_ensure};
pub use sync::{fold_files, predict_files, undo_of};

/// `AttachmentToken` (`token.ts`).
pub static ATTACHMENT: LazyLock<Token<AttachmentCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "attachment",
        "add attachmentPlugin() from '@embedpdf/plugin-attachment' to your plugins list",
    )
});

/// `attachmentPlugin()`: the files a document carries. Document-scoped;
/// optionally uses the annotations (a page's files, and removing and saving
/// them) and the stage (going to a page's file).
pub fn attachment_plugin() -> AnyPlugin {
    define::<AttachmentState, Value>("attachment")
        .document_scoped()
        .optional(ANNOTATION.any())
        .optional(STAGE.any())
        .initial_state(initial_attachment_state)
        .create(&ATTACHMENT, create_attachment_controller)
        .build()
}
