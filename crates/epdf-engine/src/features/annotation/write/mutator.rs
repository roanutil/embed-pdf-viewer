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
use crate::error::EngineError;
use crate::identity::{AnnotationRef, PageObjectNumber};
use crate::ids::IdSource;
use crate::revision::WeakAnnotationState;
use crate::session::document::DocumentSession;

use super::context::AnnotationWriteContext;
use super::create::{AnnotationCreateResult, create_annotation};
use super::delete::{AnnotationDeleteResult, WidgetAttachment, delete_annotation};
use super::draft::{AnnotationActor, AnnotationDraft};
use super::patch::AnnotationPatch;
use super::move_::{AnnotationMoveResult, move_annotations};
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
fn page_index_of(
    document: &pdfium::Document,
    page: PageObjectNumber,
) -> Result<u32, EngineError> {
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
    cancel.check()?;
    let page_object_number = annotation_ref.page();
    ensure_weak_state(session, page_object_number)?;
    let before = session.page_state(page_object_number)?;

    let outcome = {
        let owned = session.document()?;
        let page = owned.page(page_object_number)?;
        let resolve = widget_resolver(owned);
        delete_annotation(page.view(), annotation_ref, &before, &before, &resolve, cancel)
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
    session.note_mutation();
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
