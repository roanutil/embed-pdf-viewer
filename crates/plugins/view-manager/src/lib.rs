//! `packages/plugin/view-manager` in Rust: panes over the open documents.
//! A view owns a tab strip and one active document, and the views partition
//! the documents the kernel has open. Workspace-scoped: one instance sees
//! every document.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], [`effects`], and the wiring in [`view_manager_plugin`].

use epdf_kernel::{AnyPlugin, DOCUMENTS, define};

pub mod capability;
pub mod effects;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::ViewManagerCapability;
pub use reducer::{initial_view_manager_state, view_manager_reducer};
pub use types::{VIEW_MANAGER, View, ViewInfo, ViewManagerAction, ViewManagerState};

/// `viewManagerPlugin()`.
pub fn view_manager_plugin() -> AnyPlugin {
    define::<ViewManagerState, ViewManagerAction>("view-manager")
        .requires(DOCUMENTS.any())
        .initial_state(initial_view_manager_state)
        .reduce(view_manager_reducer)
        .capability(&VIEW_MANAGER, |ctx| Ok(ViewManagerCapability::new(ctx)))
        .effects(effects::register_view_manager_effects)
        .build()
}
