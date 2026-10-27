//! Reading the document Info dictionary.
//!
//! G6. Replaces `services/src/features/metadata/MetadataReader.ts` and
//! `internal/read*.ts`. Operations: `metadata.read` and
//! `metadata.readCustom`, which upstream split apart in a4b8259f7: the
//! standard fields are one answer and the dict's other keys another.
//!
//! The three-state distinction runs through everything here. A key that is
//! absent is not the same as a key that is present and empty, and neither is
//! the same as a key whose value failed to decode. The wire carries `null` for
//! the first and `""` for the second, and a patch has to be able to set either.

use std::collections::BTreeMap;

use pdfium::{Document, Trapped};

use crate::cancel::CancelToken;
use crate::clock::pdf_date_to_iso;
use crate::dto::DocumentMetadataTrapped;
use crate::error::{EngineError, EngineErrorCode};

/// The document's Info dictionary, as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub producer: Option<String>,
    pub creator: Option<String>,
    /// ISO 8601, converted from `/CreationDate` keeping the offset the file
    /// wrote ([`pdf_date_to_iso`]).
    pub created_at: Option<String>,
    /// ISO 8601, from `/ModDate`.
    pub modified_at: Option<String>,
    pub trapped: DocumentMetadataTrapped,
}

/// The Info dict's other keys (`CustomMetadata`), as `metadata.readCustom`
/// answers them.
///
/// A `BTreeMap` rather than insertion order, because the wire is a JSON
/// object and two implementations must not disagree about key order in a
/// serialized snapshot.
pub type CustomMetadata = BTreeMap<String, String>;

/// The reserved keys, read as fields rather than as custom data.
const STANDARD_KEYS: [&str; 8] =
    ["Title", "Author", "Subject", "Keywords", "Producer", "Creator", "CreationDate", "ModDate"];

/// Read the Info dictionary.
pub fn read_metadata(
    document: &Document,
    cancel: &CancelToken,
) -> Result<DocumentMetadata, EngineError> {
    cancel.check()?;

    let mut values = Vec::with_capacity(STANDARD_KEYS.len());
    for key in STANDARD_KEYS {
        cancel.check()?;
        values.push(meta_text(document, key)?);
    }
    let mut values = values.into_iter();
    let title = values.next().flatten();
    let author = values.next().flatten();
    let subject = values.next().flatten();
    let keywords = values.next().flatten();
    let producer = values.next().flatten();
    let creator = values.next().flatten();
    let creation_raw = values.next().flatten();
    let mod_raw = values.next().flatten();

    cancel.check()?;
    let trapped = match document.meta_trapped() {
        Trapped::True => DocumentMetadataTrapped::True,
        Trapped::False => DocumentMetadataTrapped::False,
        Trapped::Unknown => DocumentMetadataTrapped::Unknown,
    };

    Ok(DocumentMetadata {
        title,
        author,
        subject,
        keywords,
        producer,
        creator,
        // A date PDFium reports but cannot be parsed becomes `None` rather
        // than an error. A malformed `/CreationDate` is a bad document, not a
        // reason to refuse the other nine fields.
        created_at: creation_raw.as_deref().and_then(pdf_date_to_iso),
        modified_at: mod_raw.as_deref().and_then(pdf_date_to_iso),
        trapped,
    })
}

/// Read the Info dict's custom keys: every key but the standard ones
/// (`MetadataReader.readCustom`).
pub fn read_custom_metadata(
    document: &Document,
    cancel: &CancelToken,
) -> Result<CustomMetadata, EngineError> {
    cancel.check()?;
    let mut custom = BTreeMap::new();
    let keys = document.custom_meta_keys().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading the custom metadata keys failed: {e}"),
        )
    })?;
    for key in keys {
        cancel.check()?;
        // A key with no value is dropped rather than carried as null: the wire
        // type is a map of strings, and a null there would be a shape no
        // consumer expects.
        if let Some(value) = meta_text(document, &key)? {
            custom.insert(key, value);
        }
    }
    Ok(custom)
}

fn meta_text(document: &Document, key: &str) -> Result<Option<String>, EngineError> {
    document.meta_text(key).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading /{key} failed: {e}"),
        )
    })
}
