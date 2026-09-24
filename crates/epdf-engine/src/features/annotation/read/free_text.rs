//! Free text, and the callout that is the same kind.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readFreeTextAnnotation.ts`.
//!
//! # The colour model is the one thing to get right here
//!
//! Free text uses `/C` for something no other family does. Everywhere else
//! `/C` is the stroke or the mark; here it is the BOX BACKGROUND, and the
//! border and text colour come from `/DA` instead. So the wire's `color` is
//! the `/DA` colour and `interiorColor` is `/C`, which reads backwards until
//! you know why, and a reader that mapped them the ordinary way would produce
//! an annotation whose border and background had swapped.
//!
//! `fontColor` is a third channel and is surfaced only when it DIFFERS from
//! the `/DA` colour. `TextColor` overrides the text alone, so a value equal to
//! `color` overrides nothing, and putting it on the wire anyway would make a
//! round trip write an override where the file had none.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::ink_line::read_line_endings;
use super::shape::{PdfRectDifferences, read_rect_differences};
use super::style::{BorderFields, Color, read_border_fields, read_color_entry, read_opacity};
use super::transform::{read_rotation, read_unrotated_rect};
use super::{dictionary_read, points_to_wire};
use crate::error::EngineError;
use crate::features::annotation::intent::FreeTextIntent;
use crate::features::annotation::vocabulary::{
    AnnotationColorType, DEFAULT_STANDARD_FONT, LineEnding, StandardFont, TextAlignment,
};
use crate::geom::{CalloutLine, PdfRect};

/// What the text and border are drawn in when there is no `/DA`.
pub const DEFAULT_FREE_TEXT_COLOR: Color = Color { r: 0, g: 0, b: 0 };
/// The size when `/DA` has none, or has an unusable zero.
pub const DEFAULT_FONT_SIZE: f64 = 12.0;

/// A free-text annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTextFields {
    pub intent: FreeTextIntent,
    pub font_family: StandardFont,
    pub font_size: f64,
    pub text_align: TextAlignment,
    /// The `/DA` colour: the border, and the text unless `font_color`
    /// overrides it.
    pub color: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<Color>,
    /// `/C`, the box background. `null` when the box is transparent.
    pub interior_color: Option<Color>,
    pub opacity: f64,
    #[serde(flatten)]
    pub border: BorderFields,
    pub rect_differences: Option<PdfRectDifferences>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout_line: Option<CalloutLine>,
    /// `/LE`, the ending at the called-out end of the leader. Present only
    /// with a leader line, and only when it draws something.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_ending: Option<LineEnding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// Read them.
pub fn read_free_text(annotation: &Annotation<'_>) -> Result<FreeTextFields, EngineError> {
    let appearance = annotation.default_appearance();
    let color = appearance.map_or(DEFAULT_FREE_TEXT_COLOR, |da| Color {
        r: da.color.0,
        g: da.color.1,
        b: da.color.2,
    });

    let text_color = read_color_entry(annotation, AnnotationColorType::TextColor);
    let callout_line = read_callout_line(annotation)?;
    // The ending is the leader's, so it is meaningless without one, and
    // `None` draws nothing so reporting it would put a field on the wire that
    // says what the absence already said.
    let leader_end = read_line_endings(annotation).end;

    Ok(FreeTextFields {
        intent: FreeTextIntent::from_name(dictionary_read("IT", annotation.intent())?.as_deref()),
        font_family: appearance
            .map_or(DEFAULT_STANDARD_FONT, |da| StandardFont::from_code(da.font_code)),
        // A `/DA` size of zero means auto-fit, which free text has no support
        // for, so it normalizes to the default. A redaction keeps its zero,
        // because there it is a real instruction.
        font_size: appearance
            .map(|da| f64::from(da.font_size))
            .filter(|size| *size > 0.0)
            .unwrap_or(DEFAULT_FONT_SIZE),
        text_align: TextAlignment::from_code(annotation.text_alignment()),
        color,
        font_color: text_color.filter(|text| *text != color),
        interior_color: read_color_entry(annotation, AnnotationColorType::Color),
        opacity: read_opacity(annotation),
        border: read_border_fields(annotation)?,
        rect_differences: read_rect_differences(annotation),
        line_ending: callout_line
            .is_some()
            .then_some(leader_end)
            .filter(|ending| *ending != LineEnding::None),
        callout_line,
        rotation: read_rotation(annotation)?,
        unrotated_rect: read_unrotated_rect(annotation)?,
    })
}

/// `/CL`, which §12.5.6.6 permits with two points or three.
///
/// Any other count is reported as no leader at all. A one-point or four-point
/// `/CL` is not a leader a viewer can draw, and passing it through would put a
/// shape on the wire that the type says cannot exist.
fn read_callout_line(annotation: &Annotation<'_>) -> Result<Option<CalloutLine>, EngineError> {
    let points = points_to_wire(dictionary_read("CL", annotation.callout_line())?);
    Ok(match points.len() {
        2 => Some(CalloutLine::Straight([points[0], points[1]])),
        3 => Some(CalloutLine::Kneed([points[0], points[1], points[2]])),
        _ => None,
    })
}
