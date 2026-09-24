//! Exporting a subset of pages as a standalone PDF.
//!
//! G9. Replaces `services/src/features/pages/PagesExtractor.ts`. Operation:
//! `pages.extract`.
//!
//! A read over the session, not a mutation. Pages are copied into a scratch
//! document that is closed before this returns, so the source is untouched:
//! no revision bump, no registry refresh, no layer artifact.

use pdfium::Library;

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::identity::PageObjectNumber;
use crate::session::document::DocumentSession;

/// Copy the given pages, in the caller's order, into a new document.
///
/// Addressed by object number rather than index, like everything else that
/// crosses a boundary, and resolved against the session's registry so a page
/// the document does not have is `NotFound` before PDFium is asked.
///
/// The caller's order is preserved: extracting `[3, 1]` produces those two
/// pages in that order, not in document order. A caller reordering pages this
/// way is using `pages.extract` as intended.
pub fn extract_pages(
    session: &mut DocumentSession,
    page_object_numbers: &[PageObjectNumber],
    cancel: &CancelToken,
) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    if page_object_numbers.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.extract requires at least one page",
        ));
    }

    // Duplicates are refused rather than deduplicated. A caller that asked for
    // the same page twice either meant two copies, which PDFium will not do
    // through this call, or has a bug; silently collapsing the request would
    // return a document with fewer pages than were asked for.
    let mut seen = std::collections::BTreeSet::new();
    for number in page_object_numbers {
        if !seen.insert(*number) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("pages.extract was given duplicate page object number {number}"),
            ));
        }
    }

    // Resolved to CURRENT indices through the registry, so a document whose
    // pages have been reordered extracts the pages the caller named rather
    // than whatever now sits at those positions.
    let mut indices = Vec::with_capacity(page_object_numbers.len());
    for number in page_object_numbers {
        cancel.check()?;
        indices.push(session.record_by_object_number(*number)?.page_index);
    }

    cancel.check()?;
    session.document()?.document().extract_pages(Library::acquire(), &indices).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::DocOpenFailed,
            format!("failed to extract pages: {e}"),
        )
    })
}
