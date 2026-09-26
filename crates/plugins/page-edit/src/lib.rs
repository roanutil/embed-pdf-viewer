//! `packages/plugin/page-edit` in Rust: document-scoped, stateless,
//! ref-addressed page edits (rotate, move, delete, insert, duplicate,
//! extract) over the engine handle's page service.
//!
//! There is no client-side gate: `can_edit` is the one twin every verb
//! shares (`doc.pages.assemble`), and a refused edit is the engine's
//! `PermissionDenied`, mapped to `permission-denied`.
//!
//! The file split follows the TypeScript package: [`contract`],
//! [`controller`], and the wiring in [`page_edit_plugin`].

use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub mod contract;
pub mod controller;

#[cfg(feature = "harness")]
pub mod harness;

pub use contract::{InsertBlankOptions, InsertFromBytesOptions, PAGE_EDIT, PagePlacement, PdfSize};
pub use controller::{PageEditCapability, create_page_edit_controller};

/// `pageEditPlugin()`: document-scoped, with no state of its own (its
/// slice is the kernel's default `{}`).
pub fn page_edit_plugin() -> AnyPlugin {
    define::<Value, Value>("page-edit")
        .document_scoped()
        .create(&PAGE_EDIT, |cx| Ok((create_page_edit_controller(cx), None)))
        .build()
}
