//! `packages/plugin/metadata` in Rust: document-scoped, reactive Info-dict
//! metadata fed by the document event stream (own, foreign and remote edits
//! alike).
//!
//! The file split follows the TypeScript package: the contract here,
//! [`model`], [`controller`], and the wiring in [`metadata_plugin`].

use std::sync::LazyLock;

use epdf_kernel::{AnyPlugin, Token, define};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod controller;
#[cfg(feature = "uniffi")]
pub mod ffi;
#[cfg(feature = "harness")]
pub mod harness;
pub mod model;

#[cfg(feature = "uniffi")]
uniffi::setup_scaffolding!();

pub use controller::{MetadataCapability, create_metadata_controller};
pub use model::{
    MetadataAction, MetadataState, changed_keys, initial_metadata_state, reduce_metadata,
};

// ── contract ──────────────────────────────────────────────────────────────────

/// `DocumentMetadata`, as the engine reports it. The plugin reads only its
/// keys (for `changedKeys`), so it stays the engine's JSON.
pub type DocumentMetadata = Value;

/// `MetadataPatch`: three-state per key (absent leaves, `null` clears, a
/// value sets). Carried through to the engine as written.
pub type MetadataPatch = Value;

/// `MetadataUpdateResult`: what the engine answers an update with.
pub type MetadataUpdateResult = Value;

/// `ResourceStatus`: load state of a resource a plugin hydrates from the
/// engine. `forbidden` is a permission refusal, not an empty value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
#[serde(rename_all = "lowercase")]
pub enum ResourceStatus {
    Idle,
    Loading,
    Ready,
    Forbidden,
    Error,
}

/// `MetadataUpdatedEvent`: one confirmed change, whoever caused it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdatedEvent {
    pub metadata: DocumentMetadata,
    pub previous: Option<DocumentMetadata>,
    pub changed_keys: Vec<String>,
    /// `ChangeOrigin`.
    pub origin: Value,
}

pub static METADATA: LazyLock<Token<MetadataCapability>> = LazyLock::new(|| {
    Token::with_hint(
        "metadata",
        "add metadataPlugin() from '@embedpdf/plugin-metadata' to your plugins list",
    )
});

// ── plugin ────────────────────────────────────────────────────────────────────

/// `metadataPlugin()`: document-scoped, reactive Info-dict metadata. Takes
/// no configuration.
pub fn metadata_plugin() -> AnyPlugin {
    define::<MetadataState, MetadataAction>("metadata")
        .document_scoped()
        .initial_state(initial_metadata_state)
        .reduce(reduce_metadata)
        .create(&METADATA, create_metadata_controller)
        .build()
}
