//! Turning a reference into a live annotation.
//!
//! G11. Ports
//! `services/src/features/annotations/internal/identity/resolveAnnotationPointer.ts`
//! and `captureOrStampStableId.ts`.
//!
//! Three kinds, in the order the wire spec gives them, and each resolved by
//! its own native helper rather than by translating into an index first: an
//! index is a position, and a position is only meaningful against the
//! generation it was minted for.
//!
//! # A revision token is the caller's problem to prove
//!
//! Resolving by index does NOT validate the token here. The revision store
//! belongs to the session, this module has no access to one, and pretending
//! otherwise would mean either threading a store through every write or
//! validating against nothing. The caller validates and then resolves.

use pdfium::{Annotation, PageView};

use crate::error::{EngineError, EngineErrorCode};
use crate::identity::{AnnotationRef, AnnotationStableId};
use crate::ids::IdSource;

/// Resolve a reference on an already-open page.
pub fn resolve_annotation<'page>(
    page: &'page PageView<'_>,
    annotation_ref: &AnnotationRef,
) -> Result<Annotation<'page>, EngineError> {
    match annotation_ref {
        AnnotationRef::ObjectNumber { annot, .. } => {
            page.annotation_by_object_number(*annot).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::InvalidReference,
                    format!("no annotation with object number {annot} on this page"),
                )
            })
        }
        AnnotationRef::Nm { nm, .. } => page.annotation_by_name(nm).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidReference,
                format!("no annotation with /NM '{nm}' on this page"),
            )
        }),
        AnnotationRef::Index { index, .. } => page.annotation(*index).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidReference,
                format!("index {index} is past the end of this page's annotations"),
            )
        }),
    }
}

/// The annotation's stable id, stamping one if it has none.
///
/// # `/NM` is monotonic
///
/// An annotation that already has a durable identity is NEVER touched: not
/// its object number, which is the file's, and not its `/NM`, which someone
/// chose. Only a WEAK annotation is stamped, and after that it has one
/// forever. That is what lets a client cache a key: the engine may
/// strengthen an identity and will never change one.
///
/// The stamp is a write, so this is here rather than beside the readers.
pub fn capture_or_stamp_stable_id(
    annotation: &Annotation<'_>,
    ids: &dyn IdSource,
) -> Result<AnnotationStableId, EngineError> {
    let object_number = annotation.object_number();
    if object_number > 0 {
        return Ok(AnnotationStableId::ObjectNumber { value: object_number });
    }

    let existing = annotation
        .string_value("NM")
        .map_err(|e| {
            EngineError::with_message(
                EngineErrorCode::RuntimeUnavailable,
                format!("reading /NM failed: {e}"),
            )
        })?
        .filter(|nm| !nm.is_empty());
    if let Some(nm) = existing {
        return Ok(AnnotationStableId::Nm { value: nm });
    }

    let minted = ids.next_uuid_v4();
    match annotation.set_string_value("NM", &minted) {
        Ok(true) => Ok(AnnotationStableId::Nm { value: minted }),
        Ok(false) => Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "PDFium refused to stamp /NM on a weak annotation",
        )),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("stamping /NM failed: {e}"),
        )),
    }
}
