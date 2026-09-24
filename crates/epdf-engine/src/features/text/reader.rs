//! Reading a page's text.
//!
//! G5. Replaces `services/src/features/text/PageTextReader.ts`. Operation:
//! `pages.text`.
//!
//! Both halves of the snapshot come from the fork's batched calls, which share
//! one encoding authority, so the text and the map cannot disagree about where
//! a character landed. They are then cross-checked against the invariants in
//! [`super::charmap`] anyway: a disagreement is an engine bug, and shipping a
//! snapshot that slices wrong is worse than failing.

use pdfium::PdfiumError;

use super::charmap::{CharMapAnchor, PageTextSnapshot, char_map_violation};
use crate::access::PageAccess;
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};

/// Read a page's text and the map between its index spaces.
///
/// The port order says to gate this on correctness rather than speed: text
/// extraction is already the coarse path, and a snapshot that is fast and
/// subtly misaligned is worth nothing.
pub fn read_page_text(
    page: &dyn PageAccess,
    cancel: &CancelToken,
) -> Result<PageTextSnapshot, EngineError> {
    cancel.check()?;
    let text_page = page.view().text().map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("FPDFText_LoadPage failed: {e}"),
        )
    })?;
    cancel.check()?;

    let char_count = text_page.char_count();
    let text = text_page.text_full().map_err(text_failure)?;
    cancel.check()?;
    let char_map: Vec<CharMapAnchor> = text_page
        .char_to_text_map()
        .map_err(text_failure)?
        .into_iter()
        .map(|(char, text)| CharMapAnchor::new(char, text))
        .collect();

    let snapshot = PageTextSnapshot { text, char_count, char_map };

    // The cross-check. Two calls that share an encoder should agree, and when
    // they do not the snapshot is unusable in a way no consumer can see: every
    // slice would return plausible text taken from the wrong characters.
    if let Some(violation) =
        char_map_violation(snapshot.char_count, snapshot.text_len_utf16(), &snapshot.char_map)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("char map invariant violated: {violation}"),
        ));
    }

    Ok(snapshot)
}

fn text_failure(error: PdfiumError) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::RuntimeUnavailable,
        format!("page text extraction failed: {error}"),
    )
}
