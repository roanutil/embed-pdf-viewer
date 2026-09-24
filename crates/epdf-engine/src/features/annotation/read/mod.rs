//! The readers that walk PDFium.
//!
//! G7. The half of the port that touches a document, as opposed to the pure
//! tables in [`super::vocabulary`] and its neighbours.
//!
//! Ported so far: the base shell every annotation carries, and the
//! `unsupported` fallback. Every subtype therefore reads as a correct shell
//! with no subtype-specific fields, which is a state the wire format already
//! has a name for. [`registry::FALLS_BACK_TO_UNSUPPORTED`] lists what is left,
//! and a test asserts that list rather than leaving it to a comment.

pub mod base;
pub mod dto;
pub mod embed_metadata;
pub mod identity;
pub mod ink_line;
pub mod page;
pub mod registry;
pub mod relationship;
pub mod shape;
pub mod style;
pub mod text_markup;
pub mod transform;
pub mod unsupported;
pub mod vertex;

use pdfium::{Annotation, PdfiumError, PointF, QuadF};

use crate::error::{EngineError, EngineErrorCode};

/// Map a dictionary read that PDFium refused.
///
/// There is no blanket `From<PdfiumError>` in this engine on purpose: each
/// caller decides what a failure means, and most of them mean something more
/// specific than "the runtime said no". Here it does mean that. A key that is
/// absent is not a failure, it is `Ok(None)`, so anything reaching this is the
/// runtime declining to answer a question about a dictionary it has open,
/// which is not a fact about the document.
pub(crate) fn dictionary_read<T>(
    key: &str,
    result: Result<T, PdfiumError>,
) -> Result<T, EngineError> {
    result.map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("reading /{key} of an annotation failed: {e}"),
        )
    })
}

/// PDFium's `FS_POINTF` list into the wire's.
///
/// The widening from `f32` to `f64` is where it happens for every point in
/// this feature, so nothing downstream has to remember that PDFium's geometry
/// is single precision while the wire's is double.
pub(crate) fn points_to_wire(points: Vec<PointF>) -> Vec<crate::geom::PdfPoint> {
    points
        .into_iter()
        .map(|p| crate::geom::PdfPoint { x: f64::from(p.x), y: f64::from(p.y) })
        .collect()
}

/// The same for `FS_QUADPOINTSF`.
///
/// The four points stay in PDFium's slot order and are NOT relabelled as
/// corners. Producers disagree about which corner is which, and a quad can be
/// rotated or skewed, so naming them on the wire would be a guarantee this
/// engine cannot keep.
pub(crate) fn quads_to_wire(quads: Vec<QuadF>) -> Vec<crate::geom::PdfQuad> {
    quads
        .into_iter()
        .map(|q| crate::geom::PdfQuad {
            p1: crate::geom::PdfPoint { x: f64::from(q.x1), y: f64::from(q.y1) },
            p2: crate::geom::PdfPoint { x: f64::from(q.x2), y: f64::from(q.y2) },
            p3: crate::geom::PdfPoint { x: f64::from(q.x3), y: f64::from(q.y3) },
            p4: crate::geom::PdfPoint { x: f64::from(q.x4), y: f64::from(q.y4) },
        })
        .collect()
}

/// `/BE`, the cloudy border intensity, normalized to positive-or-absent.
///
/// A `/BE` present with intensity zero draws no clouds, and the wire domain
/// is a positive number or null, so a degenerate foreign value normalizes
/// away rather than leaking a zero no schema admits. Shared by the shape
/// family and polygon, which are the two places it can appear.
pub(crate) fn read_cloudy_intensity(annotation: &Annotation<'_>) -> Option<f64> {
    annotation.border_effect().map(f64::from).filter(|intensity| *intensity > 0.0)
}
