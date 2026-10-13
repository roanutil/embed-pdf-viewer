//! `packages/plugin/shell` in Rust: which named surfaces (panels, modals,
//! overlays) and dropdown menus are open, per document. Pure state, no
//! effects: the app renders surfaces, commands toggle them.
//!
//! The file split follows the TypeScript package: [`contract`], [`model`],
//! [`controller`], and the wiring in [`shell_plugin`]. [`record`] is the
//! JavaScript property order `listOpenSurfaces()` reads.

use epdf_kernel::{AnyPlugin, define};

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
    MenuEvent, OpenSurfaceOptions, SHELL, ShellSnapshot, SurfaceEvent, SurfaceProps, SurfaceState,
};
pub use controller::{ShellCapability, create_shell_controller};
pub use model::{ShellAction, ShellState, SurfaceRecord, initial_shell_state, shell_reducer};
pub use record::JsRecord;

/// `shellPlugin()`: document-scoped, so each document keeps its own panels
/// and switching tabs restores them.
pub fn shell_plugin() -> AnyPlugin {
    define::<ShellState, ShellAction>("shell")
        .document_scoped()
        .initial_state(initial_shell_state)
        .reduce(shell_reducer)
        .create(&SHELL, |cx| Ok((create_shell_controller(cx), None)))
        .build()
}
