//! The catalog's embedded files.
//!
//! G8. Ports the read half of
//! `services/src/features/attachments/AttachmentReader.ts` and of
//! `internal/attachmentPrimitives.ts`. Operations: `attachments.list`,
//! `attachments.readFile`, `annotations.readFile`.
//!
//! A file embedded in a PDF has exactly two homes: this name tree, and a
//! file-attachment annotation's `/FS`. The metadata projection is the same
//! one for both, and it lives with G7's annotation reader
//! ([`crate::features::annotation::read::file_attachment::read_attachment_info`])
//! because that is where it was needed first. What is added here is the tree
//! itself and the decode.
//!
//! # Position is not an address
//!
//! A tree entry's index is display order, and creating or removing an entry
//! shifts every index after it. The durable address is the KEY, which is
//! unique within the tree by construction (§7.9.6) and therefore needs no
//! revision token and no weak tier: unlike an annotation, an embedded file
//! cannot be addressed ambiguously. `index` rides on the DTO as metadata, and
//! `read_file` takes a key.

use pdfium::{AttachmentExtractError, Document};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::file_attachment::{
    AttachmentFileInfo, read_attachment_info,
};

/// One entry of the `/EmbeddedFiles` name tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedFileItem {
    #[serde(flatten)]
    pub file: AttachmentFileInfo,
    /// The tree key: the durable address for a read or a remove.
    pub key: String,
    /// Position in tree order. Display metadata, NOT an address.
    pub index: u32,
}

/// A decoded embedded file.
#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentContent {
    pub name: String,
    pub mime_type: Option<String>,
    /// The DECODED length, which is `bytes.len()` and is carried separately
    /// because the streaming form of this call reports a size without
    /// producing bytes at all.
    pub size: u32,
    pub bytes: Vec<u8>,
}

/// Every entry, in tree order.
pub fn list_attachments(
    document: &Document,
    cancel: &CancelToken,
) -> Result<Vec<EmbeddedFileItem>, EngineError> {
    cancel.check()?;
    let count = document.attachment_count();
    let mut items = Vec::new();

    for index in 0..count {
        cancel.check()?;
        // An entry PDFium will not open is a gap in the tree rather than a
        // failure of the listing, the same reading a null `/Annots` entry
        // gets. It still consumes its index, because the index is a position
        // in the tree and a caller comparing two listings has to agree with
        // the file about what is where.
        //
        // Unexercised, and kept anyway. `attachments.pdf` has a tree entry
        // whose value is the number 42, which is the most broken thing a
        // name tree can hold, and `FPDFDoc_GetAttachment` still hands back a
        // usable handle for it: it lists with an empty name. Nothing found so
        // far makes it answer null for an in-range index. The TypeScript has
        // the same guard, and a guard that has never fired is cheaper to keep
        // than to remove and rediscover.
        let Some(attachment) = document.attachment(index) else {
            continue;
        };
        items.push(EmbeddedFileItem {
            file: read_attachment_info(&attachment)?,
            // An unreadable key is the empty string rather than an error.
            // The entry exists and its metadata is worth reporting; what a
            // caller loses is the ability to address it, which is what an
            // empty key says.
            key: attachment_key(document, index)?.unwrap_or_default(),
            index,
        });
    }
    Ok(items)
}

/// Decode one entry, addressed by key.
///
/// `max_decoded_bytes` of `None` is unbudgeted. The ceiling exists because a
/// PDF can embed a stream that decompresses to far more than the file it came
/// in, and the decode happens inside PDFium where nothing else can stop it.
pub fn read_attachment_file(
    document: &Document,
    key: &str,
    max_decoded_bytes: Option<u64>,
    cancel: &CancelToken,
) -> Result<AttachmentContent, EngineError> {
    cancel.check()?;
    let index = document.attachment_index_by_key(key).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no embedded file with key '{key}' in the document"),
        )
    })?;
    let attachment = document.attachment(index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            format!(
                "embedded file with key '{key}' resolved to index {index} but could not be \
                 opened"
            ),
        )
    })?;

    let info = read_attachment_info(&attachment)?;
    cancel.check()?;
    // Cancellation is cooperative up to the decode and not inside it:
    // `EPDFAttachment_ExtractFileToOwnedBuffer` is one synchronous call.
    let bytes = attachment.extract(max_decoded_bytes.unwrap_or(0)).map_err(extract_error)?;
    cancel.check()?;

    Ok(AttachmentContent {
        size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        name: info.name,
        mime_type: info.mime_type,
        bytes,
    })
}

/// Decode the file a file-attachment annotation's `/FS` names.
///
/// The annotation is the caller's, because resolving an `AnnotationRef` to a
/// live handle needs the page and its revision, which this layer does not
/// own.
pub fn read_annotation_file(
    annotation: &pdfium::Annotation<'_>,
    max_decoded_bytes: Option<u64>,
    cancel: &CancelToken,
) -> Result<AttachmentContent, EngineError> {
    cancel.check()?;
    let attachment = annotation.file_attachment().ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "annotation is not a file attachment or carries no embedded file (/FS)",
        )
    })?;

    let info = read_attachment_info(&attachment)?;
    cancel.check()?;
    let bytes = attachment.extract(max_decoded_bytes.unwrap_or(0)).map_err(extract_error)?;
    cancel.check()?;

    Ok(AttachmentContent {
        size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
        name: info.name,
        mime_type: info.mime_type,
        bytes,
    })
}

fn attachment_key(document: &Document, index: u32) -> Result<Option<String>, EngineError> {
    document.attachment_key(index).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the key of embedded file {index} failed: {e}"),
        )
    })
}

/// Each extraction failure means something different about the document, so
/// each gets the code that says what a caller can do about it.
fn extract_error(error: AttachmentExtractError) -> EngineError {
    let code = match error {
        // The filespec names a file the document does not embed. A caller
        // asked for bytes that were never there, which is their argument
        // being wrong rather than the file being broken.
        AttachmentExtractError::NoFileStream => EngineErrorCode::InvalidArg,
        AttachmentExtractError::DecodeFailed => EngineErrorCode::MalformedPdf,
        // The caller's own ceiling. Raising it is a thing they can do.
        AttachmentExtractError::SizeLimitExceeded => EngineErrorCode::InvalidArg,
        AttachmentExtractError::WriteFailed | AttachmentExtractError::Unknown(_) => {
            EngineErrorCode::Unknown
        }
    };
    EngineError::with_message(code, error.to_string())
}
