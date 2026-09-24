//! Square and circle.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readShapeAnnotation.ts`.
//!
//! §12.5.6.8 outlines: a `/Rect`, the filled styling, and two entries nothing
//! else in the family carries. `/BE` is the cloudy border, which is an effect
//! rather than a border style and so has its own field. `/RD` is the inset
//! from `/Rect` to the geometry actually drawn, which exists so a thick or
//! cloudy border has room to render without being clipped.
//!
//! Both read as explicit `null` when absent rather than being omitted. A read
//! DTO is compared structurally against a patch that clears them, and an
//! omitted key and a null one mean different things to that comparison.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::read_cloudy_intensity;
use super::style::{FilledStyleFields, read_filled_style};
use super::transform::{read_rotation, read_unrotated_rect};
use crate::error::EngineError;
use crate::geom::PdfRect;

/// `/RD`, the four margins in PDF points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PdfRectDifferences {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// The fields square and circle share.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeFields {
    #[serde(flatten)]
    pub style: FilledStyleFields,
    pub cloudy_intensity: Option<f64>,
    pub rect_differences: Option<PdfRectDifferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// Read them.
pub fn read_shape(annotation: &Annotation<'_>) -> Result<ShapeFields, EngineError> {
    Ok(ShapeFields {
        style: read_filled_style(annotation)?,
        cloudy_intensity: read_cloudy_intensity(annotation),
        rect_differences: annotation.rectangle_differences().map(|rd| PdfRectDifferences {
            left: f64::from(rd.left),
            top: f64::from(rd.top),
            right: f64::from(rd.right),
            bottom: f64::from(rd.bottom),
        }),
        rotation: read_rotation(annotation)?,
        unrotated_rect: read_unrotated_rect(annotation)?,
    })
}
