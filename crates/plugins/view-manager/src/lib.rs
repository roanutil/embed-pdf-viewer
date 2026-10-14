//! `packages/plugin/view-manager` in Rust: panes over the open documents.
//! A pane owns a tab strip and one active document, and the panes partition
//! the documents the kernel has open. Workspace-scoped: one instance sees
//! every document.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], and the wiring in [`view_manager_plugin`]. [`record`] is
//! the JavaScript property order the pane record enumerates in.

use epdf_kernel::{AnyPlugin, DOCUMENTS, define};

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
    Pane, ViewManagerAction, ViewManagerState, initial_view_manager_state, view_manager_reducer,
};
pub use record::JsRecord;

/// `viewManagerPlugin()`: workspace-scoped, because panes are a workspace
/// concern.
pub fn view_manager_plugin() -> AnyPlugin {
    define::<ViewManagerState, ViewManagerAction>("view-manager")
        .requires(DOCUMENTS.any())
        .initial_state(initial_view_manager_state)
        .reduce(view_manager_reducer)
        .create(&VIEW_MANAGER, |cx| {
            let (api, connect) = create_view_manager_controller(cx);
            Ok((api, Some(connect)))
        })
        .build()
}
