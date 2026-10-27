//! `packages/plugin/metadata` in Rust: document-scoped, reactive Info-dict
//! metadata fed by the document event stream (own, foreign and remote edits
//! alike).
//!
//! The file split follows the TypeScript package: [`contract`],
//! [`model`], [`controller`], and the wiring in [`metadata_plugin`]
//! (`metadata.plugin.ts`). The standard fields and the custom keys are two
//! kernel mirrors, read on demand: nothing reads them when a document opens.

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define, undefined};
use serde_json::Value;

pub mod contract;
pub mod controller;
#[cfg(feature = "uniffi")]
pub mod ffi;
#[cfg(feature = "harness")]
pub mod harness;
pub mod model;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use contract::*;
pub use controller::{CustomMetadataCapability, MetadataCapability, create_metadata_controller};
pub use epdf_kernel::mirror::ResourceStatus;
pub use model::changed_keys;

/// `MetadataToken` (`token.ts`).
pub static METADATA: LazyLock<Token<MetadataCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "metadata",
        "add metadataPlugin() from '@embedpdf/plugin-metadata' to your plugins list",
    )
});

/// `metadataPlugin()`: document-scoped, reactive Info-dict metadata. Takes
/// no configuration and keeps no slice of its own: its state is the two
/// mirrors' store cells.
pub fn metadata_plugin() -> AnyPlugin {
    define::<Value, Value>("metadata")
        .document_scoped()
        // No `initialState`: the plugin has no slice of its own.
        .initial_state(undefined)
        .create(&METADATA, |cx| create_metadata_controller(cx.cx.clone()))
        .build()
}
