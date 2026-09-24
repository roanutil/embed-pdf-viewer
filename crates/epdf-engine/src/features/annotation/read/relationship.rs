//! `/IRT` and `/RT`: the edge from a reply to what it replies to.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readAnnotationRelationship.ts`.
//!
//! The edge is flat. A reply is a first-class annotation in the page list that
//! happens to name a parent, not a child nested inside one, so a comment
//! sidebar composes threads from these edges rather than receiving a tree.
//!
//! Two invariants hold together and neither survives alone:
//!
//! - `reply_type` is `Some` exactly when `in_reply_to` is. `/RT` read without
//!   `/IRT` is meaningless, and PDFium will answer anyway: its
//!   `EPDFAnnot_GetReplyType` reports `/RT`'s own default, so an annotation
//!   that replies to nothing would come back saying `reply`.
//! - A parent with neither an object number nor an `/NM` is reported as no
//!   relationship at all. `/IRT` is by construction an indirect reference, so
//!   this cannot happen in a well-formed file; in a malformed one the choice
//!   is between an unaddressable parent and no parent, and an address a client
//!   cannot dereference is worse than an absent one.

use pdfium::Annotation;

use super::dictionary_read;
use crate::error::EngineError;
use crate::features::annotation::vocabulary::AnnotationReplyType;
use crate::identity::{AnnotationRef, PageObjectNumber};

/// The `/IRT` edge, or nothing.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnnotationRelationship {
    pub in_reply_to: Option<AnnotationRef>,
    pub reply_type: Option<AnnotationReplyType>,
}

/// Read the edge.
///
/// `page` is this annotation's page, and it is also the parent's: §12.5.6.2
/// requires an `/IRT` target to live on the same page, so the parent's address
/// is built from the same page number rather than from a second lookup.
pub fn read_annotation_relationship(
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
) -> Result<AnnotationRelationship, EngineError> {
    let Some(parent) = dictionary_read("IRT", annotation.linked_annotation("IRT"))? else {
        return Ok(AnnotationRelationship::default());
    };

    let Some(in_reply_to) = parent_ref(&parent, page)? else {
        return Ok(AnnotationRelationship::default());
    };

    Ok(AnnotationRelationship {
        in_reply_to: Some(in_reply_to),
        reply_type: Some(AnnotationReplyType::from_code(annotation.reply_type_code())),
    })
}

/// The parent's address, by the same precedence [`super::identity`] uses
/// minus its last step. There is no index fallback: an index would address
/// the parent by its position in a list this read is not walking.
fn parent_ref(
    parent: &Annotation<'_>,
    page: PageObjectNumber,
) -> Result<Option<AnnotationRef>, EngineError> {
    let object_number = parent.object_number();
    if object_number > 0 {
        return Ok(Some(AnnotationRef::ObjectNumber { page, annot: object_number }));
    }
    let nm = dictionary_read("NM", parent.string_value("NM"))?;
    Ok(nm.filter(|name| !name.is_empty()).map(|nm| AnnotationRef::Nm { page, nm }))
}
