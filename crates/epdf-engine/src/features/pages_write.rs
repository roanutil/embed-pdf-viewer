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
use crate::features::annotation::write::delete::promote_inline_annotations;
use crate::geom::PdfRotation;
use crate::identity::{PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::session::document::DocumentSession;

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

/// `PagesMutator.setName`: register `name` for the page, removing
/// `replace` first when it is a different, non-empty name. Names are
/// layout: no page moves and nothing's identity changes.
pub fn set_name(
    document: &Document,
    name: &str,
    page: PageObjectNumber,
    replace: Option<&str>,
    cancel: &CancelToken,
) -> Result<PageMutationResult, EngineError> {
    cancel.check()?;
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.setName requires a non-empty name",
        ));
    }
    require_page(document, page)?;
    if let Some(old) = replace.filter(|r| !r.is_empty() && *r != name) {
        // The fork answers false for a key it did not have, which the
        // TypeScript ignores too: the rename still registers `name`.
        document.remove_named_page(old).map_err(named_page_error)?;
    }
    if !document.set_named_page(name, page.get()).map_err(named_page_error)? {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFDoc_SetNamedPage rejected '{name}' for page {}", page.get()),
        ));
    }
    read_back(document, cancel)
}

/// `PagesMutator.removeName`: drop one registration; the page stays.
pub fn remove_name(
    document: &Document,
    name: &str,
    cancel: &CancelToken,
) -> Result<PageMutationResult, EngineError> {
    cancel.check()?;
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.removeName requires a non-empty name",
        ));
    }
    if !document.remove_named_page(name).map_err(named_page_error)? {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("pages.removeName: no /Names /Pages registration '{name}'"),
        ));
    }
    read_back(document, cancel)
}

fn named_page_error(error: pdfium::PdfiumError) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, error.to_string())
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

/// That a page exists, without caring where.
///
/// Shared with the redaction applier, which resolves its pages up front for
/// the same reason every operation here does: a scope naming one page that
/// does not exist must change nothing.
pub(crate) fn require_page(document: &Document, page: PageObjectNumber) -> Result<(), EngineError> {
    resolve_indices(document, std::slice::from_ref(&page)).map(|_| ())
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

/// What an insert added, and what the list looks like now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInsertResult {
    /// The new pages, in the order they landed. Their object numbers are
    /// fresh and are resolved from the list AFTER the insert: they do not
    /// exist before it and cannot be predicted.
    #[serde(rename = "insertedPages", with = "crate::identity::page_refs")]
    pub inserted_page_object_numbers: Vec<PageObjectNumber>,
    pub layout: PageListSnapshot,
}

/// How many blank pages one call may create.
///
/// A ceiling rather than a limit on what the format allows: a larger batch
/// is a loop at the call site, and the number is shared with the server so
/// a client never discovers a lower one by surprise.
pub const INSERT_BLANK_MAX_COUNT: u32 = 100;

/// Create blank pages.
pub fn insert_blank_pages(
    document: &Document,
    width: f64,
    height: f64,
    count: u32,
    destination: Option<u32>,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    cancel.check()?;
    if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("a blank page needs positive finite dimensions, not {width}x{height}"),
        ));
    }
    if !(1..=INSERT_BLANK_MAX_COUNT).contains(&count) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("count {count} is outside 1..={INSERT_BLANK_MAX_COUNT}"),
        ));
    }
    let at = resolve_destination(document, destination, "pages.insertBlank")?;

    for created in 0..count {
        cancel.check()?;
        // `FPDFPage_New` clamps an index past the end to an append. The
        // guard above keeps that leniency out of this contract, and the
        // index here is in range by construction.
        let page = document.new_page((at + created) as i32, width, height);
        let Some(page) = page else {
            // Undo what this call made, so a failure leaves the document as
            // it was. Each undo removes the page at `at`, because removing
            // one shifts the rest down onto it.
            //
            // Unexercised: nothing provokes `FPDFPage_New` into refusing a
            // page whose dimensions and index this layer has already
            // checked. The alternative to writing it is a half-inserted
            // batch nobody asked for and nobody can name.
            for _ in 0..created {
                document.delete_page_at(at as i32);
            }
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!("PDFium refused blank page {} of {count}", created + 1),
            ));
        };
        // An empty content stream, so the saved file renders the same in a
        // viewer that expects one rather than showing nothing at all.
        //
        // Not observable here. A page with no `/Contents` lists, measures
        // and renders identically through this engine, so a build without
        // this line passes every vector; what it would change is what a
        // THIRD-PARTY viewer does with the saved file, which nothing in
        // this corpus opens.
        page.generate_content();
    }

    read_inserted(document, at, count, cancel)
}

/// Insert every page of another PDF.
///
/// The source document has to stay open for as long as this one might be
/// saved, which is why it is a borrow the caller holds rather than bytes
/// this function loads and drops. `FPDF_ImportPagesByIndex` does not fully
/// detach what it copies: an imported stream still reads through the source.
pub fn insert_pages_from(
    document: &Document,
    source: &Document,
    destination: Option<u32>,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    cancel.check()?;
    let count = source.page_count();
    // Unexercised: a PDF with no pages is one PDFium declines to open, so
    // no fixture can reach this. It stays because `import_pages` would then
    // be asked to copy nothing and would answer true, and an insert that
    // inserted nothing would report a fresh page number that is really the
    // page that was already there.
    if count == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the source document has no pages to insert",
        ));
    }
    let at = resolve_destination(document, destination, "pages.insert")?;

    // An empty index list means every page, in order.
    if !document.import_pages(source, &[], at as i32) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("PDFium refused the import of {count} pages at index {at}"),
        ));
    }
    read_inserted(document, at, count, cancel)
}

/// Where an insert goes, defaulting to the end.
fn resolve_destination(
    document: &Document,
    destination: Option<u32>,
    operation: &str,
) -> Result<u32, EngineError> {
    let total = document.page_count();
    let at = destination.unwrap_or(total);
    if at > total {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{operation} destination {at} is outside 0..={total}"),
        ));
    }
    Ok(at)
}

/// The layout, and which of its pages are the new ones.
fn read_inserted(
    document: &Document,
    at: u32,
    count: u32,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    let layout = list_pages(document, cancel)?;
    let inserted = layout
        .pages
        .iter()
        .skip(at as usize)
        .take(count as usize)
        .map(|page| page.page_object_number)
        .collect();
    Ok(PageInsertResult { inserted_page_object_numbers: inserted, layout })
}

/// What flattening one page did (`PageFlattenStatus`). A page that fails
/// fails the whole call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlattenStatus {
    /// The page's annotations are now content.
    Applied,
    /// There was nothing to flatten, which is not a failure: a page with no
    /// annotations is a page that is already flat.
    Unchanged,
}

/// One page's outcome (`PageFlattenItemResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenItem {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub status: FlattenStatus,
}

/// What a flatten did (`PageFlattenResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenResult {
    /// The request as it was made, in order. Kept so an event replay can
    /// see what was asked for and not only what happened.
    #[serde(rename = "pages", with = "crate::identity::page_refs")]
    pub page_object_numbers: Vec<PageObjectNumber>,
    pub usage: FlattenUsage,
    pub results: Vec<FlattenItem>,
    /// The pages the flatten changed, applied or promoted, in the order it
    /// first changed them, and the write's stamp.
    pub meta: MutationMeta,
}

/// Which appearance to bake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlattenUsage {
    Display,
    Print,
}

impl FlattenUsage {
    /// Which number the fork takes.
    ///
    /// Not observable on this corpus, and the corpus tried.
    /// `annotation_appearances.pdf` carries a square with `/F 36`, which
    /// prints and never shows on screen, and that is the one annotation the
    /// two usages should disagree about. They do not: both leave the same
    /// four annotations on the page and render the same pixels. So a build
    /// that passed `Display` for both passes every vector, and the mapping
    /// is written the way the fork's header names it rather than the way
    /// this corpus can prove.
    pub(crate) fn native(self) -> pdfium::FlattenUsage {
        match self {
            Self::Display => pdfium::FlattenUsage::Display,
            Self::Print => pdfium::FlattenUsage::Print,
        }
    }
}

/// A page whose flatten failed: `Unknown`, with `PagesFlattener`'s
/// messages.
fn flatten_failure(outcome: pdfium::FlattenOutcome) -> EngineError {
    let message = match outcome {
        pdfium::FlattenOutcome::Unknown(code) => {
            format!("native page flatten returned unexpected code {code}")
        }
        _ => String::from("native page flatten failed after preflight"),
    };
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

/// `PagesFlattener.requireUniquePages`, with its messages.
fn require_unique_pages(pages: &[PageObjectNumber]) -> Result<(), EngineError> {
    if pages.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.flatten requires at least one page",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for page in pages {
        if !seen.insert(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("pages.flatten was given duplicate page object number {}", page.get()),
            ));
        }
    }
    Ok(())
}

/// Turn each page's annotations into page content (`PagesFlattener`), in
/// the session's open write transaction, whose stamp the `meta` carries.
/// Each page's inline annotations become objects first, so the entries the
/// flatten removes from `/Annots` leave every other position alone.
///
/// Upstream is all or nothing: a page that fails, or a cancel, throws, and
/// the layer transaction takes back every page before it. A plain session
/// has no layer transaction (8.29), so the pages already changed stay
/// changed, and the sequence moves for them before the refusal goes out
/// (`after-first-write`: bookkeeping completes, and a refusal is no
/// evidence of rollback).
pub fn flatten_pages(
    session: &DocumentSession,
    pages: &[PageObjectNumber],
    usage: FlattenUsage,
    cancel: &CancelToken,
) -> Result<FlattenResult, EngineError> {
    use crate::access::{DocumentAccess, PageAccess};

    cancel.check()?;
    let owned = session.document()?;
    let indices =
        pages.iter().map(|page| owned.page_index(*page)).collect::<Result<Vec<_>, _>>()?;
    require_unique_pages(pages)?;

    let mut results = Vec::with_capacity(pages.len());
    let mut affected: Vec<PageObjectNumber> = Vec::new();
    for (page, page_index) in pages.iter().copied().zip(indices) {
        let flattened = cancel.check().and_then(|()| {
            if promote_inline_annotations(owned.document(), page_index, page)? {
                affected.push(page);
            }
            Ok(owned.page(page)?.view().flatten(usage.native()))
        });
        let status = match flattened {
            Ok(pdfium::FlattenOutcome::NothingToDo) => FlattenStatus::Unchanged,
            Ok(pdfium::FlattenOutcome::Applied) => FlattenStatus::Applied,
            // Unexercised: nothing in the corpus makes a flatten fail.
            Ok(outcome) => return Err(after_writes(session, &affected, flatten_failure(outcome))),
            Err(refused) => return Err(after_writes(session, &affected, refused)),
        };
        if status == FlattenStatus::Applied && !affected.contains(&page) {
            affected.push(page);
        }
        results.push(FlattenItem { page_object_number: page, status });
    }

    let meta = MutationMeta::local(
        affected.into_iter().map(PageRef::new).collect(),
        &session.write_stamp()?,
    );
    Ok(FlattenResult { page_object_numbers: pages.to_vec(), usage, results, meta })
}

/// `refused`, after the sequence moved for the pages a refused flatten had
/// already changed.
fn after_writes(
    session: &DocumentSession,
    changed: &[PageObjectNumber],
    refused: EngineError,
) -> EngineError {
    if !changed.is_empty() {
        session.note_mutation();
    }
    refused
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_flatten_is_unknown_with_the_flatteners_message() {
        let failed = flatten_failure(pdfium::FlattenOutcome::Failed);
        assert!(failed.is(EngineErrorCode::Unknown), "{failed}");
        assert_eq!(failed.message, "native page flatten failed after preflight");
        assert_eq!(
            flatten_failure(pdfium::FlattenOutcome::Unknown(7)).message,
            "native page flatten returned unexpected code 7",
        );
    }

    #[test]
    fn a_flatten_names_at_least_one_page_and_each_once() {
        let page = |n| PageObjectNumber::new(n).unwrap();
        let empty = require_unique_pages(&[]).unwrap_err();
        assert!(empty.is(EngineErrorCode::InvalidArg), "{empty}");
        assert_eq!(empty.message, "pages.flatten requires at least one page");
        let twice = require_unique_pages(&[page(3), page(4), page(3)]).unwrap_err();
        assert!(twice.is(EngineErrorCode::InvalidArg), "{twice}");
        assert_eq!(twice.message, "pages.flatten was given duplicate page object number 3");
        assert!(require_unique_pages(&[page(3), page(4)]).is_ok());
    }
}
