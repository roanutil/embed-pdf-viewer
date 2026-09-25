//! `/PieceInfo`: an application's private data on a page or the catalog.
//!
//! G8. Ports the read half of
//! `services/src/features/pieceinfo/PieceInfoAccessor.ts`. Operations:
//! `pieceInfo.read`, `pieceInfo.applications`.
//!
//! §14.5 lets an application stash whatever it likes under its own name, and
//! most of what is there was written by software nobody here controls. So the
//! read is a CONSTRAINED vocabulary over an unconstrained dictionary: five
//! value shapes are surfaced and everything else reads as
//! [`PieceInfoEntry::Unknown`], present and unreadable, rather than being
//! dropped or guessed at.
//!
//! The tag is what makes the vocabulary lossless where it applies. A PDF name
//! and a PDF string both arrive as text, and the difference between `/Draft`
//! and `(Draft)` is not recoverable from the characters, so it travels
//! alongside them.

use pdfium::{Document, PieceInfoHolder, object_type};
use serde::{Deserialize, Serialize};

use crate::cancel::CancelToken;
use crate::clock::pdf_date_to_iso;
use crate::error::{EngineError, EngineErrorCode};

/// One `/Private` value, as read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PieceInfoEntry {
    String {
        value: String,
    },
    Name {
        value: String,
    },
    Number {
        value: f64,
    },
    Boolean {
        value: bool,
    },
    StringArray {
        value: Vec<String>,
    },
    /// In the file and outside the vocabulary. A write to a sibling key
    /// preserves it; nothing here can read it.
    Unknown,
}

/// One application's data on one holder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PieceInfoSnapshot {
    /// Keyed by the PDF key name. A `BTreeMap` rather than an insertion-order
    /// map: `/Private` is a dictionary, dictionaries are unordered, and
    /// sorting makes two reads of one document compare equal.
    pub entries: std::collections::BTreeMap<String, PieceInfoEntry>,
    /// The application dictionary's `/LastModified`, as ISO 8601.
    pub last_modified: Option<String>,
}

/// Every application the holder carries data for, in file order.
pub fn read_piece_info_applications(
    document: &Document,
    holder: PieceInfoHolder,
    cancel: &CancelToken,
) -> Result<Vec<String>, EngineError> {
    cancel.check()?;
    let count = document.piece_info_application_count(holder);
    let mut out = Vec::new();
    for index in 0..count {
        cancel.check()?;
        // A name that will not read is skipped rather than reported empty:
        // an empty application name addresses nothing, so listing one would
        // hand a caller a key it cannot use.
        if let Some(name) = ffi(document.piece_info_application_at(holder, index))? {
            out.push(name);
        }
    }
    Ok(out)
}

/// One application's data, or `None` when the holder has none for it.
///
/// `None` is not an empty snapshot. An application that has never written
/// here is a different thing from one that wrote and then cleared every key,
/// and a caller deciding whether to migrate old data needs to tell them
/// apart.
pub fn read_piece_info(
    document: &Document,
    holder: PieceInfoHolder,
    application: &str,
    cancel: &CancelToken,
) -> Result<Option<PieceInfoSnapshot>, EngineError> {
    cancel.check()?;
    require_application(application)?;

    if !ffi(document.has_piece_info(holder, application))? {
        return Ok(None);
    }

    let mut entries = std::collections::BTreeMap::new();
    let count = ffi(document.piece_info_key_count(holder, application))?;
    for index in 0..count {
        cancel.check()?;
        let Some(key) = ffi(document.piece_info_key_at(holder, application, index))? else {
            continue;
        };
        let entry = read_entry(document, holder, application, &key)?;
        entries.insert(key, entry);
    }

    let last_modified = ffi(document.piece_info_last_modified(holder, application))?;
    Ok(Some(PieceInfoSnapshot {
        entries,
        // A `/LastModified` the date parser refuses reads as absent rather
        // than failing the whole snapshot. It is bookkeeping about the data,
        // not the data.
        last_modified: last_modified.as_deref().and_then(pdf_date_to_iso),
    }))
}

/// Decode one value by its object type.
fn read_entry(
    document: &Document,
    holder: PieceInfoHolder,
    application: &str,
    key: &str,
) -> Result<PieceInfoEntry, EngineError> {
    Ok(match ffi(document.piece_info_value_type(holder, application, key))? {
        object_type::STRING => ffi(document.piece_info_string(holder, application, key))?
            .map_or(PieceInfoEntry::Unknown, |value| PieceInfoEntry::String { value }),
        object_type::NAME => ffi(document.piece_info_name(holder, application, key))?
            .map_or(PieceInfoEntry::Unknown, |value| PieceInfoEntry::Name { value }),
        object_type::NUMBER => ffi(document.piece_info_number(holder, application, key))?
            .map_or(PieceInfoEntry::Unknown, |value| PieceInfoEntry::Number {
                value: f64::from(value),
            }),
        object_type::BOOLEAN => ffi(document.piece_info_boolean(holder, application, key))?
            .map_or(PieceInfoEntry::Unknown, |value| PieceInfoEntry::Boolean { value }),
        object_type::ARRAY => read_string_array(document, holder, application, key)?,
        // Every other object type, a stream and a dictionary included. In the
        // file, preserved by a write to a sibling key, and not readable here.
        _ => PieceInfoEntry::Unknown,
    })
}

fn read_string_array(
    document: &Document,
    holder: PieceInfoHolder,
    application: &str,
    key: &str,
) -> Result<PieceInfoEntry, EngineError> {
    let count = ffi(document.piece_info_string_array_count(holder, application, key))?;
    // Negative means the array holds something that is not a text string.
    // Zero means an empty array, which is a value this vocabulary has, so the
    // two cannot be collapsed.
    let Ok(count) = u32::try_from(count) else {
        return Ok(PieceInfoEntry::Unknown);
    };

    let mut value = Vec::with_capacity(count as usize);
    for index in 0..count {
        value.push(
            ffi(document.piece_info_string_array_at(holder, application, key, index))?
                .unwrap_or_default(),
        );
    }
    Ok(PieceInfoEntry::StringArray { value })
}

/// An application name has to be usable as a dictionary key.
///
/// Empty is refused rather than treated as the catalog's own data: `/PieceInfo
/// <</  ...>>` is not a thing, and a caller passing an empty string has a bug
/// this names rather than an intent to be guessed.
fn require_application(application: &str) -> Result<(), EngineError> {
    if application.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a piece-info application name cannot be empty",
        ));
    }
    Ok(())
}

fn ffi<T>(result: Result<T, pdfium::PdfiumError>) -> Result<T, EngineError> {
    result.map_err(|e| {
        // An argument this layer refuses is the caller's, and anything else
        // is the runtime declining. They are different things to a caller and
        // so get different codes.
        let code = match e {
            pdfium::PdfiumError::InvalidArgument(_) => EngineErrorCode::InvalidArg,
            _ => EngineErrorCode::RuntimeUnavailable,
        };
        EngineError::with_message(code, format!("reading /PieceInfo failed: {e}"))
    })
}

/// What a patch sets one key to.
///
/// Six states, and `Clear` is one of them. Removing a key and setting it to
/// an empty string are different edits here, unlike in `/Info`: the fork's
/// piece-info setters and its clear are separate calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PieceInfoPatchValue {
    /// `null` removes the key.
    Clear,
    Boolean(bool),
    Number(f64),
    String(String),
    StringArray(Vec<String>),
    /// A PDF name, which is not a string: `{ "name": "Draft" }` on the
    /// wire, because a reader that took it for text would quote it.
    Name {
        name: String,
    },
}

/// A set of keys to write, under one application.
pub type PieceInfoPatch = std::collections::BTreeMap<String, PieceInfoPatchValue>;

/// Write a patch.
///
/// # This is not a transaction
///
/// A native setter that refuses on the fourth key leaves the first three
/// written. Everything a caller can get wrong is checked first, so what is
/// left below the apply boundary is PDFium refusing something that passed;
/// unwinding would mean restoring values this layer never read.
pub fn update_piece_info(
    document: &Document,
    holder: PieceInfoHolder,
    application: &str,
    patch: &PieceInfoPatch,
    clock: &dyn crate::clock::Clock,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    cancel.check()?;
    if application.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a piece-info application needs a name",
        ));
    }
    if patch.is_empty() {
        return Ok(());
    }
    for (key, value) in patch {
        validate(key, value)?;
    }

    // The apply boundary. Nothing below it is the caller's mistake.
    cancel.check()?;
    // ONE date for the whole patch, however many keys it touches. §14.5
    // lets a consumer discard an application's data when the document has
    // changed since it last wrote, and that check compares this date with
    // the document's; a batch stamped across two seconds would look like
    // two edits.
    let last_modified = crate::clock::format_pdf_date(clock.now()).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::Unknown, "the clock produced no PDF date")
    })?;

    for (key, value) in patch {
        let native = match value {
            PieceInfoPatchValue::Clear => pdfium::PieceInfoValue::Clear,
            PieceInfoPatchValue::Boolean(flag) => pdfium::PieceInfoValue::Boolean(*flag),
            PieceInfoPatchValue::Number(number) => pdfium::PieceInfoValue::Number(*number as f32),
            PieceInfoPatchValue::String(text) => pdfium::PieceInfoValue::String(text.clone()),
            PieceInfoPatchValue::StringArray(values) => {
                pdfium::PieceInfoValue::StringArray(values.clone())
            }
            PieceInfoPatchValue::Name { name } => pdfium::PieceInfoValue::Name(name.clone()),
        };
        let written = document
            .set_piece_info(holder, application, key, &native, &last_modified)
            .map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!("a piece-info string is not one a PDF can hold: {e}"),
                )
            })?;
        if !written {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("PDFium refused the piece-info key '{key}' after it validated"),
            ));
        }
    }
    Ok(())
}

/// Remove an application's data entirely.
pub fn clear_piece_info(
    document: &Document,
    holder: PieceInfoHolder,
    application: &str,
    cancel: &CancelToken,
) -> Result<(), EngineError> {
    cancel.check()?;
    if application.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a piece-info application needs a name",
        ));
    }
    let cleared = document.clear_piece_info(holder, application).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("the application name is not one a PDF can hold: {e}"),
        )
    })?;
    if !cleared {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("PDFium refused to clear the piece-info application '{application}'"),
        ));
    }
    Ok(())
}

/// Everything a patch entry can be wrong about.
fn validate(key: &str, value: &PieceInfoPatchValue) -> Result<(), EngineError> {
    if key.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "a piece-info key cannot be empty",
        ));
    }
    match value {
        // A non-finite number has no PDF spelling, and PDFium would write
        // whatever the cast produced.
        PieceInfoPatchValue::Number(number) if !number.is_finite() => {
            Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("the piece-info number for '{key}' is not finite"),
            ))
        }
        // An empty name is not a name: `/` alone is not valid syntax.
        PieceInfoPatchValue::Name { name } if name.is_empty() => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("the piece-info name for '{key}' cannot be empty"),
        )),
        _ => Ok(()),
    }
}
