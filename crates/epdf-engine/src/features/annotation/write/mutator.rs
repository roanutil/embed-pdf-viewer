//! The bookkeeping around an annotation write.
//!
//! G11. Ports the part of
//! `services/src/features/annotations/AnnotationMutator.ts` that is not the
//! write itself: acquiring the page, learning whether the page holds weak
//! references, capturing the page state on each side of the change, and
//! deciding whether the local index epoch has to advance.
//!
//! The four writers in this module take that state as arguments rather than
//! reaching for it, which is what makes them testable against a fixture with
//! no session at all. The cost is that every caller would otherwise have to
//! get the sequence right, and getting it wrong is silent: a delete that
//! forgets to bump leaves clients resolving index references into a page
//! whose indices moved.
//!
//! # Which mutations bump
//!
//! Create appends, so no existing index moves. Update rewrites one
//! annotation in place. Neither advances the epoch, and both read their
//! result back against the revision the page already had.
//!
//! Delete and move rearrange `/Annots`, so both bump, ONCE, and always,
//! whatever the page's current weak state says. A snapshot taken before the
//! page's annotations were strengthened can still be holding index
//! references, and those are exactly the ones a rearrangement invalidates.
//!
//! # The bump on the way out
//!
//! A structural write that fails partway has already moved indices. The
//! guard here bumps on the failure path for exactly that reason: leaving the
//! epoch where it was would tell every client that nothing had changed,
//! which is the one thing that is certainly untrue.

use crate::access::{DocumentAccess, PageAccess};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::pages_write::FlattenUsage;
use crate::identity::{AnnotationRef, PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::session::document::DocumentSession;

use super::delete::promote_inline_annotations;
use super::resolve::resolve_annotation_index_raw;

/// One ref's outcome in [`flatten`]: `AnnotationFlattenItemResult`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnnotationFlattenItem {
    #[serde(rename = "ref")]
    pub annotation_ref: AnnotationRef,
    pub status: AnnotationFlattenStatus,
}

/// `applied`: painted into the page content and removed. `unchanged`: left
/// in place, hidden for the usage, a Popup, or without a usable normal
/// appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationFlattenStatus {
    Applied,
    Unchanged,
}

/// `AnnotationFlattenResult`. `meta` names the page when the flatten
/// changed it (an apply, or the promotion before it) and nothing otherwise.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationFlattenResult {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub usage: FlattenUsage,
    pub results: Vec<AnnotationFlattenItem>,
    pub meta: MutationMeta,
}

/// `AnnotationFlattener.requireRefs`.
fn require_refs(
    op: &str,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
) -> Result<(), EngineError> {
    if refs.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{op} requires at least one ref"),
        ));
    }
    for annotation_ref in refs {
        if annotation_ref.page() != page_object_number {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "{op} refs must all target page {}; got ref on page {}",
                    page_object_number.get(),
                    annotation_ref.page().get()
                ),
            ));
        }
    }
    Ok(())
}

/// Where each ref is in its page's `/Annots` (`resolveAnnotIndexRaw`, as
/// `AnnotationFlattener.resolveAll` resolves them): `NotFound` for a ref
/// nothing on the page answers to.
///
/// Upstream resolves after promoting the page and finds a promoted inline
/// annotation through the layer's births. The flatten resolves first,
/// which finds the same positions: the promotion keeps every position.
fn positions_of(
    document: &pdfium::Document,
    page_index: u32,
    refs: &[AnnotationRef],
) -> Result<Vec<u32>, EngineError> {
    refs.iter()
        .map(|annotation_ref| resolve_annotation_index_raw(document, page_index, annotation_ref))
        .collect()
}

/// The loaded page's annotation at each position (`resolveAnnotPtr`). A
/// failure drops, and so closes, whatever was opened.
fn open_all<'page>(
    page: &'page pdfium::PageView<'_>,
    page_object_number: PageObjectNumber,
    positions: &[u32],
) -> Result<Vec<pdfium::Annotation<'page>>, EngineError> {
    positions
        .iter()
        .map(|index| {
            page.annotation(*index).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    format!(
                        "annotation {index} of page {} could not be opened",
                        page_object_number.get()
                    ),
                )
            })
        })
        .collect()
}

/// `AnnotationFlattener.flatten`: `pages.flatten` for a chosen set of one
/// page's annotations. A content and annotation mutation of that page, run
/// in the session's open write transaction, whose stamp its `meta` carries.
///
/// A refusal after the promotion leaves the page promoted, which a layer
/// transaction would take back; this engine opens none around its writes
/// until Task 8.30, so the sequence moves for it, as `after-first-write`
/// asks.
pub fn flatten(
    session: &DocumentSession,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
    usage: FlattenUsage,
    cancel: &CancelToken,
) -> Result<AnnotationFlattenResult, EngineError> {
    cancel.check()?;
    // The handler resolves the page before the flattener checks the refs.
    let owned = session.document()?;
    let page_index = owned.page_index(page_object_number)?;
    require_refs("annotations.flatten", page_object_number, refs)?;
    let document = owned.document();
    let positions = positions_of(document, page_index, refs)?;
    // A flatten removes entries from /Annots; promotion keeps every position.
    let promoted = promote_inline_annotations(document, page_index, page_object_number)?;

    let flattened = owned.page(page_object_number).and_then(|page| {
        let annotations = open_all(page.view(), page_object_number, &positions)?;
        Ok(page.view().flatten_annotations(&annotations, usage.native()))
        // Every handle closes here, before anything reads the page again.
    });
    let (outcome, statuses) = match flattened {
        Ok(flattened) => flattened,
        Err(refused) => return Err(after_promotion(session, promoted, refused)),
    };

    if outcome == pdfium::FlattenOutcome::Failed {
        // Every ref resolved on THIS page, so a failure is the fork
        // disagreeing with /Annots or a catalog write failing.
        let foreign =
            statuses.iter().position(|s| *s == pdfium::AnnotationFlattenStatus::NotOnPage);
        let refused = match foreign {
            Some(index) => EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "annotations.flatten: ref {index} is not an annotation of page {}",
                    page_object_number.get()
                ),
            ),
            None => EngineError::with_message(
                EngineErrorCode::Unknown,
                "native annotation flatten failed after preflight",
            ),
        };
        return Err(after_promotion(session, promoted, refused));
    }

    let results = refs
        .iter()
        .zip(&statuses)
        .map(|(annotation_ref, status)| AnnotationFlattenItem {
            annotation_ref: annotation_ref.clone(),
            status: if *status == pdfium::AnnotationFlattenStatus::Applied {
                AnnotationFlattenStatus::Applied
            } else {
                AnnotationFlattenStatus::Unchanged
            },
        })
        .collect();
    // A promotion with nothing to flatten still changed the page.
    let changed = outcome == pdfium::FlattenOutcome::Applied || promoted;
    let affected_pages = if changed { vec![PageRef::new(page_object_number)] } else { Vec::new() };
    let meta = MutationMeta::local(affected_pages, &session.write_stamp()?);
    Ok(AnnotationFlattenResult { page_object_number, usage, results, meta })
}

/// `refused`, after the sequence moved for a promotion that stays.
fn after_promotion(session: &DocumentSession, promoted: bool, refused: EngineError) -> EngineError {
    if promoted {
        session.note_mutation();
    }
    refused
}

/// `AnnotationFlattener.exportAppearance`: a chosen set of one page's
/// annotation appearances as a NEW single-page PDF. A read: nothing about
/// the document changes.
pub fn export_appearance(
    session: &DocumentSession,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
    cancel: &CancelToken,
) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    let owned = session.document()?;
    let page_index = owned.page_index(page_object_number)?;
    require_refs("annotations.exportAppearance", page_object_number, refs)?;
    let positions = positions_of(owned.document(), page_index, refs)?;
    let page = owned.page(page_object_number)?;
    let annotations = open_all(page.view(), page_object_number, &positions)?;
    match page.view().export_annotations_as_document(&annotations) {
        // All-or-nothing by contract: a ref without a usable normal
        // appearance (hidden, popup, no /AP) refuses the whole export.
        Ok(None) => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "annotations.exportAppearance: every ref must be a visible annotation of this page with a normal appearance",
        )),
        Ok(Some(bytes)) => Ok(bytes),
        Err(_) => Err(EngineError::with_message(
            EngineErrorCode::DocOpenFailed,
            "failed to save exported appearances",
        )),
    }
}
