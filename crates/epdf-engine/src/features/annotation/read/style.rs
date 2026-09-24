//! Colour, opacity and the effective border.
//!
//! G7. Ports `services/src/features/annotations/internal/read/readStyle.ts`.
//!
//! Three layers, each a superset of the one before, because the DTO families
//! are: ink has stroke styling and no fill, the shape and vertex families add
//! `/IC`, and free text reads only the border half because it gets its colour
//! from `/DA`. They are separate functions rather than one with flags so a
//! reader asks for what its family has.
//!
//! Every default here is the v2 engine's, which means a document that omitted
//! an entry reads the same through this port as it did through the engine that
//! wrote it. Red for geometry, and it is not arbitrary: it is what the
//! annotation was drawn as.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use crate::error::EngineError;
use crate::features::annotation::vocabulary::{AnnotationBorderStyle, AnnotationColorType};

/// An sRGB colour, 0..=255 per channel.
///
/// No alpha. Annotation transparency is `/CA` and travels as `opacity`, so
/// there is one place it can come from and a patch cannot set it twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// What a geometry annotation with no `/C` is drawn as.
pub const DEFAULT_GEOMETRY_COLOR: Color = Color { r: 255, g: 0, b: 0 };

/// The border a bordered kind draws, after `/BS`, the legacy `/Border` array
/// and the ISO defaults have all been resolved by PDFium.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorderFields {
    pub stroke_width: f64,
    pub border_style: AnnotationBorderStyle,
    /// Absent, not empty, when the border is not dashed or names no pattern.
    /// The wire distinguishes the two and a clearing patch operates on the
    /// difference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_array: Option<Vec<f64>>,
}

/// Stroke styling with no fill.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeometryStyleFields {
    pub color: Color,
    pub opacity: f64,
    #[serde(flatten)]
    pub border: BorderFields,
}

/// Stroke styling plus `/IC`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilledStyleFields {
    #[serde(flatten)]
    pub geometry: GeometryStyleFields,
    /// `/IC`. Explicitly `null` when absent rather than omitted, so a read
    /// DTO compares structurally against a patch that clears it.
    pub interior_color: Option<Color>,
}

/// `/C`, or the family's default.
pub fn read_color(annotation: &Annotation<'_>, fallback: Color) -> Color {
    read_color_entry(annotation, AnnotationColorType::Color).unwrap_or(fallback)
}

/// One colour entry, or `None` when the annotation has no such entry.
pub fn read_color_entry(
    annotation: &Annotation<'_>,
    color_type: AnnotationColorType,
) -> Option<Color> {
    annotation.color(color_type as i32).map(|(r, g, b)| Color { r, g, b })
}

/// `/CA`, with an absent entry reading as fully opaque.
///
/// The clamp cannot fire through this accessor and is kept anyway.
/// `EPDFAnnot_GetOpacity` reports an alpha PDFium has already reduced to a
/// byte, so the quotient is in range by construction, and a `/CA 2` in the
/// file never reaches here as a two. The TypeScript clamps for the same
/// reason and the same non-reason; the day this reads a raw `/CA` instead,
/// the clamp is what stops a viewer compositing with an alpha it has no
/// representation for.
pub fn read_opacity(annotation: &Annotation<'_>) -> f64 {
    annotation.opacity().map_or(1.0, |value| value.clamp(0.0, 1.0))
}

/// The effective border.
pub fn read_border_fields(annotation: &Annotation<'_>) -> Result<BorderFields, EngineError> {
    let style = annotation.border_style();
    let dash = dictionary_read("BS", annotation.border_dash_pattern())?;
    Ok(BorderFields {
        stroke_width: f64::from(style.width),
        border_style: AnnotationBorderStyle::from_code(style.style_code),
        dash_array: (!dash.is_empty())
            .then(|| dash.into_iter().map(f64::from).collect::<Vec<f64>>()),
    })
}

/// Stroke styling, for the families with no fill.
pub fn read_geometry_style(
    annotation: &Annotation<'_>,
) -> Result<GeometryStyleFields, EngineError> {
    Ok(GeometryStyleFields {
        color: read_color(annotation, DEFAULT_GEOMETRY_COLOR),
        opacity: read_opacity(annotation),
        border: read_border_fields(annotation)?,
    })
}

/// Stroke styling plus `/IC`, for the filled families.
pub fn read_filled_style(annotation: &Annotation<'_>) -> Result<FilledStyleFields, EngineError> {
    Ok(FilledStyleFields {
        geometry: read_geometry_style(annotation)?,
        interior_color: read_color_entry(annotation, AnnotationColorType::InteriorColor),
    })
}
