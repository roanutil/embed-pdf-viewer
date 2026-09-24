//! The rotation keys EmbedPDF writes into `/EMBD_Metadata`.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readAnnotationTransformMetadata.ts`.
//!
//! PDF has no rotation entry for most annotation subtypes. A rotated square is
//! a square whose appearance stream is rotated and whose `/Rect` is the
//! enclosing box, so the logical box and the angle are both lost once written.
//! These two keys keep them, which is what lets a rotated annotation be
//! rotated again without accumulating error.
//!
//! `Rotation` is reported for every rotatable kind, box and vertex alike.
//! `UnrotatedRect` is for box kinds only: a vertex annotation's points ARE its
//! geometry, so there is no separate logical box to remember.

use pdfium::Annotation;

use super::dictionary_read;
use crate::error::EngineError;
use crate::geom::PdfRect;

const KEY_ROTATION: &str = "Rotation";
const KEY_UNROTATED_RECT: &str = "UnrotatedRect";

/// The rotation in degrees, PDF convention.
///
/// Zero reads as absent, deliberately. An annotation rotated by nothing is an
/// unrotated annotation, and reporting `Some(0.0)` would make a round trip
/// through a writer stamp a key onto a file that had none.
pub fn read_rotation(annotation: &Annotation<'_>) -> Result<Option<f64>, EngineError> {
    if !annotation.has_embed_metadata() {
        return Ok(None);
    }
    let value = dictionary_read(KEY_ROTATION, annotation.embed_metadata_number(KEY_ROTATION))?;
    Ok(value.filter(|degrees| *degrees != 0.0).map(f64::from))
}

/// The logical box of a rotated box annotation.
pub fn read_unrotated_rect(annotation: &Annotation<'_>) -> Result<Option<PdfRect>, EngineError> {
    if !annotation.has_embed_metadata() {
        return Ok(None);
    }
    let rect =
        dictionary_read(KEY_UNROTATED_RECT, annotation.embed_metadata_rect(KEY_UNROTATED_RECT))?;
    Ok(rect.map(|r| PdfRect {
        left: f64::from(r.left),
        bottom: f64::from(r.bottom),
        right: f64::from(r.right),
        top: f64::from(r.top),
    }))
}
