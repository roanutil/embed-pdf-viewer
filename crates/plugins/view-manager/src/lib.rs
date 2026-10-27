//! `packages/plugin/view-manager` in Rust: panes over the open documents.
//! A pane owns a tab strip and one active document, and the panes partition
//! the documents the kernel has open. Workspace-scoped: one instance sees
//! every document.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], and the wiring in [`view_manager_plugin`]. [`record`] is
//! the JavaScript property order the pane record enumerates in.

use epdf_kernel::{AnyPlugin, DOCUMENTS, define};
use serde_json::Value;

pub mod contract;
pub mod controller;
pub mod model;
pub mod record;

#[cfg(feature = "harness")]
pub mod harness;

#[cfg(feature = "uniffi")]
pub mod ffi;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use contract::{
    DocumentMovedEvent, PaneEvent, PaneFocusChangedEvent, PaneId, PaneInfo, VIEW_MANAGER,
};
pub use controller::{ViewManagerCapability, create_view_manager_controller};
pub use model::{
    Pane, Panes, ViewManagerState, add_document, create_pane, initial_view_manager_state,
    move_document_between, move_document_within, move_pane, next_pane_id, pane_of_document,
    reconcile, remove_document, remove_pane, rename_document, set_active_document,
    set_focused_pane,
};
pub use record::JsRecord;

/// `viewManagerPlugin()`: which documents each pane shows. Workspace-scoped,
/// because panes span documents.
pub fn view_manager_plugin() -> AnyPlugin {
    define::<ViewManagerState, Value>("view-manager")
        .requires(DOCUMENTS.any())
        .initial_state(initial_view_manager_state)
        .create(&VIEW_MANAGER, |cx| {
            let (api, connect) = create_view_manager_controller(cx);
            Ok((api, Some(connect)))
        })
        .build()
}
