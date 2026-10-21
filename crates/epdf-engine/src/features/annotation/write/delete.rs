//! Removing an annotation.
//!
//! G11. Ports the `delete` half of
//! `services/src/features/annotations/AnnotationMutator.ts`. Operation:
//! `annotations.delete`.
//!
//! Subtype-agnostic: three fork helpers handle the three reference kinds
//! directly, so nothing here resolves a reference to an index first. That
//! matters because resolving to an index and then removing by index is two
//! operations with a gap between them, and the gap is where a concurrent
//! structural change turns a delete into a delete of something else.
//!
//! # The reported id is the one the annotation had, not the one it was asked
//! for
//!
//! Deleting by index reports whatever durable id the annotation turned out to
//! carry, and `None` when it had none. The engine does not invent one: a
//! caller that targeted by index and gets `None` back knows its caches cannot
//! be updated surgically and has to re-read, which is the truth.
//!
//! # A delete is structural
//!
//! It removes an entry and shifts every later index down, so the page's
//! revision is bumped whatever its weak state is. An old snapshot can still
//! hold an index from before the page was strengthened, and gating the bump
//! on the CURRENT state would leave that snapshot silently wrong.

use pdfium::PageView;
use serde::{Deserialize, Serialize};

use super::impact::{AnnotationListMutationMeta, AnnotationMutationKind, compute_mutation_impact};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::identity::{AnnotationRef, AnnotationStableId};
use crate::revision::PageState;

/// What a delete produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationDeleteResult {
    /// The stable id of what was removed, or `None` for an annotation that
    /// had none.
    pub deleted: Option<AnnotationStableId>,
    pub meta: AnnotationListMutationMeta,
}

/// Whether a widget may be deleted as an ordinary annotation.
///
/// An ATTACHED widget is half of a form field, and removing it through the
/// annotation plane would leave the field pointing at nothing. An INERT one
/// is an ordinary annotation that happens to be a widget, and deletes like
/// any other. The caller supplies the verdict because answering it needs the
/// form model, which this module does not own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetAttachment {
    /// Not a widget, or a widget attached to no field.
    Deletable,
    /// Attached to a field. Detaching or deleting the field is the caller's
    /// route.
    Attached,
}

/// Remove one annotation.
///
/// `resolve_widget` is consulted only for a widget, and only after the
/// annotation has been found: it is the form-model question this module
/// cannot answer, and asking it for every delete would build a form model
/// per delete on documents that have no form at all.
pub fn delete_annotation(
    page: &PageView<'_>,
    annotation_ref: &AnnotationRef,
    state_before: &PageState,
    state_after: &PageState,
    resolve_widget: &dyn Fn(u32) -> WidgetAttachment,
    cancel: &CancelToken,
) -> Result<AnnotationDeleteResult, EngineError> {
    cancel.check()?;

    // Probe first, so a reference that names nothing is reported as such
    // rather than as a bare failure the caller has to interpret. The fork's
    // helpers check existence too; what they cannot do is say which of the
    // two things went wrong.
    let deleted = match annotation_ref {
        AnnotationRef::ObjectNumber { annot, .. } => {
            let found = page.annotation_by_object_number(*annot).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidReference,
                    format!("no annotation with object number {annot} on this page"),
                )
            })?;
            check_widget(&found, resolve_widget)?;
            drop(found);
            cancel.check()?;
            if !page.remove_annotation_by_object_number(*annot) {
                return Err(removal_failed("object number"));
            }
            Some(AnnotationStableId::ObjectNumber { value: *annot })
        }
        AnnotationRef::Nm { nm, .. } => {
            let found = page.annotation_by_name(nm).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidReference,
                    format!("no annotation with /NM '{nm}' on this page"),
                )
            })?;
            check_widget(&found, resolve_widget)?;
            drop(found);
            cancel.check()?;
            if !page.remove_annotation_by_name(nm) {
                return Err(removal_failed("/NM"));
            }
            Some(AnnotationStableId::Nm { value: nm.clone() })
        }
        AnnotationRef::Index { index, .. } => {
            // The caller validated the revision token before calling: an
            // index is only meaningful against the generation it was minted
            // for, and this module has no revision store to check it against.
            delete_at(page, *index, resolve_widget, cancel)?
        }
        AnnotationRef::BaseIndex { base_index, .. } => {
            let index = super::resolve::base_index_position(page, *base_index)?;
            delete_at(page, index, resolve_widget, cancel)?
        }
    };

    let meta = compute_mutation_impact(
        AnnotationMutationKind::Delete,
        state_before,
        state_after,
        deleted.clone().into_iter().collect(),
    )
    .map_err(|e| EngineError::with_message(EngineErrorCode::InvalidArg, e.to_string()))?;

    Ok(AnnotationDeleteResult { deleted, meta })
}

/// Delete the annotation at `index` of the page's `/Annots`, answering
/// whatever durable id it turned out to have.
fn delete_at(
    page: &PageView<'_>,
    index: u32,
    resolve_widget: &dyn Fn(u32) -> WidgetAttachment,
    cancel: &CancelToken,
) -> Result<Option<AnnotationStableId>, EngineError> {
    let found = page.annotation(index).ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidReference,
            format!("index {index} is past the end of this page's annotations"),
        )
    })?;
    check_widget(&found, resolve_widget)?;
    // Whatever durable id it turned out to have. Read BEFORE the
    // removal, because afterwards there is nothing to ask.
    let object_number = found.object_number();
    let nm = found
        .string_value("NM")
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading /NM of the annotation being deleted failed: {e}"),
            )
        })?
        .filter(|nm| !nm.is_empty());
    drop(found);
    cancel.check()?;
    if !page.remove_annotation_at(index) {
        return Err(removal_failed("index"));
    }
    Ok(match (object_number, nm) {
        (n, _) if n > 0 => Some(AnnotationStableId::ObjectNumber { value: n }),
        (_, Some(nm)) => Some(AnnotationStableId::Nm { value: nm }),
        // A weak annotation. The engine refuses to fabricate an id
        // at delete time, so the caller learns it must re-read.
        _ => None,
    })
}

fn check_widget(
    annotation: &pdfium::Annotation<'_>,
    resolve_widget: &dyn Fn(u32) -> WidgetAttachment,
) -> Result<(), EngineError> {
    if AnnotationSubtype::from_code(annotation.subtype_code()) != AnnotationSubtype::Widget {
        return Ok(());
    }
    // A widget with no object number cannot be attached: the form model
    // joins on exactly that, so there is nothing for a field to point at.
    //
    // Removing this guard passes every test, because the form model answers
    // "no field" for object number zero anyway. It is kept because that is a
    // property of the model rather than a promise, and because asking a
    // question about an annotation that has no address is the kind of thing
    // that starts returning a surprising answer.
    let object_number = annotation.object_number();
    if object_number == 0 {
        return Ok(());
    }
    match resolve_widget(object_number) {
        WidgetAttachment::Deletable => Ok(()),
        WidgetAttachment::Attached => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "the widget is attached to a form field; detach it or delete the field",
        )),
    }
}

fn removal_failed(by: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::Unknown,
        format!("PDFium refused to remove the annotation by {by}"),
    )
}

/// The subtype codes this module names, kept honest.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::annotation::subtype::code;

    #[test]
    fn the_widget_guard_is_about_widgets() {
        assert_eq!(AnnotationSubtype::from_code(code::WIDGET), AnnotationSubtype::Widget);
        assert_ne!(AnnotationSubtype::from_code(code::SQUARE), AnnotationSubtype::Widget);
    }
}
