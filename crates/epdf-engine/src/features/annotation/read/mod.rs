//! The readers that walk PDFium.
//!
//! G7. The half of the port that touches a document, as opposed to the pure
//! tables in [`super::vocabulary`] and its neighbours.
//!
//! Ported so far: the base shell every annotation carries, and the
//! `unsupported` fallback. Every subtype therefore reads as a correct shell
//! with no subtype-specific fields, which is a state the wire format already
//! has a name for. [`registry::FALLS_BACK_TO_UNSUPPORTED`] lists what is left,
//! and a test asserts that list rather than leaving it to a comment.

pub mod base;
pub mod dto;
pub mod embed_metadata;
pub mod identity;
pub mod page;
pub mod registry;
pub mod relationship;
pub mod unsupported;

use pdfium::PdfiumError;

use crate::error::{EngineError, EngineErrorCode};

/// Map a dictionary read that PDFium refused.
///
/// There is no blanket `From<PdfiumError>` in this engine on purpose: each
/// caller decides what a failure means, and most of them mean something more
/// specific than "the runtime said no". Here it does mean that. A key that is
/// absent is not a failure, it is `Ok(None)`, so anything reaching this is the
/// runtime declining to answer a question about a dictionary it has open,
/// which is not a fact about the document.
pub(crate) fn dictionary_read<T>(
    key: &str,
    result: Result<T, PdfiumError>,
) -> Result<T, EngineError> {
    result.map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading /{key} of an annotation failed: {e}"),
        )
    })
}
