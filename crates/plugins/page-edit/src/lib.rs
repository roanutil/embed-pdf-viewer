//! `packages/plugin/page-edit` in Rust: document-scoped page edits (rotate,
//! reorder, delete, insert, duplicate, extract) over the engine handle, each
//! one change of the document's history, and the page selection.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], [`state`], and the wiring in [`page_edit_plugin`].

use epdf_kernel::{AnyPlugin, define};
use serde_json::Value;

pub mod contract;
pub mod controller;
pub mod model;
pub mod state;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use contract::*;
pub use controller::{PageEditCapability, create_page_edit_controller};
pub use model::{PageEditState, initial_page_edit_state};
pub use state::{PageEditStateView, page_edit_state};

/// `pageEditPlugin()`: document-scoped, the document's page edits, each one
/// change of its history, and which of its pages are selected.
pub fn page_edit_plugin() -> AnyPlugin {
    define::<PageEditState, Value>("page-edit")
        .document_scoped()
        .initial_state(initial_page_edit_state)
        .create(&PAGE_EDIT, |cx| Ok((create_page_edit_controller(cx), None)))
        .build()
}
