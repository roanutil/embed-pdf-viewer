//! `packages/plugin/page-edit` in Rust: document-scoped, stateless
//! PON-addressed page edits over the engine handle's page service.
//!
//! The verbs return the engine's own abortable promise. There is no
//! client-side gate: `can_edit` is the one twin every verb shares
//! (`doc.pages.assemble`), and a refused edit is the engine's
//! `PermissionDenied`, passed through.
//!
//! The file split follows the TypeScript package: [`types`],
//! [`capability`], and the wiring in [`page_edit_plugin`].

use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub mod capability;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::PageEditCapability;
pub use types::{
    AddBlankOptions, InsertOptions, PAGE_EDIT, PageObjectNumber, PagePlacement, PdfSize,
};

/// `pageEditPlugin()`: document-scoped, with no state of its own (its
/// slice is the kernel's default `{}`).
pub fn page_edit_plugin() -> AnyPlugin {
    define::<Value, Value>("page-edit")
        .document_scoped()
        .capability(&PAGE_EDIT, |ctx| Ok(PageEditCapability::new(ctx)))
        .build()
}
