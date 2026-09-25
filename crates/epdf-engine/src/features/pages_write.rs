//! Reordering, rotating and removing pages.
//!
//! G13. Ports `services/src/features/pages/PagesMutator.ts`.
//!
//! # A page's identity is its object number, and a move does not change it
//!
//! There is no weak page reference in this engine. A page is addressed by
//! its PDF object number, which survives every reorder, so there is no
//! document-level revision token and nothing for a client to re-resolve
//! after a move.
//!
//! Per-page revision tokens do NOT bump on a move either, and that is the
//! point of doing it this way. `FPDF_MovePages` rewrites pointers in the
//! page tree and leaves each page's `/Annots` untouched, so a caller holding
//! a weak annotation reference by INDEX still holds a valid one after the
//! shuffle. Shuffling pages must not silently break a pending highlight.
//!
//! # Every operation reads the layout back
//!
//! A reorder changes every page's index, a rotation changes what a page
//! measures, and a delete changes both for everything after it. There is no
//! useful subset to report, so each returns the whole list.

use serde::{Deserialize, Serialize};

use pdfium::Document;

use super::pages::{PageListSnapshot, list_pages};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::PdfRotation;
use crate::identity::PageObjectNumber;

/// What a page mutation leaves behind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMutationResult {
    pub layout: PageListSnapshot,
}

/// Reorder pages.
///
/// The named pages are detached and re-inserted as one contiguous block at
/// `destination`, in the order given. `destination` is a position in the
/// POST-REMOVAL index space, which is the only frame in which the answer is
/// unambiguous: moving pages 0 and 1 to index 1 of the original list would
/// otherwise have two readings.
pub fn move_pages(
    document: &Document,
    pages: &[PageObjectNumber],
    destination: u32,
    cancel: &CancelToken,
) -> Result<PageMutationResult, EngineError> {
    cancel.check()?;
    require_distinct("pages.move", pages)?;
    let indices = resolve_indices(document, pages)?;

    let total = document.page_count();
    let remaining = total - indices.len() as u32;
    if destination > remaining {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "pages.move destination {destination} is past the end: {} pages remain \
                 after the block is detached",
                remaining
            ),
        ));
    }

    let signed: Vec<i32> = indices.iter().map(|index| *index as i32).collect();
    if !document.move_pages(&signed, destination as i32) {
        // `FPDF_MovePages` validates the whole request before touching the
        // tree, so a refusal means nothing moved and there is nothing to
        // describe beyond what was asked for.
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("PDFium refused the move of {signed:?} to index {destination}"),
        ));
    }
    read_back(document, cancel)
}

/// Rotate pages.
pub fn rotate_pages(
    document: &Document,
    pages: &[PageObjectNumber],
    rotation: PdfRotation,
    cancel: &CancelToken,
) -> Result<PageMutationResult, EngineError> {
    cancel.check()?;
    require_distinct("pages.rotate", pages)?;
    // Resolved BEFORE anything is written, so a list naming one page that
    // does not exist rotates none of the others.
    resolve_indices(document, pages)?;

    for page in pages {
        cancel.check()?;
        if !document.set_page_rotation_by_object_number(page.get(), rotation.quarter_turns()) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("PDFium refused to rotate page {} after it resolved", page.get()),
            ));
        }
    }
    read_back(document, cancel)
}

/// Remove pages.
///
/// A deleted page's object number is RETIRED: the fork nulls the object
/// rather than freeing the slot, so per-page state a caller kept keyed by
/// object number can never silently attach to an unrelated future page.
pub fn delete_pages(
    document: &Document,
    pages: &[PageObjectNumber],
    cancel: &CancelToken,
) -> Result<PageMutationResult, EngineError> {
    cancel.check()?;
    require_distinct("pages.delete", pages)?;
    resolve_indices(document, pages)?;

    // A document with no pages is not a document. PDFium would let the last
    // one go and produce a file nothing opens.
    let total = document.page_count();
    if pages.len() as u32 >= total {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "pages.delete would remove every page ({} of {total}); a document must \
                 keep at least one",
                pages.len()
            ),
        ));
    }

    for page in pages {
        cancel.check()?;
        if !document.delete_page_by_object_number(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("PDFium refused to delete page {} after it resolved", page.get()),
            ));
        }
    }
    read_back(document, cancel)
}

/// Every page named, and each named once.
///
/// A duplicate is refused rather than deduplicated. In a move it would make
/// the destination ambiguous; in a delete the second one names a page that
/// is already gone. Either way the caller meant something this cannot do.
fn require_distinct(operation: &str, pages: &[PageObjectNumber]) -> Result<(), EngineError> {
    if pages.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{operation} needs at least one page"),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for page in pages {
        if !seen.insert(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("{operation} names page {} twice", page.get()),
            ));
        }
    }
    Ok(())
}

/// Where each page sits today.
///
/// Resolved in one pass over the page list rather than one lookup per page,
/// and resolved up front: a list with one bad entry must change nothing.
fn resolve_indices(
    document: &Document,
    pages: &[PageObjectNumber],
) -> Result<Vec<u32>, EngineError> {
    let count = document.page_count();
    let mut positions = std::collections::BTreeMap::new();
    for index in 0..count {
        positions.insert(document.page_object_number_by_index(index), index);
    }
    pages
        .iter()
        .map(|page| {
            positions.get(&page.get()).copied().ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("no page with object number {}", page.get()),
                )
            })
        })
        .collect()
}

fn read_back(document: &Document, cancel: &CancelToken) -> Result<PageMutationResult, EngineError> {
    Ok(PageMutationResult { layout: list_pages(document, cancel)? })
}
