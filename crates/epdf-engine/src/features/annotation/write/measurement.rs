//! A line's, polygon's or polyline's measurement fields, written
//! (`services/src/features/annotations/internal/write/
//! writeMeasurementFields.ts`).
//!
//! A create and an update write them the same way. The caption arrives
//! complete, as `resolveMeasurementDraft` and `measurementFollows` leave it,
//! because the native setters write the whole caption at once. A foreign
//! measure never arrives: it was refused, or a read's marker sent back was
//! dropped.

use pdfium::Annotation;

use super::prepare::{DraftFields, intent_name};
use super::primitives as p;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::subtype::AnnotationSubtype;
use crate::features::measure::{require_measure_write, write_measure};
use crate::geom::{PdfPoint, PdfPointTurn, pdf_point_turned};

/// `drawnPoint`: an upright point turned with the shape's points.
pub fn drawn_point(point: PdfPoint, turn: Option<PdfPointTurn>) -> PdfPoint {
    match turn {
        Some(turn) => pdf_point_turned(point, turn),
        None => point,
    }
}

fn native(point: PdfPoint) -> pdfium::PointF {
    pdfium::PointF { x: point.x as f32, y: point.y as f32 }
}

/// `writeMeasurementFields`. `caption_center` is a polygon's or polyline's
/// caption centre as drawn (turned with its vertices), when the write
/// states one.
pub fn write_measurement_fields(
    annotation: &Annotation<'_>,
    subtype: AnnotationSubtype,
    f: &DraftFields,
    caption_center: Option<PdfPoint>,
) -> Result<(), EngineError> {
    match &f.intent {
        None => {}
        Some(None) => p::remove_key(annotation, "IT")?,
        Some(Some(intent)) => {
            let name = intent_name(intent).ok_or_else(|| {
                EngineError::with_message(EngineErrorCode::Unknown, "an intent with no /IT name")
            })?;
            require_measure_write(annotation.set_intent(name).unwrap_or(false))?;
        }
    }
    match &f.measure {
        None => {}
        Some(None) => require_measure_write(annotation.remove_measure())?,
        Some(Some(measure)) => {
            // A foreign measure is only ever replaced on purpose, and the
            // native setter never resets one in place, so it goes first.
            if annotation
                .measure()
                .is_some_and(|m| m.subtype() != pdfium::measure::MeasureSubtype::Rl)
            {
                require_measure_write(annotation.remove_measure())?;
            }
            write_measure(annotation.add_measure(), measure)?;
        }
    }
    if subtype == AnnotationSubtype::Line {
        if let Some(leader) = f.leader {
            let (length, extension, offset) = leader
                .map(|l| (l.length, l.extension.unwrap_or(0.0), l.offset.unwrap_or(0.0)))
                .unwrap_or_default();
            require_measure_write(annotation.set_line_leader(
                length as f32,
                extension as f32,
                offset as f32,
            ))?;
        }
        match f.caption_enabled {
            None => {}
            Some(None) => p::remove_key(annotation, "Cap")?,
            Some(Some(enabled)) => {
                let offset =
                    f.caption_offset.flatten().map(|o| (o.along as f32, o.perpendicular as f32));
                let top = f.caption_position.as_deref() == Some("top");
                require_measure_write(annotation.set_line_caption(enabled, top, offset))?;
            }
        }
    } else {
        match f.caption_enabled {
            None => {}
            Some(None) => {
                for key in ["MeasurementCaption", "MeasurementCaptionCenter"] {
                    annotation.clear_embed_metadata_key(key).map_err(|e| {
                        EngineError::with_message(
                            EngineErrorCode::RuntimeUnavailable,
                            format!("EPDFAnnot_ClearEmbedMetadataKey failed: {e}"),
                        )
                    })?;
                }
            }
            Some(Some(enabled)) => {
                require_measure_write(
                    annotation.set_shape_caption(enabled, caption_center.map(native)),
                )?;
            }
        }
    }
    Ok(())
}
