//! The redaction marking.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readRedactAnnotation.ts`.
//!
//! A `/Redact` annotation is the NON-DESTRUCTIVE half of a redaction: it
//! declares what should be removed and what the region must look like
//! afterwards. Removing it is a separate document operation and not an
//! annotation mutation, which is why nothing here is irreversible.
//!
//! Two colours mean two different things and neither is a stroke. `/C` is the
//! marking outline a reviewer sees before apply; `/IC` is the fill painted
//! where the content was, after. `null` for `/IC` is the ISO default and
//! means the region is left transparent.
//!
//! A `/RO` pre-baked overlay stream, when a file has one, wins over the label
//! fields at apply time. It is an appearance artifact and is not on the wire,
//! so a consumer reading `overlayText` is reading what WOULD be drawn if no
//! `/RO` existed.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::style::{Color, read_color, read_color_entry, read_opacity};
use super::{dictionary_read, quads_to_wire};
use crate::error::EngineError;
use crate::features::annotation::vocabulary::{
    AnnotationColorType, DEFAULT_STANDARD_FONT, StandardFont, TextAlignment,
};
use crate::geom::PdfQuad;

/// What a marking with no `/C` is outlined in.
pub const DEFAULT_REDACT_COLOR: Color = Color { r: 255, g: 0, b: 0 };
/// What the label is drawn in when there is no `/DA`.
pub const DEFAULT_LABEL_COLOR: Color = Color { r: 0, g: 0, b: 0 };
/// The label size when there is no `/DA`.
pub const DEFAULT_FONT_SIZE: f64 = 12.0;

/// A redaction's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactFields {
    /// `/QuadPoints`, the regions of a TEXT redaction. Empty for an area
    /// redaction, where `/Rect` itself is the region.
    pub quad_points: Vec<PdfQuad>,
    pub color: Color,
    pub opacity: f64,
    pub interior_color: Option<Color>,
    pub overlay_text: Option<String>,
    pub repeat: bool,
    pub font_family: StandardFont,
    /// `/DA` size. Zero is meaningful here and is kept: for a redaction label
    /// it means auto-fit to the region. Free text normalizes the same zero
    /// away, because there it means nothing.
    pub font_size: f64,
    pub font_color: Color,
    pub text_align: TextAlignment,
}

/// Read them.
pub fn read_redact(annotation: &Annotation<'_>) -> Result<RedactFields, EngineError> {
    let appearance = annotation.default_appearance();
    Ok(RedactFields {
        quad_points: quads_to_wire(dictionary_read("QuadPoints", annotation.quad_points())?),
        color: read_color(annotation, DEFAULT_REDACT_COLOR),
        opacity: read_opacity(annotation),
        interior_color: read_color_entry(annotation, AnnotationColorType::InteriorColor),
        overlay_text: dictionary_read("OverlayText", annotation.overlay_text())?,
        repeat: annotation.overlay_text_repeat(),
        font_family: appearance
            .map_or(DEFAULT_STANDARD_FONT, |da| StandardFont::from_code(da.font_code)),
        font_size: appearance.map_or(DEFAULT_FONT_SIZE, |da| f64::from(da.font_size)),
        font_color: appearance.map_or(DEFAULT_LABEL_COLOR, |da| Color {
            r: da.color.0,
            g: da.color.1,
            b: da.color.2,
        }),
        text_align: TextAlignment::from_code(annotation.text_alignment()),
    })
}
