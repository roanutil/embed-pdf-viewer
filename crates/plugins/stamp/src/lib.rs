//! `packages/plugin/stamp` in Rust: workspace-scoped stamp libraries and
//! assets. A library IS a PDF: its `/Title` is the name, its `/Names /Pages`
//! registry the assets, its pages the artwork, `/PieceInfo` only what has no
//! standard home. Marks (ink, text, images, PDF pages) are authored into
//! pages. Placement targets a document through its annotation plugin; a
//! form-backed (dynamic) asset is first evaluated on a detached copy in a
//! script realm the target document's actions plugin mints, then flattened.
//!
//! The files follow the TypeScript package: [`contract`] (with `token.ts`
//! and `host-contract.ts`), [`model`], [`convention`], [`blank_library`],
//! [`controller`] (the composition root), and the areas under
//! [`services`], [`read`] and [`write`], each an `impl` block on the one
//! [`StampCapability`].

// Callback-shaped types are the port of JavaScript closures, as in the kernel.
#![allow(clippy::type_complexity)]

pub mod blank_library;
pub mod contract;
pub mod controller;
pub mod convention;
pub mod model;
pub mod persistence;
pub mod promise;
pub mod read;
pub mod services;
pub mod value;
pub mod write;

#[cfg(feature = "harness")]
pub mod harness;

use epdf_kernel::{AnyPlugin, define};
use epdf_plugin_actions::ACTIONS;
use epdf_plugin_annotation::ANNOTATION;

pub use contract::*;
pub use controller::{StampCapability, StampHostCapability, create_stamp_controller};
pub use model::{StampAction, StampState, initial_stamp_state, stamp_reducer};
pub use services::asset_engine::DEFAULT_PREVIEW_WIDTH;
pub use services::binaries::AssetBinary;

/// `stampPlugin(config)`: workspace-scoped (libraries outlive any one
/// document); placement targets a document through its annotation plugin.
pub fn stamp_plugin(config: StampConfig) -> AnyPlugin {
    define::<StampState, StampAction>("stamp")
        .optional(ANNOTATION.any())
        .optional(ACTIONS.any())
        .initial_state(initial_stamp_state)
        .reduce(stamp_reducer)
        .create(&STAMP, move |cx| Ok((create_stamp_controller(cx, config.clone()), None)))
        .build()
}
