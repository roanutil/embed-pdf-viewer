//! Which address an annotation gets, and how durable it is.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readAnnotationIdentity.ts`.
//!
//! Three sources in a fixed order, and the order is the whole content:
//!
//! 1. The PDF indirect object number, which survives anything short of
//!    rewriting the file.
//! 2. `/NM`, which the file's author chose and which survives a renumbering.
//! 3. The index in `/Annots`, plus a revision token.
//!
//! Only the third is weak, and it is weak in a specific way: the index shifts
//! whenever anything is inserted or removed on the page, so a client holding
//! one has to prove its token is still current before the engine will
//! dereference it. Getting the order backwards would hand out a weak address
//! for an annotation that had a durable one, which no test of a single
//! annotation would catch.

use pdfium::Annotation;

use super::dictionary_read;
use crate::error::EngineError;
use crate::identity::{AnnotationIdentityQuality, AnnotationRef, PageObjectNumber};
use crate::revision::RevisionToken;

/// The address, its quality, and the `/NM` the annotation carries whether or
/// not the address used it.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationIdentity {
    pub annotation_ref: AnnotationRef,
    pub quality: AnnotationIdentityQuality,
    /// `/NM` as read, independent of whether it was chosen as the address.
    /// An annotation with both an object number and an `/NM` reports both,
    /// because a client may be holding either.
    pub nm: Option<String>,
}

/// Decide an annotation's address.
pub fn read_annotation_identity(
    annotation: &Annotation<'_>,
    page: PageObjectNumber,
    index: u32,
    revision: &RevisionToken,
) -> Result<AnnotationIdentity, EngineError> {
    let object_number = annotation.object_number();
    let nm = dictionary_read("NM", annotation.string_value("NM"))?;

    if object_number > 0 {
        return Ok(AnnotationIdentity {
            annotation_ref: AnnotationRef::ObjectNumber { page, annot: object_number },
            quality: AnnotationIdentityQuality::Durable,
            nm,
        });
    }

    // An `/NM` present and empty is not an address. It is a name the file set
    // to nothing, and two annotations could both carry it.
    if let Some(name) = nm.as_ref().filter(|name| !name.is_empty()) {
        return Ok(AnnotationIdentity {
            annotation_ref: AnnotationRef::Nm { page, nm: name.clone() },
            quality: AnnotationIdentityQuality::Durable,
            nm,
        });
    }

    Ok(AnnotationIdentity {
        annotation_ref: AnnotationRef::Index { page, index, revision: revision.clone() },
        quality: AnnotationIdentityQuality::Weak,
        nm,
    })
}
