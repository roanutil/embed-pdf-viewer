//! The setters every annotation writer composes.
//!
//! G11. Ports `services/src/features/annotations/internal/write/
//! annotationWritePrimitives.ts`, `writeAnnotationBox.ts`,
//! `writeAnnotationPoints.ts`, `writeAnnotationTransformMetadata.ts`
//! (`writeRecordedTurn`) and `writeEmbedMetadata.ts`. Each setter that PDFium
//! refuses is an `Unknown` error naming the call, as upstream's are; the
//! ones whose refusal is benign (a clear of an entry that isn't there) say
//! nothing.

use pdfium::Annotation;

use super::prepare::Rgb;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::vocabulary::{AnnotationColorType, LineEndings};
use crate::geom::{NamedPdfQuad, PdfPoint, PdfPointTurn, PdfRect};

fn refused(call: &str) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, format!("{call} returned false"))
}

fn checked(ok: bool, call: &str) -> Result<(), EngineError> {
    if ok { Ok(()) } else { Err(refused(call)) }
}

/// A dictionary write that can fail on its key or value as well as refuse.
pub fn written(result: Result<bool, pdfium::PdfiumError>, call: &str) -> Result<(), EngineError> {
    match result {
        Ok(true) => Ok(()),
        Ok(false) => Err(refused(call)),
        Err(e) => Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("{call} failed: {e}"),
        )),
    }
}

/// `setAnnotColor`.
pub fn set_color(
    annotation: &Annotation<'_>,
    which: AnnotationColorType,
    Rgb(r, g, b): Rgb,
) -> Result<(), EngineError> {
    checked(annotation.set_color(which as i32, r, g, b), "EPDFAnnot_SetColor")
}

/// `clearAnnotColor`: a false return is benign, there was nothing to clear.
pub fn clear_color(annotation: &Annotation<'_>, which: AnnotationColorType) {
    annotation.clear_color(which as i32);
}

/// `setAnnotOpacity`: `/CA`, stored as given.
pub fn set_opacity(annotation: &Annotation<'_>, opacity: f64) -> Result<(), EngineError> {
    checked(annotation.set_opacity(opacity), "EPDFAnnot_SetOpacity")
}

/// `setAnnotFlags`: only the flags named change, the rest keep their bit.
pub fn set_flags(
    annotation: &Annotation<'_>,
    flags: &[Option<bool>; 10],
) -> Result<(), EngineError> {
    let mut bits = annotation.flags();
    for (bit, value) in flags.iter().enumerate() {
        match value {
            Some(true) => bits |= 1 << bit,
            Some(false) => bits &= !(1 << bit),
            None => {}
        }
    }
    checked(annotation.set_flags(bits), "FPDFAnnot_SetFlags")
}

/// `setAnnotRect`.
pub fn set_rect(annotation: &Annotation<'_>, rect: PdfRect) -> Result<(), EngineError> {
    checked(
        annotation.set_rect(pdfium::RectF {
            left: rect.left as f32,
            top: rect.top as f32,
            right: rect.right as f32,
            bottom: rect.bottom as f32,
        }),
        "EPDFAnnot_SetRect",
    )
}

/// `writeAnnotString`.
pub fn set_string(annotation: &Annotation<'_>, key: &str, value: &str) -> Result<(), EngineError> {
    written(annotation.set_string_value(key, value), "FPDFAnnot_SetStringValue")
}

/// `writeAnnotStringOrClear`: `None` removes the entry, which an empty
/// string would not.
pub fn set_string_or_clear(
    annotation: &Annotation<'_>,
    key: &str,
    value: Option<&str>,
) -> Result<(), EngineError> {
    match value {
        Some(value) => set_string(annotation, key, value),
        None => {
            annotation.remove_key(key).map_err(|e| {
                EngineError::with_message(
                    EngineErrorCode::RuntimeUnavailable,
                    format!("EPDFAnnot_RemoveKey failed: {e}"),
                )
            })?;
            Ok(())
        }
    }
}

/// `setBorderStyle`: `/BS /S` and `/BS /W` in one call.
pub fn set_border_style(
    annotation: &Annotation<'_>,
    style_code: i32,
    width: f64,
) -> Result<(), EngineError> {
    checked(annotation.set_border_style(style_code, width as f32), "EPDFAnnot_SetBorderStyle")
}

/// `setBorderDashPattern`, each entry clamped to 0 or more.
pub fn set_border_dash_pattern(
    annotation: &Annotation<'_>,
    dashes: &[f64],
) -> Result<(), EngineError> {
    let dashes: Vec<f32> = dashes.iter().map(|d| d.max(0.0) as f32).collect();
    checked(annotation.set_border_dash_pattern(&dashes), "EPDFAnnot_SetBorderDashPattern")
}

/// `setBorderEffect`.
pub fn set_border_effect(annotation: &Annotation<'_>, intensity: f64) -> Result<(), EngineError> {
    checked(annotation.set_border_effect(intensity as f32), "EPDFAnnot_SetBorderEffect")
}

/// `clearRectangleDifferences`: benign when there is none. Drawing a box
/// kind's appearance writes `/RD` again when the drawing reaches past the
/// shape.
pub fn clear_rectangle_differences(annotation: &Annotation<'_>) {
    annotation.clear_rectangle_differences();
}

fn native_point(point: PdfPoint) -> pdfium::PointF {
    pdfium::PointF { x: point.x as f32, y: point.y as f32 }
}

/// `setVertices`.
pub fn set_vertices(annotation: &Annotation<'_>, vertices: &[PdfPoint]) -> Result<(), EngineError> {
    let points: Vec<pdfium::PointF> = vertices.iter().copied().map(native_point).collect();
    checked(annotation.set_vertices(&points), "EPDFAnnot_SetVertices")
}

/// `setLine`.
pub fn set_line(
    annotation: &Annotation<'_>,
    start: PdfPoint,
    end: PdfPoint,
) -> Result<(), EngineError> {
    checked(annotation.set_line(native_point(start), native_point(end)), "EPDFAnnot_SetLine")
}

/// `setInkList`: the list replaced whole, a stroke with no points skipped.
pub fn set_ink_list(
    annotation: &Annotation<'_>,
    strokes: &[Vec<PdfPoint>],
) -> Result<(), EngineError> {
    annotation.remove_ink_list();
    for stroke in strokes.iter().filter(|stroke| !stroke.is_empty()) {
        let points: Vec<pdfium::PointF> = stroke.iter().copied().map(native_point).collect();
        if annotation.add_ink_stroke(&points) < 0 {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "FPDFAnnot_AddInkStroke returned < 0",
            ));
        }
    }
    Ok(())
}

/// `setLineEndings`.
pub fn set_line_endings(
    annotation: &Annotation<'_>,
    endings: LineEndings,
) -> Result<(), EngineError> {
    checked(
        annotation.set_line_endings(endings.start.code(), endings.end.code()),
        "EPDFAnnot_SetLineEndings",
    )
}

/// `setIntent`.
pub fn set_intent(annotation: &Annotation<'_>, name: &str) -> Result<(), EngineError> {
    written(annotation.set_intent(name), "EPDFAnnot_SetIntent")
}

/// `setCalloutLine`.
pub fn set_callout_line(
    annotation: &Annotation<'_>,
    points: &[PdfPoint],
) -> Result<(), EngineError> {
    let points: Vec<pdfium::PointF> = points.iter().copied().map(native_point).collect();
    checked(annotation.set_callout_line(&points), "EPDFAnnot_SetCalloutLine")
}

/// `setTextAlignment`.
pub fn set_text_alignment(annotation: &Annotation<'_>, code: i32) -> Result<(), EngineError> {
    checked(annotation.set_text_alignment(code), "EPDFAnnot_SetTextAlignment")
}

/// A boolean entry (`EPDFAnnot_SetBooleanValue`).
pub fn set_boolean(annotation: &Annotation<'_>, key: &str, value: bool) -> Result<(), EngineError> {
    written(annotation.set_boolean_value(key, value), "EPDFAnnot_SetBooleanValue")
}

/// `/Name` (`EPDFAnnot_SetName`).
pub fn set_name(annotation: &Annotation<'_>, name: &str) -> Result<(), EngineError> {
    checked(annotation.set_name(name), "EPDFAnnot_SetName")
}

fn native_quad(quad: NamedPdfQuad) -> pdfium::QuadF {
    // FS_QUADPOINTSF holds the corners in the order Acrobat reads.
    pdfium::QuadF {
        x1: quad.upper_left.x as f32,
        y1: quad.upper_left.y as f32,
        x2: quad.upper_right.x as f32,
        y2: quad.upper_right.y as f32,
        x3: quad.lower_left.x as f32,
        y3: quad.lower_left.y as f32,
        x4: quad.lower_right.x as f32,
        y4: quad.lower_right.y as f32,
    }
}

/// `appendQuadPoints`.
pub fn append_quad_points(
    annotation: &Annotation<'_>,
    quads: &[NamedPdfQuad],
) -> Result<(), EngineError> {
    for quad in quads {
        checked(
            annotation.append_quad_point(native_quad(*quad)),
            "FPDFAnnot_AppendAttachmentPoints",
        )?;
    }
    Ok(())
}

/// `setRectFromQuadPoints`: `/Rect` is the smallest box enclosing every
/// quad.
pub fn set_rect_from_quad_points(
    annotation: &Annotation<'_>,
    quads: &[NamedPdfQuad],
) -> Result<(), EngineError> {
    let corners: Vec<PdfPoint> = quads
        .iter()
        .flat_map(|q| [q.upper_left, q.upper_right, q.lower_left, q.lower_right])
        .collect();
    let mut rect = PdfRect {
        left: f64::INFINITY,
        bottom: f64::INFINITY,
        right: f64::NEG_INFINITY,
        top: f64::NEG_INFINITY,
    };
    for point in corners {
        rect.left = rect.left.min(point.x);
        rect.right = rect.right.max(point.x);
        rect.bottom = rect.bottom.min(point.y);
        rect.top = rect.top.max(point.y);
    }
    set_rect(annotation, rect)
}

// ── /EMBD_Metadata ──────────────────────────────────────────────────────────

/// `EMBD_METADATA_SCHEMA_VERSION`.
pub const EMBD_METADATA_SCHEMA_VERSION: f32 = 1.0;

const KEY_ROTATION: &str = "Rotation";
const KEY_UNROTATED_RECT: &str = "UnrotatedRect";

/// Seed `/SchemaVersion` when the dictionary is about to be made by its
/// first key (`ensureSchemaVersion`).
pub fn ensure_schema_version(annotation: &Annotation<'_>) -> Result<(), EngineError> {
    if annotation.has_embed_metadata() {
        return Ok(());
    }
    written(
        annotation.set_embed_metadata_number("SchemaVersion", EMBD_METADATA_SCHEMA_VERSION),
        "EPDFAnnot_SetEmbedMetadataNumber",
    )
}

/// `writeEmbedMetadataString`.
pub fn set_embed_string(
    annotation: &Annotation<'_>,
    key: &str,
    value: &str,
) -> Result<(), EngineError> {
    written(annotation.set_embed_metadata_string(key, value), "EPDFAnnot_SetEmbedMetadataString")
}

fn clear_embed_key(annotation: &Annotation<'_>, key: &str) -> Result<(), EngineError> {
    annotation.clear_embed_metadata_key(key).map(|_| ()).map_err(|e| {
        EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            format!("EPDFAnnot_ClearEmbedMetadataKey failed: {e}"),
        )
    })
}

/// A turn as the file keeps it: degrees clockwise, and the box it turns
/// about the middle of (`AnnotationTurn`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedTurn {
    pub rotation: f64,
    pub content_box: PdfRect,
}

/// `writeRecordedTurn`: the turn in our keys (`/Rotation` counterclockwise,
/// `/UnrotatedRect`) and in Acrobat's `/Rotate` for the kinds Acrobat turns
/// (a stamp at any angle, a free text a quarter turn); `None` clears them.
pub fn write_recorded_turn(
    annotation: &Annotation<'_>,
    turn: Option<RecordedTurn>,
) -> Result<(), EngineError> {
    use crate::features::annotation::appearance::pdf_from_clockwise;
    use crate::features::annotation::subtype::code;
    match turn {
        Some(turn) => {
            ensure_schema_version(annotation)?;
            written(
                annotation.set_embed_metadata_number(
                    KEY_ROTATION,
                    pdf_from_clockwise(turn.rotation) as f32,
                ),
                "EPDFAnnot_SetEmbedMetadataNumber",
            )?;
            ensure_schema_version(annotation)?;
            let rect = turn.content_box;
            written(
                annotation.set_embed_metadata_rect(
                    KEY_UNROTATED_RECT,
                    pdfium::RectF {
                        left: rect.left as f32,
                        top: rect.top as f32,
                        right: rect.right as f32,
                        bottom: rect.bottom as f32,
                    },
                ),
                "EPDFAnnot_SetEmbedMetadataRect",
            )?;
        }
        None => {
            clear_embed_key(annotation, KEY_ROTATION)?;
            clear_embed_key(annotation, KEY_UNROTATED_RECT)?;
        }
    }
    // `writeAcrobatRotate`.
    let subtype = annotation.subtype_code();
    let rotation = turn.map(|turn| turn.rotation);
    let quarter = rotation.is_some_and(|r| [90.0, 180.0, 270.0].contains(&r));
    let kept =
        rotation.is_some() && (subtype == code::STAMP || (subtype == code::FREETEXT && quarter));
    if kept {
        annotation.set_rotate(pdf_from_clockwise(rotation.unwrap_or_default()) as f32);
    } else if subtype == code::STAMP || subtype == code::FREETEXT {
        annotation.set_rotate(0.0);
    }
    Ok(())
}

/// A turn of 0 is none (`turnOf`).
pub fn turn_of(rotation: Option<f64>) -> Option<f64> {
    rotation.filter(|degrees| *degrees != 0.0)
}

/// `writeAnnotationBox`: a box kind's box and turn written whole. The turn
/// in our keys (and `/Rotate`), `/Rect` the box or the upright box around
/// it turned, and no `/RD`: drawing the appearance writes it again when the
/// drawing reaches past the shape.
pub fn write_annotation_box(
    annotation: &Annotation<'_>,
    content_box: PdfRect,
    rotation: Option<f64>,
) -> Result<(), EngineError> {
    let content_box = crate::geom::normalize_pdf_rect(content_box);
    let rotation = turn_of(rotation);
    write_recorded_turn(
        annotation,
        rotation.map(|rotation| RecordedTurn { rotation, content_box }),
    )?;
    set_rect(
        annotation,
        match rotation {
            None => content_box,
            Some(rotation) => crate::geom::pdf_rect_turned_bounds(content_box, rotation),
        },
    )?;
    clear_rectangle_differences(annotation);
    Ok(())
}

/// A create's points placed (`writeNewPoints`): upright points turned by
/// `rotation` about the middle of their box, the turn recorded in our keys,
/// and `/Rect` the box around the drawn points until the appearance measures
/// itself. Answers the drawn points, in the lists they came in.
pub fn write_new_points(
    annotation: &Annotation<'_>,
    upright: &[Vec<PdfPoint>],
    rotation: Option<f64>,
) -> Result<Vec<Vec<PdfPoint>>, EngineError> {
    let all: Vec<PdfPoint> = upright.iter().flatten().copied().collect();
    let turn: Option<PdfPointTurn> = turn_of(rotation)
        .filter(|_| !all.is_empty())
        .map(|degrees| crate::geom::pdf_turn_of_upright(&all, degrees));
    write_recorded_turn(
        annotation,
        turn.map(|turn| RecordedTurn {
            rotation: turn.degrees,
            content_box: crate::geom::pdf_points_bounds(&all),
        }),
    )?;
    let drawn: Vec<Vec<PdfPoint>> = upright
        .iter()
        .map(|set| {
            set.iter()
                .map(|point| match turn {
                    Some(turn) => crate::geom::pdf_point_turned(*point, turn),
                    None => *point,
                })
                .collect()
        })
        .collect();
    let flat: Vec<PdfPoint> = drawn.iter().flatten().copied().collect();
    set_rect(annotation, crate::geom::pdf_points_bounds(&flat))?;
    Ok(drawn)
}
