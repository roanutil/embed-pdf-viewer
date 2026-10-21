//! The turn an annotation is drawn with.
//!
//! G7. Ports `services/src/features/annotations/internal/read/`
//! `readAnnotationTurn.ts`, `readPointsTurn.ts` and
//! `readAnnotationTransformMetadata.ts`.
//!
//! The API's `rotation` is degrees clockwise; the file keeps PDF's
//! counterclockwise angle (`/EMBD_Metadata /Rotation`, Acrobat's `/Rotate`,
//! an appearance's matrix), so every read converts here, once:
//!
//! - a box kind (square, circle, free text, stamp, caret) reads what is
//!   drawn ([`read_annotation_box`]), with the `box` the drawing turns;
//! - a kind drawn from points (line, polyline, polygon, ink) reads our
//!   recorded turn while the points still match it
//!   ([`read_points_turn`]), and turns its points back upright.
//!
//! An upright annotation reads neither, and its `rotation` is `null`.

use pdfium::{Annotation, AppearanceMode, MatrixF, RectF};

use crate::features::annotation::subtype::code;
use crate::geom::{
    PdfPoint, PdfPointTurn, PdfRect, is_same_pdf_rect, normalize_pdf_rect, pdf_point_unturned,
    pdf_points_bounds, pdf_rect_turned_bounds,
};

/// The shortest decimal that is the same `f32` as `value`
/// (`float32Decimal`): a number a PDF stores as a float reads back as it was
/// written, `0.3` and not `0.30000001192092896`.
pub fn float32_decimal(value: f64) -> f64 {
    let float = value as f32;
    if !float.is_finite() {
        return f64::from(float);
    }
    for digits in 1..9 {
        let Ok(decimal) = format!("{:.*e}", digits - 1, f64::from(float)).parse::<f64>() else {
            continue;
        };
        if decimal as f32 == float {
            return decimal;
        }
    }
    f64::from(float)
}

/// A counterclockwise file angle as the API's clockwise one, in `[0, 360)`,
/// as it was written: `33.3` reads `33.3`. `None` for no turn
/// (`clockwiseFromPdf`).
pub fn clockwise_from_pdf(degrees: f64) -> Option<f64> {
    let clockwise = float32_decimal((((-float32_decimal(degrees)) % 360.0) + 360.0) % 360.0 + 0.0);
    (clockwise != 0.0 && clockwise != 360.0).then_some(clockwise)
}

/// `/EMBD_Metadata /Rotation`, where our writer records a turn.
const KEY_ROTATION: &str = "Rotation";
/// `/EMBD_Metadata /UnrotatedRect`: the box the recorded turn turns.
const KEY_UNROTATED_RECT: &str = "UnrotatedRect";

/// A kind drawn from points: its advisory rotation (degrees clockwise), or
/// `None` when absent or zero (`readAnnotationRotation`).
pub fn read_annotation_rotation(annotation: &Annotation<'_>) -> Option<f64> {
    if !annotation.has_embed_metadata() {
        return None;
    }
    let degrees = annotation.embed_metadata_number(KEY_ROTATION).ok().flatten()?;
    clockwise_from_pdf(f64::from(degrees))
}

fn rect_of(rect: RectF) -> PdfRect {
    PdfRect {
        left: f64::from(rect.left),
        bottom: f64::from(rect.bottom),
        right: f64::from(rect.right),
        top: f64::from(rect.top),
    }
}

/// `/EMBD_Metadata /UnrotatedRect`, normalized.
fn recorded_box(annotation: &Annotation<'_>) -> Option<PdfRect> {
    let rect = annotation.embed_metadata_rect(KEY_UNROTATED_RECT).ok().flatten()?;
    Some(normalize_pdf_rect(rect_of(rect)))
}

// ── points ──────────────────────────────────────────────────────────────────

/// The turn `drawn` was made with, while our keys still describe it: the
/// drawn points, turned back, have the recorded box. `None` upright, and
/// for points another app moved or redrew after we recorded a turn
/// (`readPointsTurn`).
pub fn read_points_turn(
    annotation: &Annotation<'_>,
    drawn: &[Vec<PdfPoint>],
) -> Option<PdfPointTurn> {
    let points: Vec<PdfPoint> = drawn.iter().flatten().copied().collect();
    if points.is_empty() {
        return None;
    }
    let rotation = read_annotation_rotation(annotation)?;
    let recorded = recorded_box(annotation)?;
    let turn = PdfPointTurn {
        degrees: rotation,
        center: PdfPoint {
            x: (recorded.left + recorded.right) / 2.0,
            y: (recorded.bottom + recorded.top) / 2.0,
        },
    };
    let upright: Vec<PdfPoint> =
        points.into_iter().map(|point| pdf_point_unturned(point, turn)).collect();
    is_same_pdf_rect(pdf_points_bounds(&upright), recorded).then_some(turn)
}

/// `point` as upright, turned back by `turn` when there is one
/// (`uprightPoint`).
pub fn upright_point(point: PdfPoint, turn: Option<PdfPointTurn>) -> PdfPoint {
    match turn {
        Some(turn) => pdf_point_unturned(point, turn),
        None => point,
    }
}

// ── boxes ───────────────────────────────────────────────────────────────────

/// How a box kind is drawn turned: degrees clockwise about the middle of
/// its own box (`AnnotationTurn`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnotationTurn {
    pub rotation: f64,
    pub content_box: PdfRect,
}

/// A box kind's box, and its turn (degrees clockwise) or `None` upright
/// (`AnnotationBox`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnnotationBox {
    pub content_box: PdfRect,
    pub rotation: Option<f64>,
}

/// Degrees: files write angles to a few decimals.
const ANGLE_TOLERANCE: f64 = 0.05;
/// Relative to the scale: how far a matrix may be from a turn times an even
/// scale.
const SHAPE_TOLERANCE: f64 = 2e-3;
const QUARTER_TURNS: [f64; 3] = [90.0, 180.0, 270.0];
/// Degrees: a recorded turn smaller than this either way is none.
const RECORDED_MIN: f64 = 0.01;

/// A turn in the file's sense: counterclockwise, in `[0, 360)`.
#[derive(Debug, Clone, Copy)]
struct Turn {
    degrees: f64,
    content_box: PdfRect,
}

fn normalize_degrees(degrees: f64) -> f64 {
    ((degrees % 360.0) + 360.0) % 360.0
}

fn angle_between(degrees: f64, other: f64) -> f64 {
    let difference = (normalize_degrees(degrees) - normalize_degrees(other)).abs();
    difference.min(360.0 - difference)
}

fn is_same_turn(degrees: f64, other: f64) -> bool {
    angle_between(degrees, other) <= ANGLE_TOLERANCE
}

/// The turn of a matrix that is a turn times an even scale,
/// counterclockwise; `None` for any other (`matrixTurn`).
fn matrix_turn(m: MatrixF) -> Option<f64> {
    let (a, b, c, d) = (f64::from(m.a), f64::from(m.b), f64::from(m.c), f64::from(m.d));
    let scale = a.hypot(b);
    // `!(scale > 0)` also refuses a NaN.
    if scale.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    let tolerance = SHAPE_TOLERANCE * scale;
    if (a - d).abs() > tolerance || (b + c).abs() > tolerance {
        return None;
    }
    let degrees = normalize_degrees(b.atan2(a) * 180.0 / std::f64::consts::PI);
    (angle_between(degrees, 0.0) > ANGLE_TOLERANCE).then_some(degrees)
}

/// `/Rotate` when the annotation has one, counterclockwise.
fn read_rotate(annotation: &Annotation<'_>) -> Option<f64> {
    if !annotation.has_key("Rotate").unwrap_or(false) {
        return None;
    }
    annotation.rotate().map(f64::from)
}

/// Our keys, as written.
fn read_recorded(annotation: &Annotation<'_>) -> Option<Turn> {
    if !annotation.has_embed_metadata() {
        return None;
    }
    let degrees = normalize_degrees(f64::from(
        annotation.embed_metadata_number(KEY_ROTATION).ok().flatten()?,
    ));
    if angle_between(degrees, 0.0) < RECORDED_MIN {
        return None;
    }
    let content_box = recorded_box(annotation)?;
    if content_box.right <= content_box.left || content_box.top <= content_box.bottom {
        return None;
    }
    Some(Turn { degrees, content_box })
}

/// The turn the appearance is drawn with, as a viewer places it
/// (`readDrawn`).
fn read_drawn(annotation: &Annotation<'_>, rotate: Option<f64>) -> Option<Turn> {
    let (form, m) = annotation.appearance_transform()?;
    let turn = matrix_turn(m)?;
    let form = normalize_pdf_rect(rect_of(form));
    let (a, b, c, d, e, f) = (
        f64::from(m.a),
        f64::from(m.b),
        f64::from(m.c),
        f64::from(m.d),
        f64::from(m.e),
        f64::from(m.f),
    );
    let scale = a.hypot(b);
    let cx = (form.left + form.right) / 2.0;
    let cy = (form.bottom + form.top) / 2.0;
    let x = a * cx + c * cy + e;
    let y = b * cx + d * cy + f;
    let half_width = (form.right - form.left) * scale / 2.0;
    let half_height = (form.top - form.bottom) * scale / 2.0;
    let degrees = match rotate {
        Some(rotate) if is_same_turn(rotate, turn) => normalize_degrees(rotate),
        _ => (turn * 100.0).round() / 100.0,
    };
    Some(Turn {
        degrees,
        content_box: PdfRect {
            left: x - half_width,
            bottom: y - half_height,
            right: x + half_width,
            top: y + half_height,
        },
    })
}

/// The upright box around the shape: `/Rect` less `/RD`, or `/Rect` when
/// `/RD` leaves nothing (`uprightShape`).
fn upright_shape(annotation: &Annotation<'_>) -> PdfRect {
    let raw = annotation.rect();
    let rect = normalize_pdf_rect(rect_of(raw));
    let Some(rd) = annotation.rectangle_differences() else {
        return rect;
    };
    let shape = PdfRect {
        left: rect.left + f64::from(rd.left),
        bottom: rect.bottom + f64::from(rd.bottom),
        right: rect.right - f64::from(rd.right),
        top: rect.top - f64::from(rd.top),
    };
    if shape.left <= shape.right && shape.bottom <= shape.top { shape } else { rect }
}

/// The kinds that turn as a box about its middle.
fn is_box_kind(subtype: i32) -> bool {
    matches!(subtype, code::SQUARE | code::CIRCLE | code::FREETEXT | code::STAMP | code::CARET)
}

/// The turn a box kind is drawn with (`readTurn`): the first of our
/// recorded keys while they still describe what is drawn, the appearance's
/// matrix, and a free text's quarter turn in `/Rotate`.
fn read_turn(annotation: &Annotation<'_>) -> Option<Turn> {
    let subtype = annotation.subtype_code();
    if !is_box_kind(subtype) {
        return None;
    }
    let rotate = read_rotate(annotation);
    let drawn = read_drawn(annotation, rotate);
    let frame = upright_shape(annotation);
    let is_free_text = subtype == code::FREETEXT;
    let quarter = match rotate {
        Some(rotate) if is_free_text => {
            QUARTER_TURNS.into_iter().find(|candidate| is_same_turn(rotate, *candidate))
        }
        _ => None,
    };

    if let Some(recorded) = read_recorded(annotation)
        && is_same_pdf_rect(pdf_rect_turned_bounds(recorded.content_box, recorded.degrees), frame)
    {
        let drawn_agrees = match drawn {
            Some(drawn) => is_same_turn(drawn.degrees, recorded.degrees),
            // No turn in the drawing's matrix: nothing drawn contradicts it,
            // or a free text draws its turn inside its stream (a callout
            // always, a text box Acrobat turned a quarter with its /Rotate).
            None => {
                !annotation.has_appearance_stream(AppearanceMode::Normal)
                    || (is_free_text
                        && (annotation.has_key("CL").unwrap_or(false)
                            || quarter.is_some_and(|q| is_same_turn(q, recorded.degrees))))
            }
        };
        if drawn_agrees {
            return Some(recorded);
        }
    }
    if drawn.is_some() {
        return drawn;
    }
    let quarter = quarter?;
    // A quarter turn keeps the frame upright: the box is the frame, its
    // sides swapped.
    if quarter == 180.0 {
        return Some(Turn { degrees: quarter, content_box: frame });
    }
    let x = (frame.left + frame.right) / 2.0;
    let y = (frame.bottom + frame.top) / 2.0;
    let half_width = (frame.top - frame.bottom) / 2.0;
    let half_height = (frame.right - frame.left) / 2.0;
    Some(Turn {
        degrees: quarter,
        content_box: PdfRect {
            left: x - half_width,
            bottom: y - half_height,
            right: x + half_width,
            top: y + half_height,
        },
    })
}

/// The turn a box kind is drawn with (clockwise, the API's), or `None` when
/// it is drawn upright (`readAnnotationTurn`).
pub fn read_annotation_turn(annotation: &Annotation<'_>) -> Option<AnnotationTurn> {
    let turn = read_turn(annotation)?;
    let rotation = clockwise_from_pdf(turn.degrees)?;
    Some(AnnotationTurn { rotation, content_box: turn.content_box })
}

/// A box kind's box and turn: the turned box, or upright `/Rect` less
/// `/RD` (`readAnnotationBox`).
pub fn read_annotation_box(annotation: &Annotation<'_>) -> AnnotationBox {
    match read_annotation_turn(annotation) {
        Some(turn) => {
            AnnotationBox { content_box: turn.content_box, rotation: Some(turn.rotation) }
        }
        None => AnnotationBox { content_box: upright_shape(annotation), rotation: None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_float_reads_back_as_the_decimal_it_was_written_as() {
        assert_eq!(float32_decimal(f64::from(0.3f32)), 0.3);
        assert_eq!(float32_decimal(f64::from(33.3f32)), 33.3);
        assert_eq!(float32_decimal(1.0), 1.0);
        assert_eq!(float32_decimal(f64::from(0.123_456_79f32)), 0.123_456_79);
        assert!(float32_decimal(f64::NAN).is_nan());
    }

    #[test]
    fn a_counterclockwise_angle_reads_clockwise() {
        // `annotation-rotation.test.ts`: a stored 30 reads 330.
        assert_eq!(clockwise_from_pdf(30.0), Some(330.0));
        assert_eq!(clockwise_from_pdf(-90.0), Some(90.0));
        assert_eq!(clockwise_from_pdf(f64::from(33.3f32)), Some(326.7));
        assert_eq!(clockwise_from_pdf(0.0), None);
        assert_eq!(clockwise_from_pdf(360.0), None);
        assert_eq!(clockwise_from_pdf(-0.0), None);
    }

    #[test]
    fn only_an_even_scale_times_a_turn_is_a_turn() {
        let turned = |degrees: f64, scale: f64| {
            let r = degrees.to_radians();
            MatrixF {
                a: (scale * r.cos()) as f32,
                b: (scale * r.sin()) as f32,
                c: (-scale * r.sin()) as f32,
                d: (scale * r.cos()) as f32,
                e: 10.0,
                f: 20.0,
            }
        };
        assert!((matrix_turn(turned(30.0, 2.0)).unwrap() - 30.0).abs() < 1e-4);
        assert!((matrix_turn(turned(-90.0, 1.0)).unwrap() - 270.0).abs() < 1e-4);
        assert_eq!(matrix_turn(turned(0.0, 1.0)), None);
        // A stretch is not a turn.
        assert_eq!(matrix_turn(MatrixF { a: 2.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }), None);
        assert_eq!(matrix_turn(MatrixF { a: 0.0, b: 0.0, c: 0.0, d: 0.0, e: 0.0, f: 0.0 }), None);
    }

    #[test]
    fn angles_compare_round_the_circle() {
        assert!(is_same_turn(359.98, 0.0));
        assert!(is_same_turn(-90.0, 270.0));
        assert!(!is_same_turn(90.0, 90.1));
    }
}
