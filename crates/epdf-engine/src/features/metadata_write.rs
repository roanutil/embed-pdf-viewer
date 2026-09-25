//! Changing the document information dictionary.
//!
//! G14. Ports `services/src/features/metadata/MetadataMutator.ts` and
//! `internal/write/*.ts`.
//!
//! # A clear and an empty value are the same edit here, and the port order
//! says they should not be
//!
//! The wire has three states and `Clear` is one of them, so the type can say
//! "remove the title" and "set the title to nothing" separately. The FORK
//! cannot: `EPDF_SetMetaText` removes the entry for an empty string as
//! readily as for a null, so both leave a document with no `/Title`.
//! `a_clear_and_an_empty_value_are_the_same_edit` records that, against the
//! dictionary rather than the DTO, because the reader normalizes a
//! present-but-empty entry to absent and would agree either way.
//!
//! The TypeScript's `writeMetaText` claims it dropped "v2's empty-string
//! removes quirk". It did not: the quirk is in the fork, below both
//! engines.
//!
//! # A custom key cannot shadow a standard one
//!
//! The custom map writes straight into `/Info`, which is the same dictionary
//! the standard fields live in. A custom `Title` would overwrite the
//! structured field and the reader would report it as the title, so the nine
//! reserved keys are skipped rather than refused: a patch carrying one is a
//! caller using the wrong door, and the right door is next to it.

use serde::{Deserialize, Serialize};

use pdfium::Document;

use super::metadata::{DocumentMetadata, read_metadata};
use crate::cancel::CancelToken;
use crate::clock::{format_pdf_date, iso_8601_to_timestamp};
use crate::dto::{MetadataPatch, Patch};
use crate::error::{EngineError, EngineErrorCode};

/// What an update leaves behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdateResult {
    /// The whole dictionary, re-read. A patch can touch any subset and the
    /// reader normalizes what it finds, so there is no useful smaller
    /// answer.
    pub metadata: DocumentMetadata,
}

/// The `/Info` keys the structured fields own.
///
/// A custom key matching one of these is skipped. `/Trapped` is on the list
/// even though it is not a text field, because a custom write would put a
/// string where a name belongs.
const RESERVED_KEYS: [&str; 9] = [
    "Title",
    "Author",
    "Subject",
    "Keywords",
    "Producer",
    "Creator",
    "CreationDate",
    "ModDate",
    "Trapped",
];

/// Whether a custom key can be written.
///
/// A PDF name, in practice: printable ASCII, not empty, not longer than 127
/// characters, and not starting with the slash a caller might think is part
/// of the name. Anything else is skipped rather than refused, on the same
/// terms as a reserved key.
fn is_writable_custom_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 127
        && !key.starts_with('/')
        && !RESERVED_KEYS.contains(&key)
        && key.bytes().all(|byte| (0x20..=0x7E).contains(&byte))
}

/// Apply a patch.
pub fn update_metadata(
    document: &Document,
    patch: &MetadataPatch,
    cancel: &CancelToken,
) -> Result<MetadataUpdateResult, EngineError> {
    cancel.check()?;

    for (value, key) in [
        (&patch.title, "Title"),
        (&patch.author, "Author"),
        (&patch.subject, "Subject"),
        (&patch.keywords, "Keywords"),
        (&patch.producer, "Producer"),
        (&patch.creator, "Creator"),
    ] {
        write_text(document, key, as_str_patch(value))?;
    }

    // The two dates arrive as ISO 8601 and are stored as PDF dates. The
    // conversion is here rather than at the boundary because the wire's
    // vocabulary and the file's are different vocabularies, and only one of
    // them is this engine's to define.
    for (value, key) in [(&patch.created, "CreationDate"), (&patch.modified, "ModDate")] {
        let converted: Patch<String> = match value {
            Patch::Keep => Patch::Keep,
            Patch::Clear => Patch::Clear,
            Patch::Set(iso) => Patch::Set(
                iso_8601_to_timestamp(iso.as_str()).and_then(format_pdf_date).ok_or_else(|| {
                    EngineError::with_message(
                        EngineErrorCode::InvalidArg,
                        format!("{key} is not a date this engine can write: {iso}"),
                    )
                })?,
            ),
        };
        write_text(document, key, as_str_patch(&converted))?;
    }

    if let Some(trapped) = patch.trapped
        && !document.set_meta_trapped(native_trapped(trapped))
    {
        return Err(refused("Trapped"));
    }

    if let Some(custom) = patch.custom.as_ref() {
        for (key, value) in custom {
            if !is_writable_custom_key(key) {
                continue;
            }
            write_text(document, key, patch_of(value))?;
        }
    }

    cancel.check()?;
    Ok(MetadataUpdateResult { metadata: read_metadata(document, cancel)? })
}

/// A patch of owned strings as one of borrowed ones.
fn as_str_patch(value: &Patch<String>) -> Patch<&str> {
    match value {
        Patch::Keep => Patch::Keep,
        Patch::Clear => Patch::Clear,
        Patch::Set(text) => Patch::Set(text.as_str()),
    }
}

/// The wire's three states as PDFium's.
///
/// `NotSet` is absent from both vocabularies, which is what makes this
/// total: a patch that wants the entry left alone omits the field.
fn native_trapped(value: crate::dto::DocumentMetadataTrapped) -> pdfium::Trapped {
    use crate::dto::DocumentMetadataTrapped as T;
    match value {
        T::True => pdfium::Trapped::True,
        T::False => pdfium::Trapped::False,
        T::Unknown => pdfium::Trapped::Unknown,
    }
}

/// A custom map entry, which is two-state per key: a value or a removal.
fn patch_of(value: &Option<String>) -> Patch<&str> {
    match value {
        Some(text) => Patch::Set(text.as_str()),
        None => Patch::Clear,
    }
}

fn write_text(document: &Document, key: &str, value: Patch<&str>) -> Result<(), EngineError> {
    let written = match value {
        Patch::Keep => return Ok(()),
        Patch::Clear => document.set_meta_text(key, None),
        Patch::Set(text) => document.set_meta_text(key, Some(text)),
    };
    match written {
        Ok(true) => Ok(()),
        Ok(false) => Err(refused(key)),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("/{key} is not a name a PDF can hold: {e}"),
        )),
    }
}

fn refused(key: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("PDFium refused /{key}"))
}
