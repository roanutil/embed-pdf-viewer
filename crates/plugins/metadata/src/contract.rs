//! `contract.ts`: the capability's events and the types it carries.
//!
//! The plugin reads only the keys of what the engine sends (for
//! `changedKeys`), so the metadata stays the engine's JSON: the standard
//! fields with ISO 8601 `createdAt` / `modifiedAt` and the tri-valued
//! `trapped`, and the custom keys as a `{ key: value }` object.

use std::rc::Rc;

use epdf_kernel::signal::AbortSignal;
use serde::Serialize;
use serde_json::Value;

/// `DocumentMetadata`: the Info dict's standard fields, as the engine
/// reports them.
pub type DocumentMetadata = Value;

/// `CustomMetadata`: the Info dict's other keys, `{ key: value }`.
pub type CustomMetadata = Value;

/// `MetadataPatch`: three-state per field (absent leaves, `null` clears, a
/// value sets). Carried to the engine as written.
pub type MetadataPatch = Value;

/// `CustomMetadataPatch`: per key, `null` removes and a string sets.
/// Carried to the engine as written.
pub type CustomMetadataPatch = Value;

/// `EventOrigin`, as the confirmed event carried it.
pub type EventOrigin = Value;

/// `OperationOptions`: what the verbs take.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `MetadataUpdatedEvent`: a confirmed change of the standard fields.
/// Local edits, other plugins, scripts and other sessions all reach the
/// same confirmed `metadata.updated` document event, which is the one path
/// that updates the snapshot and fires this.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdatedEvent {
    pub metadata: Rc<DocumentMetadata>,
    pub previous: Option<Rc<DocumentMetadata>>,
    pub changed_keys: Vec<String>,
    pub origin: EventOrigin,
}

/// `MetadataResyncedEvent`: the standard fields were (re)loaded from the
/// engine.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MetadataResyncedEvent {
    pub metadata: Rc<DocumentMetadata>,
}

/// `CustomMetadataUpdatedEvent`: a confirmed change of the custom keys
/// (`metadata.customUpdated`), whoever caused it. `changed_keys` names the
/// keys that were added, changed or removed.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMetadataUpdatedEvent {
    pub custom: Rc<CustomMetadata>,
    pub previous: Option<Rc<CustomMetadata>>,
    pub changed_keys: Vec<String>,
    pub origin: EventOrigin,
}

/// `CustomMetadataResyncedEvent`: the custom keys were (re)loaded from the
/// engine.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CustomMetadataResyncedEvent {
    pub custom: Rc<CustomMetadata>,
}
