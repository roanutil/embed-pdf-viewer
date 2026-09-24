//! Polygon and polyline.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readVertexAnnotation.ts`.
//!
//! §12.5.6.9 point lists. The rotation here is ADVISORY and carries no
//! `unrotatedRect`: the vertices have already been rotated, because they are
//! the portable visual, so the scalar only records the angle that was applied
//! and lets a UI draw an oriented selection box and offer a reset. PDFium
//! ignores a lone `Rotation`, so it changes no appearance.
//!
//! `/RD` is absent from this family on purpose. A polygon's geometry is fully
//! described by its vertices, so there is no gap between the box and the
//! drawing for a margin to describe.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::style::{FilledStyleFields, read_filled_style};
use super::transform::read_rotation;
use super::{dictionary_read, points_to_wire};
use crate::error::EngineError;
use crate::geom::PdfPoint;

/// The fields polygon and polyline share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VertexFields {
    #[serde(flatten)]
    pub style: FilledStyleFields,
    /// `/Vertices`, in order, in PDF user space.
    pub vertices: Vec<PdfPoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
}

/// Read them.
pub fn read_vertex(annotation: &Annotation<'_>) -> Result<VertexFields, EngineError> {
    Ok(VertexFields {
        style: read_filled_style(annotation)?,
        vertices: points_to_wire(dictionary_read("Vertices", annotation.vertices())?),
        rotation: read_rotation(annotation)?,
    })
}
