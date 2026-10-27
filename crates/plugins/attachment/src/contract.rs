//! `contract.ts`: the public attachment vocabulary.
//!
//! Files, refs and sources are the engine's records as JSON (`Value`), as
//! the plugin carries them. An [`AttachedFile`] is the file's info (`name`,
//! `mimeType`, `description`, `size`, `checksum`, `createdAt`,
//! `modifiedAt`), then `id` and `home`: `{kind: 'document', ref: {kind:
//! 'key', key}}` for one of the document's attachments, `{kind: 'page',
//! ref}` with an annotation ref for a page's file attachment annotation.

use epdf_kernel::bytes::Bytes;
use epdf_kernel::signal::AbortSignal;
use serde::Serialize;
use serde_json::{Value, json};

pub use crate::names::display_name_of;
pub use epdf_kernel::mirror::ResourceStatus;

/// `AttachedFile`: one file the document carries, from either home.
pub type AttachedFile = Value;
/// `AttachmentRef`: `{kind: 'key', key}`, the name-tree key.
pub type AttachmentRef = Value;
/// `EventOrigin`, as the confirmed event carried it.
pub type EventOrigin = Value;
/// `AttachmentFileSource`: `{data, name?, mimeType?, description?}`, the
/// bytes as the kernel holds them (`{"$bytes": base64}`, see
/// [`file_source`]). A `File` or `Blob` has no spelling here: the host
/// passes its name and type.
pub type AttachmentFileSource = Value;

/// An [`AttachmentFileSource`] for these bytes under this name.
pub fn file_source(bytes: &[u8], name: &str) -> AttachmentFileSource {
    json!({ "data": Bytes(bytes.to_vec()).to_json(), "name": name })
}

/// `OperationOptions`: what the reads and writes take.
#[derive(Clone, Default)]
pub struct OperationOptions {
    pub signal: Option<AbortSignal>,
}

/// `AttachmentContent`: a downloaded file, ready to save: its bytes, its
/// name as a person may see it ([`display_name_of`]) and its type.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttachmentContent {
    /// The engine's bytes, `{"$bytes": base64}`.
    pub bytes: Value,
    pub name: String,
    /// The file's declared type: `null` when it has none, as the file said
    /// it.
    #[serde(rename = "mimeType")]
    pub mime_type: Value,
}

impl AttachmentContent {
    /// The bytes, decoded.
    pub fn data(&self) -> Option<Vec<u8>> {
        Bytes::from_json(&self.bytes).map(|b| b.0)
    }
}

/// `AttachmentsChangedEvent`: a confirmed change of the document's
/// attachments, whoever made it: this session or another. Fires after the
/// list shows it. A page's file is an annotation: its changes are the
/// annotation plugin's events.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttachmentsChangedEvent {
    /// `added`, `removed`, or `restored` (an undo brought a removed file
    /// back).
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// The files it touched; for a removal, as they were.
    pub files: Vec<AttachedFile>,
    pub origin: EventOrigin,
}

/// `AttachmentsResyncedEvent`: the document's attachments were read from
/// the engine, first or again.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttachmentsResyncedEvent {
    pub files: Vec<AttachedFile>,
}
