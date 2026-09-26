//! `packages/plugin/stamp` in Rust: workspace-scoped stamp libraries.
//!
//! An imported PDF becomes a canonical library whose catalog and page
//! `/PieceInfo` carry its metadata; each page is a vector asset with a
//! derived placement PDF and a cached preview. PNG and JPEG assets stay
//! loose. Placement names its document and goes through that document's
//! annotation plugin (`armStamp`); with scripting enabled, a form-backed PDF
//! asset is evaluated on a detached copy through the form plugin's standalone
//! scripting controller and flattened first.
//!
//! The file split follows the TypeScript package: `types`, `reducer`,
//! `capability`, and the wiring in [`stamp_plugin`]. The package's
//! `/contract` entry is [`STAMP`] and the types.

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod capability;
pub mod promise;
pub mod reducer;
pub mod types;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};

pub use capability::{AssetBinary, StampCapability, create_stamp_capability};
pub use reducer::{initial_stamp_state, stamp_reducer};
pub use types::*;

/// `stampPlugin(config)`. Workspace-scoped: libraries are shared across
/// every open document. There is no hard `requires`: the annotation token is
/// document-scoped and resolved per placement, so a viewer without the
/// annotation plugin fails at the arm call with the kernel's own
/// missing-capability error.
pub fn stamp_plugin(config: StampConfig) -> AnyPlugin {
    define::<StampState, StampAction>("stamp")
        .initial_state(initial_stamp_state)
        .reduce(stamp_reducer)
        .capability(&STAMP, move |ctx| Ok(create_stamp_capability(ctx, config.clone())))
        .build()
}
