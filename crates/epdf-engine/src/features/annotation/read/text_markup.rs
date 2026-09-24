//! The four text-markup subtypes.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readTextMarkupAnnotation.ts`.
//!
//! Highlight, underline, squiggly and strikeout share a wire shape
//! (§12.5.6.10): a colour, an opacity and the quads the markup covers. They
//! differ in two ways only, and both are here rather than in four near-copies:
//! highlight defaults to yellow where the other three default to black, and
//! strikeout carries an `/IT` because it can be the subordinate half of a
//! replace-text edit.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::style::{Color, read_color, read_opacity};
use super::{dictionary_read, quads_to_wire};
use crate::error::EngineError;
use crate::geom::PdfQuad;

/// What a highlight with no `/C` is drawn as.
pub const DEFAULT_HIGHLIGHT_COLOR: Color = Color { r: 255, g: 255, b: 0 };
/// What the other three are.
pub const DEFAULT_TEXT_MARKUP_COLOR: Color = Color { r: 0, g: 0, b: 0 };

/// The fields the family shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextMarkupFields {
    pub color: Color,
    pub opacity: f64,
    /// `/QuadPoints`. One quad per run of marked text, and the quads are
    /// positional: this engine asserts no corner semantics about them.
    pub quad_points: Vec<PdfQuad>,
}

/// Read the shared fields.
pub fn read_text_markup(
    annotation: &Annotation<'_>,
    fallback: Color,
) -> Result<TextMarkupFields, EngineError> {
    Ok(TextMarkupFields {
        color: read_color(annotation, fallback),
        opacity: read_opacity(annotation),
        quad_points: quads_to_wire(dictionary_read("QuadPoints", annotation.quad_points())?),
    })
}
