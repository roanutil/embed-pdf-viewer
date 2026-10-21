//! Changing the document information dictionary.
//!
//! G14. Ports `services/src/features/metadata/MetadataMutator.ts` and
//! `internal/write/*.ts`. The writes run as change ops
//! ([`crate::features::changes`]): `metadata.update` and
//! `metadata.updateCustom` are each a one-op change, and `document.apply`
//! takes both as ops.
//!
//! # A clear and an empty value are different edits
//!
//! The wire has three states and the fork keeps all three: `null` removes
//! the entry (`EPDF_SetMetaText` with a null value) and `''` writes a
//! present-but-empty one, which reads back as `''`. The reader tells them
//! apart through `EPDF_HasMetaText`. The same holds per key of a custom
//! patch.
//!
//! # A custom key cannot shadow a standard one
//!
//! The custom map writes straight into `/Info`, which is the same dictionary
//! the standard fields live in. A custom `Title` would overwrite the
//! structured field, so a custom key that is a standard key, or that a PDF
//! name cannot hold, is refused with `InvalidArg` naming it, before anything
//! is written (`applyCustomMetadataPatch.ts`, a4b8259f7).

use serde::{Deserialize, Serialize};

use pdfium::Document;

use super::metadata::{CustomMetadata, DocumentMetadata, read_custom_metadata, read_metadata};
use crate::cancel::CancelToken;
use crate::clock::format_pdf_date;
use crate::dto::{CustomMetadataPatch, MetadataPatch, Patch};
use crate::error::{EngineError, EngineErrorCode};
use crate::mutation::MutationMeta;

/// What a `metadata.update` leaves behind (`MetadataUpdateResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataUpdateResult {
    /// The standard fields, re-read: the shape `metadata.read` answers.
    pub metadata: DocumentMetadata,
    /// No page and no cache delta: a metadata write touches no page.
    pub meta: MutationMeta,
}

/// What a `metadata.updateCustom` leaves behind
/// (`CustomMetadataUpdateResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomMetadataUpdateResult {
    /// The custom keys, re-read: the shape `metadata.readCustom` answers.
    pub custom: CustomMetadata,
    pub meta: MutationMeta,
}

/// The `/Info` keys the structured fields own (`STANDARD_INFO_KEYS`).
///
/// `/Trapped` is on the list even though it is not a text field, because a
/// custom write would put a string where a name belongs.
const STANDARD_INFO_KEYS: [&str; 9] = [
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

/// Why a custom key can't be written, or `None` when it can
/// (`customKeyProblem`): it must be writable as a PDF name (non-empty
/// printable ASCII, at most 127 characters, no leading slash) and must not
/// be a standard key. The messages are upstream's.
fn custom_key_problem(key: &str) -> Option<String> {
    if STANDARD_INFO_KEYS.contains(&key) {
        return Some(format!("'{key}' is a standard key; set it with metadata.update"));
    }
    // Upstream counts UTF-16 units; a key past ASCII is refused below either
    // way, so counting chars here answers the same.
    if key.is_empty() || key.chars().count() > 127 || key.starts_with('/') {
        return Some(format!(
            "'{key}' can't be a custom key: 1 to 127 characters, no leading slash"
        ));
    }
    if key.chars().any(|c| !(' '..='~').contains(&c)) {
        return Some(format!("'{key}' can't be a custom key: printable ASCII only"));
    }
    None
}

/// Apply a [`MetadataPatch`] to the Info dict in place
/// (`applyMetadataPatch`): an absent field stays, `null` clears it, a value
/// sets it (dates in PDF date syntax, keeping their offset); `''` is a value.
///
/// A native write that fails is `Unknown`, so a caller never reports a
/// partly applied edit as success. Undoing what was written before it is the
/// change's job ([`crate::features::changes`]).
pub fn apply_metadata_patch(document: &Document, patch: &MetadataPatch) -> Result<(), EngineError> {
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

    // The two dates arrive as ISO 8601 and are stored as PDF dates, in the
    // offset they arrived with.
    for (value, key) in [(&patch.created_at, "CreationDate"), (&patch.modified_at, "ModDate")] {
        let converted: Patch<String> = match value {
            Patch::Keep => Patch::Keep,
            Patch::Clear => Patch::Clear,
            Patch::Set(iso) => Patch::Set(format_pdf_date(iso.as_str())?),
        };
        write_text(document, key, as_str_patch(&converted))?;
    }

    if let Some(trapped) = patch.trapped
        && !document.set_meta_trapped(native_trapped(trapped))
    {
        return Err(failed("Trapped"));
    }
    Ok(())
}

/// Apply a [`CustomMetadataPatch`] in place (`applyCustomMetadataPatch`): a
/// string sets its key (`''` is a value), `null` removes it, a key left out
/// stays. A standard or malformed key is `InvalidArg` naming it, before
/// anything is written.
pub fn apply_custom_metadata_patch(
    document: &Document,
    patch: &CustomMetadataPatch,
) -> Result<(), EngineError> {
    check_custom_keys(patch.0.keys())?;
    for (key, value) in &patch.0 {
        let value = match value {
            Some(text) => Patch::Set(text.as_str()),
            None => Patch::Clear,
        };
        write_text(document, key, value)?;
    }
    Ok(())
}

/// Refuse a custom patch naming a key it cannot write: `InvalidArg` naming
/// the first such key in the order given, as `applyCustomMetadataPatch`
/// checks before writing.
pub fn check_custom_keys<'a>(
    keys: impl IntoIterator<Item = &'a String>,
) -> Result<(), EngineError> {
    for key in keys {
        if let Some(problem) = custom_key_problem(key) {
            return Err(EngineError::with_message(EngineErrorCode::InvalidArg, problem)
                .with_details(serde_json::json!({ "field": key })));
        }
    }
    Ok(())
}

/// Apply a patch and re-read the standard fields (`MetadataMutator.update`
/// without its `meta`, which the change adds).
pub fn update_metadata(
    document: &Document,
    patch: &MetadataPatch,
    cancel: &CancelToken,
) -> Result<DocumentMetadata, EngineError> {
    cancel.check()?;
    apply_metadata_patch(document, patch)?;
    cancel.check()?;
    read_metadata(document, cancel)
}

/// Apply a custom patch and re-read the custom keys
/// (`MetadataMutator.updateCustom` without its `meta`).
pub fn update_custom_metadata(
    document: &Document,
    patch: &CustomMetadataPatch,
    cancel: &CancelToken,
) -> Result<CustomMetadata, EngineError> {
    cancel.check()?;
    apply_custom_metadata_patch(document, patch)?;
    cancel.check()?;
    read_custom_metadata(document, cancel)
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

fn write_text(document: &Document, key: &str, value: Patch<&str>) -> Result<(), EngineError> {
    let written = match value {
        Patch::Keep => return Ok(()),
        Patch::Clear => document.set_meta_text(key, None),
        Patch::Set(text) => document.set_meta_text(key, Some(text)),
    };
    match written {
        Ok(true) => Ok(()),
        Ok(false) => Err(failed(key)),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("/{key} is not a name a PDF can hold: {e}"),
        )),
    }
}

/// `failed to write metadata <key>`, as upstream words it.
fn failed(key: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("failed to write metadata {key}"))
}

#[cfg(test)]
mod tests {
    use super::custom_key_problem;

    #[test]
    fn a_custom_key_is_refused_for_what_a_pdf_name_cannot_hold() {
        assert!(custom_key_problem("Department").is_none());
        assert!(custom_key_problem("Case-Id").is_none());
        assert!(custom_key_problem("Title").unwrap().contains("standard key"));
        assert!(custom_key_problem("Trapped").unwrap().contains("standard key"));
        assert!(custom_key_problem("/Leading").unwrap().contains("no leading slash"));
        assert!(custom_key_problem("").unwrap().contains("1 to 127"));
        assert!(custom_key_problem(&"k".repeat(128)).unwrap().contains("1 to 127"));
        assert!(custom_key_problem(&"k".repeat(127)).is_none());
        assert!(custom_key_problem("Café").unwrap().contains("printable ASCII"));
        assert!(custom_key_problem("tab\there").unwrap().contains("printable ASCII"));
    }
}
