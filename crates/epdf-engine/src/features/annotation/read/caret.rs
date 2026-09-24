//! The caret.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readCaretAnnotation.ts`.
//!
//! A caret has no geometry beyond its `/Rect` and an optional `/RD` inset of
//! the drawn symbol from it. What makes it interesting is the `/IT`: a caret
//! with `Replace` is one half of an Acrobat text edit, and the strikeout that
//! is its other half points at it through `/IRT` with `/RT /Group`.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::shape::PdfRectDifferences;
use super::style::{Color, read_color, read_opacity};
use super::transform::{read_rotation, read_unrotated_rect};
use crate::error::EngineError;
use crate::features::annotation::intent::CaretIntent;
use crate::geom::PdfRect;

/// What a caret with no `/C` is drawn as.
pub const DEFAULT_CARET_COLOR: Color = Color { r: 255, g: 0, b: 0 };

/// A caret annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaretFields {
    pub intent: Option<CaretIntent>,
    pub color: Color,
    pub opacity: f64,
    pub rect_differences: Option<PdfRectDifferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// Read them.
pub fn read_caret(annotation: &Annotation<'_>) -> Result<CaretFields, EngineError> {
    Ok(CaretFields {
        intent: CaretIntent::from_name(dictionary_read("IT", annotation.intent())?.as_deref()),
        color: read_color(annotation, DEFAULT_CARET_COLOR),
        opacity: read_opacity(annotation),
        rect_differences: super::shape::read_rect_differences(annotation),
        rotation: read_rotation(annotation)?,
        unrotated_rect: read_unrotated_rect(annotation)?,
    })
}
