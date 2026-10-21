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
//! every call takes an [`AttachmentRef`] (a9434c0e2).
//!
//! # Writes
//!
//! The write half of `AttachmentMutator.ts` and `writeAttachmentFilePayload`
//! is here too: [`create_attachment`] and [`delete_attachment`], which the
//! `attachments.create` and `attachments.delete` change ops run
//! (`features::changes::attachments`), and the capture an undo of a delete
//! puts back ([`capture_attachment`], [`restore_attachment`]).

use pdfium::{AttachmentExtractError, Document};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::read::file_attachment::{
    AttachmentFileInfo, read_attachment_info,
};

/// Durable address of a document-level attachment: its name-tree key
/// (`AttachmentRef`). A tagged union so other ref kinds can follow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AttachmentRef {
    Key { key: String },
}

impl AttachmentRef {
    /// `toAttachmentRef`.
    pub fn key(key: impl Into<String>) -> Self {
        Self::Key { key: key.into() }
    }

    /// The name-tree key it names.
    pub fn as_key(&self) -> &str {
        match self {
            Self::Key { key } => key,
        }
    }
}

/// One entry of the `/EmbeddedFiles` name tree (`Attachment`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddedFileItem {
    #[serde(flatten)]
    pub file: AttachmentFileInfo,
    /// The attachment's address, for download and delete.
    #[serde(rename = "ref")]
    pub reference: AttachmentRef,
    /// Position in tree order. Display metadata, NOT an address.
    pub index: u32,
}

/// The metadata fields a write takes (`AttachmentFileBase`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentFileBase {
    /// The file name, which becomes the tree key.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
            reference: AttachmentRef::key(attachment_key(document, index)?.unwrap_or_default()),
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

/// The entry under `key` as it reads now; `None` when no file has the key
/// (`AttachmentMutator.find`).
pub fn find_attachment(
    document: &Document,
    key: &str,
) -> Result<Option<EmbeddedFileItem>, EngineError> {
    let Some(index) = document.attachment_index_by_key(key) else {
        return Ok(None);
    };
    let Some(entry) = document.attachment(index) else {
        return Ok(None);
    };
    Ok(Some(EmbeddedFileItem {
        file: read_attachment_info(&entry)?,
        reference: AttachmentRef::key(key),
        index,
    }))
}

/// Add an embedded file to the catalog's name tree
/// (`AttachmentMutator.create`), and read it back.
///
/// The name becomes the tree KEY, which is the entry's identity. A duplicate
/// is refused rather than replacing: two files with one key would leave the
/// document with an address that names two things. `bytes` is the op's
/// `file` resource, which it must carry.
pub fn create_attachment(
    document: &Document,
    file: &AttachmentFileBase,
    bytes: Option<&[u8]>,
    clock: &dyn Clock,
    cancel: &CancelToken,
) -> Result<EmbeddedFileItem, EngineError> {
    cancel.check()?;
    let Some(bytes) = bytes else {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "an attachments.create op needs its bytes as its file resource",
        )
        .with_details(serde_json::json!({ "field": "resources.file" })));
    };
    if file.name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "attachment file requires a name",
        ));
    }
    // Probed rather than left to PDFium, which refuses a duplicate too but
    // with a failure that says nothing about which name collided.
    if document.attachment_index_by_key(&file.name).is_some() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "an embedded file with key '{}' already exists: keys are unique; rename and retry",
                file.name
            ),
        ));
    }

    cancel.check()?;
    let attachment = document.add_attachment(&file.name).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "FPDFDoc_AddAttachment returned NULL")
    })?;
    write_file_payload(document, &attachment, bytes, file, clock)?;

    // Read back through a fresh lookup: the tree is key-sorted, so the new
    // entry's INDEX depends on what was already there and cannot be known
    // before the insert.
    find_attachment(document, &file.name)?.ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::Unknown,
            "created attachment did not resolve by key after the write",
        )
    })
}

/// `writeAttachmentFilePayload`, for an embedded file and a file
/// attachment's file alike: the bytes, the creation date in UTC, the
/// declared type when there is one, and the description when given.
pub(crate) fn write_file_payload(
    document: &Document,
    attachment: &pdfium::Attachment<'_>,
    bytes: &[u8],
    file: &AttachmentFileBase,
    clock: &dyn Clock,
) -> Result<(), EngineError> {
    // Made whatever the length: a zero-byte attachment is a real one, and
    // skipping this would leave a tree entry naming a stream that does not
    // exist.
    if !attachment.set_file(document, bytes) {
        return Err(unknown("FPDFAttachment_SetFile returned false"));
    }
    // `FPDFAttachment_SetFile` dates the file in local time without an
    // offset; a date the engine makes is UTC, unambiguous across zones.
    // Best effort, as upstream's (`attachmentPrimitives.ts:89-91`) ignores a
    // refusal: the file is written either way, with SetFile's own date.
    if let Some(created) = crate::clock::format_pdf_timestamp(clock.now()) {
        let _ = attachment.set_string_value("CreationDate", &created);
    }
    // A file without a declared type has none: the engine never guesses one.
    if let Some(mime) = file.mime_type.as_deref().filter(|mime| !mime.is_empty())
        && !matches!(attachment.set_mime_type(mime), Ok(true))
    {
        return Err(unknown("EPDFAttachment_SetSubtype returned false"));
    }
    if let Some(description) = &file.description
        && !attachment.set_description(description)
    {
        return Err(unknown("EPDFAttachment_SetDescription returned false"));
    }
    Ok(())
}

fn set_param(
    attachment: &pdfium::Attachment<'_>,
    key: &str,
    value: &str,
) -> Result<(), EngineError> {
    match attachment.set_string_value(key, value) {
        Ok(true) => Ok(()),
        _ => Err(unknown(&format!("FPDFAttachment_SetStringValue refused /{key}"))),
    }
}

fn unknown(message: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

/// Remove an embedded file by key (`AttachmentMutator.delete`).
///
/// Unlinks the tree entry. The stream it named goes when the document is
/// next written whole; an incremental save leaves it in the file and
/// unreachable, which is the format's behaviour rather than this layer's.
pub fn delete_attachment(
    document: &Document,
    key: &str,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    cancel.check()?;
    let index = document.attachment_index_by_key(key).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no embedded file with key '{key}' in the document"),
        )
    })?;
    if !document.delete_attachment(index) {
        return Err(unknown("FPDFDoc_DeleteAttachment returned false"));
    }
    Ok(())
}

/// What an undo of a delete puts back: the file under its key, as it read
/// before the delete.
///
/// Upstream captures the file's objects (`EPDFAttachment_ExportRawToOwnedBuffer`)
/// and imports them back (`EPDFAttachment_ImportRaw`), which the fork allows
/// only inside a layer transaction. This engine opens a plain document and
/// takes back a refused change through reverse steps, with no layer
/// transaction (8.30), so the capture is the file as the API reads and
/// writes it: its name, type, description, `/Params` dates and checksum,
/// and decoded bytes. What it does not keep: the filespec's object number,
/// whether a stream was shared with another entry, `/Params /Size` (which
/// the restore derives from the bytes), a `/CheckSum` that is not hex (a
/// literal string the setter would write back in another form, so the
/// restore keeps the digest `FPDFAttachment_SetFile` computes), entries the
/// API has no setter for,
/// and the absence of a `/Params` entry the restore's `FPDFAttachment_SetFile`
/// writes (`Size`, `CheckSum`, `CreationDate`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentCapture {
    /// `/UF`, which can differ from the key.
    pub name: String,
    pub mime_type: Option<String>,
    pub description: Option<String>,
    /// `/Params /CreationDate` and `/ModDate` as the PDF wrote them, and
    /// `/CheckSum` when it reads as hex.
    pub creation_date: Option<String>,
    pub mod_date: Option<String>,
    pub checksum: Option<String>,
    /// The decoded file; `None` for an entry with no embedded stream.
    #[serde(with = "hex_bytes")]
    pub bytes: Option<Vec<u8>>,
}

/// Capture the file under `key` before a delete removes it.
pub fn capture_attachment(
    document: &Document,
    key: &str,
) -> Result<AttachmentCapture, EngineError> {
    let index = document.attachment_index_by_key(key).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("no embedded file with key '{key}' in the document"),
        )
    })?;
    let attachment = document
        .attachment(index)
        .ok_or_else(|| unknown(&format!("attachment '{key}' could not be captured")))?;
    let info = read_attachment_info(&attachment)?;
    let read = |key: &str| attachment.string_value(key).ok().flatten().filter(|v| !v.is_empty());
    let bytes = match attachment.extract(0) {
        Ok(bytes) => Some(bytes),
        Err(AttachmentExtractError::NoFileStream) => None,
        Err(e) => return Err(extract_error(e)),
    };
    Ok(AttachmentCapture {
        name: info.name,
        mime_type: info.mime_type,
        description: info.description,
        creation_date: read("CreationDate"),
        mod_date: read("ModDate"),
        // Only a hex checksum round-trips through the setter; the restore
        // leaves any other to `FPDFAttachment_SetFile`'s digest.
        checksum: read("CheckSum").filter(|_| info.checksum.as_deref().is_some_and(is_hex)),
        bytes,
    })
}

fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Put a captured file back under `key`, which no file may hold now.
pub fn restore_attachment(
    document: &Document,
    key: &str,
    capture: &AttachmentCapture,
) -> Result<(), EngineError> {
    if document.attachment_index_by_key(key).is_some() {
        return Err(unknown(&format!("attachment '{key}' is held by another file")));
    }
    let attachment = document
        .add_attachment(key)
        .ok_or_else(|| unknown("FPDFDoc_AddAttachment returned NULL"))?;
    if let Some(bytes) = &capture.bytes
        && !attachment.set_file(document, bytes)
    {
        return Err(unknown("FPDFAttachment_SetFile returned false"));
    }
    // PDFium names the file after its key; a foreign file's `/UF` can differ.
    if !capture.name.is_empty() && capture.name != key && !attachment.set_name(&capture.name) {
        return Err(unknown("EPDFAttachment_SetName returned false"));
    }
    if let Some(date) = &capture.creation_date {
        set_param(&attachment, "CreationDate", date)?;
    }
    if let Some(date) = &capture.mod_date {
        set_param(&attachment, "ModDate", date)?;
    }
    if let Some(checksum) = &capture.checksum {
        set_param(&attachment, "CheckSum", checksum)?;
    }
    if let Some(mime) = &capture.mime_type
        && !matches!(attachment.set_mime_type(mime), Ok(true))
    {
        return Err(unknown("EPDFAttachment_SetSubtype returned false"));
    }
    if let Some(description) = &capture.description
        && !attachment.set_description(description)
    {
        return Err(unknown("EPDFAttachment_SetDescription returned false"));
    }
    Ok(())
}

/// Bytes in a record as lowercase hex: a record crosses the wire as JSON
/// (`ChangeRecordPayload`), which has no byte type.
pub(crate) mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        match bytes {
            None => s.serialize_none(),
            Some(bytes) => s.serialize_str(&encode(bytes)),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|text| decode(&text).ok_or_else(|| serde::de::Error::custom("not hex bytes")))
            .transpose()
    }

    pub fn encode(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn decode(text: &str) -> Option<Vec<u8>> {
        if !text.len().is_multiple_of(2) {
            return None;
        }
        (0..text.len())
            .step_by(2)
            .map(|at| text.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::hex_bytes;

    #[test]
    fn hex_bytes_round_trip_and_refuse_what_is_not_hex() {
        let bytes = vec![0u8, 1, 0xab, 0xff];
        assert_eq!(hex_bytes::encode(&bytes), "0001abff");
        assert_eq!(hex_bytes::decode("0001abff"), Some(bytes));
        assert_eq!(hex_bytes::decode(""), Some(Vec::new()));
        assert_eq!(hex_bytes::decode("abc"), None);
        assert_eq!(hex_bytes::decode("zz"), None);
    }
}
