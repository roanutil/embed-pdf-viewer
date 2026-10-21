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

use crate::access::{DocumentAccess, OwnedDocument, PageAccess};
use crate::cancel::CancelToken;
use crate::clock::Clock;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::pages_write::FlattenUsage;
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::ids::IdSource;
use crate::mutation::PageStateMeta;
use crate::revision::WeakAnnotationState;
use crate::session::document::DocumentSession;

use super::context::AnnotationWriteContext;
use super::create::{AnnotationCreateResult, create_annotation};
use super::delete::{AnnotationDeleteResult, WidgetAttachment, delete_annotation};
use super::draft::{AnnotationActor, AnnotationDraft};
use super::move_::{AnnotationMoveResult, move_annotations};
use super::patch::AnnotationPatch;
use super::resolve::resolve_annotation;
use super::update::{AnnotationUpdateResult, update_annotation};

/// Whether this page holds an annotation nothing can address durably.
///
/// An annotation is weak when it has no indirect object number AND no `/NM`,
/// which leaves its position in `/Annots` as the only way to name it. One is
/// enough to make a rearrangement visible to a client, so the scan stops at
/// the first.
///
/// An entry PDFium refuses is skipped rather than counted. It is a gap in the
/// list, and a gap nobody can reference cannot be a reference that breaks.
fn has_weak_annotations(page: &dyn PageAccess) -> Result<bool, EngineError> {
    let view = page.view();
    for index in 0..view.annotation_count() {
        let Some(annotation) = view.annotation(index) else { continue };
        if annotation.object_number() > 0 {
            continue;
        }
        let nm = annotation.string_value("NM").unwrap_or(None);
        if nm.as_deref().unwrap_or("").is_empty() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Learn the page's weak state if the session does not already know it.
///
/// Only when unknown, because the scan walks every annotation on the page and
/// a mutation that already recorded the answer has not invalidated it. The
/// writers that DO invalidate it re-record afterwards.
fn ensure_weak_state(
    session: &mut DocumentSession,
    page_object_number: PageObjectNumber,
) -> Result<(), EngineError> {
    if matches!(
        session.weak_annotation_state(page_object_number)?,
        WeakAnnotationState::Known { .. }
    ) {
        return Ok(());
    }
    let has_weak = scan_weak(session, page_object_number)?;
    session.record_weak_flag(page_object_number, has_weak)
}

/// Scan the page and drop it again before the session is touched.
///
/// A separate function because the scan borrows the document out of the
/// session and recording the answer borrows the session itself, so the two
/// cannot overlap. Inlining it would leave every caller repeating the same
/// block to end the borrow.
fn scan_weak(
    session: &DocumentSession,
    page_object_number: PageObjectNumber,
) -> Result<bool, EngineError> {
    let owned = session.document()?;
    let page = owned.page(page_object_number)?;
    has_weak_annotations(&page)
}

/// Where a page sits in the document today.
///
/// Resolved per call rather than cached, because a page insert or a reorder
/// moves it and the writers address `/Annots` through a loaded page whose
/// index is the one the document has now.
fn page_index_of(document: &pdfium::Document, page: PageObjectNumber) -> Result<u32, EngineError> {
    (0..document.page_count())
        .find(|index| document.page_object_number_by_index(*index) == page.get())
        .ok_or_else(|| {
            EngineError::with_message(
                crate::error::EngineErrorCode::NotFound,
                format!("no page with object number {}", page.get()),
            )
        })
}

/// Which widgets on this document belong to a form field.
///
/// Consulted only when the thing being deleted is a widget. An attached
/// widget is a VIEW of a field, and removing it here would leave the field's
/// `/Kids` pointing at nothing: the corruption class `forms.repair` exists to
/// clean up. A document with no form model at all has no attached widgets by
/// definition, so everything is deletable.
fn widget_resolver(owned: &OwnedDocument) -> impl Fn(u32) -> WidgetAttachment + '_ {
    let model = owned.document().form_model();
    move |widget: u32| match model.as_ref().and_then(|m| m.field_index_for_widget(widget)) {
        Some(_) => WidgetAttachment::Attached,
        None => WidgetAttachment::Deletable,
    }
}

/// Create one annotation on a page.
///
/// Append-only, so the result is read back against the revision the page
/// already had: a client that listed the page a moment ago can place the new
/// annotation into that listing without refetching it.
pub fn create(
    session: &mut DocumentSession,
    page_object_number: PageObjectNumber,
    draft: &AnnotationDraft,
    actor: &AnnotationActor,
    context: &AnnotationWriteContext<'_>,
    clock: &dyn Clock,
    cancel: &CancelToken,
) -> Result<AnnotationCreateResult, EngineError> {
    cancel.check()?;
    ensure_weak_state(session, page_object_number)?;
    let state = session.page_state(page_object_number)?;

    let owned = session.document()?;
    let page = owned.page(page_object_number)?;
    let result = create_annotation(
        owned.document(),
        page.view(),
        page_object_number,
        // The same state on both sides. Nothing moved, so there is no
        // "after" to report that differs from the "before".
        &state,
        &state,
        draft,
        actor,
        context,
        clock,
        cancel,
    )?;
    session.note_mutation();
    Ok(result)
}

/// Rewrite one annotation in place.
pub fn update(
    session: &mut DocumentSession,
    annotation_ref: &AnnotationRef,
    patch: &AnnotationPatch,
    context: &AnnotationWriteContext<'_>,
    clock: &dyn Clock,
    ids: &dyn IdSource,
    cancel: &CancelToken,
) -> Result<AnnotationUpdateResult, EngineError> {
    cancel.check()?;
    let page_object_number = annotation_ref.page();
    ensure_weak_state(session, page_object_number)?;
    let state = session.page_state(page_object_number)?;

    let owned = session.document()?;
    let index = page_index_of(owned.document(), page_object_number)?;
    let page = owned.page(page_object_number)?;
    let result = update_annotation(
        owned.document(),
        page.view(),
        index,
        page_object_number,
        annotation_ref,
        patch,
        context,
        &state,
        &state,
        clock,
        ids,
        cancel,
    )?;
    session.note_mutation();
    Ok(result)
}

/// Remove one annotation.
///
/// Structural: the epoch advances whether or not the write succeeds, because
/// a partial delete has already moved indices.
pub fn delete(
    session: &mut DocumentSession,
    annotation_ref: &AnnotationRef,
    cancel: &CancelToken,
) -> Result<AnnotationDeleteResult, EngineError> {
    let result = delete_unnoted(session, annotation_ref, cancel)?;
    session.note_mutation();
    Ok(result)
}

/// [`delete`] without noting the mutation, for a caller that notes it once
/// for a whole cascade: `AnnotationMutator.delete` leaves the bump to
/// `finishMutation`, so `forms.deleteField`'s widget deletes add none.
pub(crate) fn delete_unnoted(
    session: &mut DocumentSession,
    annotation_ref: &AnnotationRef,
    cancel: &CancelToken,
) -> Result<AnnotationDeleteResult, EngineError> {
    cancel.check()?;
    let page_object_number = annotation_ref.page();
    ensure_weak_state(session, page_object_number)?;
    let before = session.page_state(page_object_number)?;

    let outcome = {
        let owned = session.document()?;
        let page = owned.page(page_object_number)?;
        let resolve = widget_resolver(owned);
        delete_annotation(
            owned.document(),
            page.view(),
            annotation_ref,
            &before,
            &before,
            &resolve,
            cancel,
        )
    };

    // Bumped on both paths. A refusal that never touched `/Annots` costs one
    // wasted refetch; a partial write that is not reported costs a client
    // resolving index references into a page that moved underneath it.
    session.bump_revision(page_object_number)?;
    let mut result = outcome?;

    let has_weak = scan_weak(session, page_object_number)?;
    session.record_weak_flag(page_object_number, has_weak)?;
    let after = session.page_state(page_object_number)?;
    result.meta.affected_pages = vec![after];
    Ok(result)
}

/// Reorder a contiguous block of a page's annotations.
pub fn move_to(
    session: &mut DocumentSession,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
    to_index: u32,
    ids: &dyn IdSource,
    cancel: &CancelToken,
) -> Result<AnnotationMoveResult, EngineError> {
    cancel.check()?;
    ensure_weak_state(session, page_object_number)?;
    let before = session.page_state(page_object_number)?;

    // Bumped BEFORE the write rather than after, because the moved
    // annotations are read back against the new epoch: a caller comparing the
    // returned DTOs against the returned page state has to see one revision,
    // not a mixture.
    session.bump_revision(page_object_number)?;
    let after_bump = session.page_state(page_object_number)?;

    let outcome = {
        let owned = session.document()?;
        let index = page_index_of(owned.document(), page_object_number)?;
        let page = owned.page(page_object_number)?;
        move_annotations(
            owned.document(),
            page.view(),
            index,
            page_object_number,
            refs,
            to_index,
            &before,
            &after_bump,
            ids,
            cancel,
        )
    };
    let mut result = outcome?;

    let has_weak = scan_weak(session, page_object_number)?;
    session.record_weak_flag(page_object_number, has_weak)?;
    result.meta.affected_pages = vec![session.page_state(page_object_number)?];
    session.note_mutation();
    Ok(result)
}

/// One ref's outcome in [`flatten`]: `AnnotationFlattenItemResult`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AnnotationFlattenItem {
    #[serde(rename = "ref")]
    pub annotation_ref: AnnotationRef,
    pub status: AnnotationFlattenStatus,
}

/// `applied`: painted into the page content and removed. `skipped`: left
/// in place, hidden for the usage, a Popup, or without a usable normal
/// appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AnnotationFlattenStatus {
    Applied,
    Skipped,
}

/// `AnnotationFlattenResult`. `meta` is `None` when nothing was applied.
///
/// The page is the wire's `page: PageRef`, as every other result this crate
/// answers names it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationFlattenResult {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub usage: FlattenUsage,
    pub results: Vec<AnnotationFlattenItem>,
    pub meta: Option<PageStateMeta>,
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

/// `AnnotationFlattener.resolveAll`: every ref on the page, an index ref's
/// revision validated first (`resolveAnnotPtr`). A failure drops, and so
/// closes, whatever was opened.
fn resolve_all<'page>(
    session: &DocumentSession,
    page: &'page pdfium::PageView<'_>,
    refs: &[AnnotationRef],
) -> Result<Vec<pdfium::Annotation<'page>>, EngineError> {
    refs.iter()
        .map(|annotation_ref| {
            if let AnnotationRef::Index { revision, .. } = annotation_ref {
                session.validate_revision(revision)?;
            }
            resolve_annotation(session.document()?.document(), page, annotation_ref)
        })
        .collect()
}

/// `AnnotationFlattener.flatten`: `pages.flatten` for a chosen set of one
/// page's annotations. A content and annotation mutation of that page.
pub fn flatten(
    session: &mut DocumentSession,
    page_object_number: PageObjectNumber,
    refs: &[AnnotationRef],
    usage: FlattenUsage,
    cancel: &CancelToken,
) -> Result<AnnotationFlattenResult, EngineError> {
    cancel.check()?;
    require_refs("annotations.flatten", page_object_number, refs)?;

    let (outcome, statuses) = {
        let owned = session.document()?;
        let page = owned.page(page_object_number)?;
        let annotations = resolve_all(session, page.view(), refs)?;
        page.view().flatten_annotations(&annotations, usage.native())
        // Every handle closes here, before anything reads the page again.
    };

    if outcome == pdfium::FlattenOutcome::Failed {
        // Every ref resolved on THIS page, so a failure is the fork
        // disagreeing with /Annots or a catalog write failing.
        let foreign =
            statuses.iter().position(|s| *s == pdfium::AnnotationFlattenStatus::NotOnPage);
        return Err(match foreign {
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
        });
    }

    let results = refs
        .iter()
        .zip(&statuses)
        .map(|(annotation_ref, status)| AnnotationFlattenItem {
            annotation_ref: annotation_ref.clone(),
            status: if *status == pdfium::AnnotationFlattenStatus::Applied {
                AnnotationFlattenStatus::Applied
            } else {
                AnnotationFlattenStatus::Skipped
            },
        })
        .collect();
    if outcome != pdfium::FlattenOutcome::Applied {
        return Ok(AnnotationFlattenResult { page_object_number, usage, results, meta: None });
    }

    // Content and annotation liveness changed on this page: the same
    // bookkeeping the whole-page flatten performs per affected page.
    session.note_mutation();
    session.bump_revision(page_object_number)?;
    // The TypeScript re-lists the page for its weak-annotation state and
    // ignores a failure, so this does too.
    if let Ok(has_weak) = scan_weak(session, page_object_number) {
        let _ = session.record_weak_flag(page_object_number, has_weak);
    }
    let meta = PageStateMeta {
        affected_pages: vec![session.page_state(page_object_number)?],
        cache_delta: None,
    };
    Ok(AnnotationFlattenResult { page_object_number, usage, results, meta: Some(meta) })
}

/// The bookkeeping `PagesFlattener` and `RedactionApplier` perform once
/// their pages are written: one sequence bump, each changed page's revision
/// bumped and its weak-annotation state re-read (a failed re-read is
/// ignored, as the TypeScript ignores it), and the `PageStateMeta` naming
/// the pages in the order given.
///
/// `pages` is what the write CHANGED (applied or failed), not what it was
/// asked for. A page left unchanged keeps its revision, so a ref a caller
/// holds into it stays good.
pub fn settle_rewritten_pages(
    session: &mut DocumentSession,
    pages: &[PageObjectNumber],
) -> Result<PageStateMeta, EngineError> {
    session.note_mutation();
    for page in pages {
        session.bump_revision(*page)?;
        if let Ok(has_weak) = scan_weak(session, *page) {
            let _ = session.record_weak_flag(*page, has_weak);
        }
    }
    let affected_pages =
        pages.iter().map(|page| session.page_state(*page)).collect::<Result<_, _>>()?;
    Ok(PageStateMeta { affected_pages, cache_delta: None })
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
    require_refs("annotations.exportAppearance", page_object_number, refs)?;
    let owned = session.document()?;
    let page = owned.page(page_object_number)?;
    let annotations = resolve_all(session, page.view(), refs)?;
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
