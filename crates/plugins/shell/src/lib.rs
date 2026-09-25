//! `packages/plugin/shell` in Rust: which named surfaces (panels, modals,
//! overlays) and dropdown menus are open, per document. Pure state, no
//! effects: the app renders surfaces, commands toggle them.
//!
//! The file split follows the TypeScript package: [`types`], [`reducer`],
//! [`capability`], and the wiring in [`shell_plugin`]. [`record`] is the
//! JavaScript property order `openSurfaces()` reads.

use epdf_kernel::{AnyPlugin, define};

pub mod capability;
pub mod record;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

pub use capability::ShellCapability;
pub use record::JsRecord;
pub use reducer::{initial_shell_state, shell_reducer};
pub use types::{OpenSurfaceOptions, SHELL, ShellAction, ShellState, SurfaceProps, SurfaceState};

/// `shellPlugin()`: document-scoped, so each document keeps its own panels
/// and switching tabs restores them.
pub fn shell_plugin() -> AnyPlugin {
    define::<ShellState, ShellAction>("shell")
        .document_scoped()
        .initial_state(initial_shell_state)
        .reduce(shell_reducer)
        .capability(&SHELL, |ctx| Ok(ShellCapability::new(ctx)))
        .build()
}
