//! `packages/plugin/shell` in Rust: which named surfaces (panels, modals,
//! overlays) and dropdown menus are open, per document. Pure state, no
//! effects: the app renders surfaces, commands toggle them.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`]
//! (the named transitions), [`controller`], and the wiring in
//! [`shell_plugin`]. [`record`] is the JavaScript property order
//! `listOpenSurfaces()` reads.

use epdf_kernel::{AnyPlugin, define};
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
    MenuClosedEvent, MenuOpenedEvent, OpenSurfaceOptions, SHELL, ShellSnapshot, SurfaceClosedEvent,
    SurfaceOpenedEvent, SurfaceProps, SurfaceState,
};
pub use controller::{ShellCapability, create_shell_controller};
pub use model::{
    ShellState, SnapshotSurface, SurfaceRecord, apply_snapshot, close_all_menus,
    close_all_surfaces, close_menu, close_surface, initial_shell_state, open_menu, open_surface,
    set_surface_props, snapshot_of, toggle_menu, toggle_surface,
};
pub use record::JsRecord;

/// `shellPlugin()`: document-scoped, so each document keeps its own panels
/// and switching tabs restores them. The app renders the surfaces; the
/// shell only stores their state.
pub fn shell_plugin() -> AnyPlugin {
    define::<ShellState, Value>("shell")
        .document_scoped()
        .initial_state(initial_shell_state)
        .create(&SHELL, |cx| Ok((create_shell_controller(cx), None)))
        .build()
}
