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

use pdfium::{Annotation, Document, PageView};

use crate::error::{EngineError, EngineErrorCode};
use crate::identity::{AnnotationRef, AnnotationStableId};
use crate::ids::IdSource;

/// Resolve a reference on an already-open page of `document`.
pub fn resolve_annotation<'page>(
    document: &Document,
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
        AnnotationRef::BaseIndex { base_index, .. } => {
            let index = base_index_position(document, page, *base_index)?;
            page.annotation(index).ok_or_else(|| base_index_not_found(*base_index))
        }
    }
}

/// Where the annotation born inline at `base_index` of the page is now
/// (`resolveAnnotIndexRaw.ts` `baseIndexPosition`). On a page the layer
/// promoted, the birth list gives the object it became
/// (`EPDFLayer_IsPagePromoted`, `EPDFLayer_GetBirthObjectNumber`); on any
/// other page only an inline entry (object number `0`) still sits at its
/// birth position, so an object there, or nothing, is `NotFound`.
pub fn base_index_position(
    document: &Document,
    page: &PageView<'_>,
    base_index: u32,
) -> Result<u32, EngineError> {
    let page_object_number = page.object_number();
    if document.layer_is_page_promoted(page_object_number) {
        let found = document
            .layer_birth_object_number(page_object_number, base_index)
            .and_then(|object_number| page.annotation_by_object_number(object_number))
            .ok_or_else(|| base_index_not_found(base_index))?;
        return u32::try_from(page.annotation_index(&found))
            .map_err(|_| base_index_not_found(base_index));
    }
    match page.annotation(base_index) {
        Some(found) if found.object_number() == 0 => Ok(base_index),
        _ => Err(base_index_not_found(base_index)),
    }
}

/// Where the annotation `annotation_ref` names is in its page's `/Annots`,
/// found from the page's dictionaries without loading the page
/// (`resolveAnnotIndexRaw`). `NotFound` when it isn't there. A `baseIndex`
/// on a page the layer promoted is found through its birth; on any other
/// page it is still inline at that position.
pub fn resolve_annotation_index_raw(
    document: &Document,
    page_index: u32,
    annotation_ref: &AnnotationRef,
) -> Result<u32, EngineError> {
    let page = annotation_ref.page().get();
    let count = document.raw_annotation_count(page_index).unwrap_or(0);
    let index_of = |object_number: u32| {
        (0..count).find(|index| {
            document
                .raw_annotation(page_index, *index)
                .is_some_and(|annotation| annotation.object_number() == object_number)
        })
    };
    match annotation_ref {
        AnnotationRef::ObjectNumber { annot, .. } => index_of(*annot).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no annotation with object number {annot} on page {page}"),
            )
        }),
        AnnotationRef::BaseIndex { base_index, .. } => {
            let found = if document.layer_is_page_promoted(page) {
                document.layer_birth_object_number(page, *base_index).and_then(index_of)
            } else {
                // Only an inline entry still sits at its birth position.
                document
                    .raw_annotation(page_index, *base_index)
                    .filter(|annotation| annotation.object_number() == 0)
                    .map(|_| *base_index)
            };
            found.ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::NotFound,
                    format!("no annotation born at position {base_index} of page {page}"),
                )
            })
        }
        AnnotationRef::Nm { .. } | AnnotationRef::Index { .. } => Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "an annotation is named by object number or base index",
        )),
    }
}

fn base_index_not_found(base_index: u32) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::NotFound,
        format!("no annotation born at position {base_index} of this page"),
    )
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
