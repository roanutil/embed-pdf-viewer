//! Annotation values between page space and the file's coordinates, field
//! by field, by what each holds (`pageSpace/annotations.ts`).
//!
//! The viewer runs the engine's predictor on its own page-space values by
//! flipping them into y-up coordinates and back (`pageSpace/helpers.ts`,
//! `MIRROR`): the visible box is the zero rect, so a point `(x, y)` becomes
//! `(x + 0, 0 - y)`. Only that frame is ported.
//!
//! Destinations (`linkTarget`, `actions`) are carried as they are. On the
//! mirror frame the TypeScript converts them too, but no predictor rule
//! reads or writes one, and the page-space wrappers put back every value a
//! caller gave that no rule changed, so what comes out is the value that
//! went in either way. The `NaN` refusal still looks inside them.

use epdf_geometry::{PdfRect, Point, Rect};
use serde_json::Value;

use super::facts::{FieldSpace, space_of};
use crate::error::{CoreError, CoreResult};
use crate::json::{self, Object, num};

/// `pagePointOf(point, MIRROR)`.
fn page_point(p: Point) -> Point {
    Point { x: p.x - 0.0, y: 0.0 - p.y }
}

/// `pdfPointOf(point, MIRROR)`.
fn pdf_point(p: Point) -> Point {
    Point { x: p.x + 0.0, y: 0.0 - p.y }
}

/// `pageBoxOf(rect, MIRROR)`.
pub fn page_box(r: PdfRect) -> Rect {
    Rect { x: r.left - 0.0, y: 0.0 - r.top, width: r.right - r.left, height: r.top - r.bottom }
}

/// `pdfRectOf(box, MIRROR)`.
pub fn pdf_rect(b: Rect) -> PdfRect {
    let left = b.x + 0.0;
    let top = 0.0 - b.y;
    PdfRect { left, right: left + b.width, bottom: top - b.height, top }
}

pub fn pdf_rect_value(r: PdfRect) -> Value {
    Value::Object(json::object([
        ("left", num(r.left)),
        ("right", num(r.right)),
        ("bottom", num(r.bottom)),
        ("top", num(r.top)),
    ]))
}

/// A PDF rect member read loosely: a missing edge reads as NaN.
pub fn pdf_rect_loose(value: &Value) -> PdfRect {
    let object = value.as_object();
    let at = |name| object.and_then(|o| json::get_num(o, name)).unwrap_or(f64::NAN);
    PdfRect { left: at("left"), right: at("right"), bottom: at("bottom"), top: at("top") }
}

/// A PDF rect member when it is four numbers.
pub fn pdf_rect_of_value(value: &Value) -> Option<PdfRect> {
    let object = value.as_object()?;
    Some(PdfRect {
        left: json::get_num(object, "left")?,
        right: json::get_num(object, "right")?,
        bottom: json::get_num(object, "bottom")?,
        top: json::get_num(object, "top")?,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    ToPage,
    ToPdf,
}

fn map_point(value: &Value, direction: Direction) -> Value {
    let p = json::point_loose(value);
    json::point_value(match direction {
        Direction::ToPage => page_point(p),
        Direction::ToPdf => pdf_point(p),
    })
}

fn map_points(value: &Value, direction: Direction) -> Value {
    match value.as_array() {
        Some(items) => Value::Array(items.iter().map(|p| map_point(p, direction)).collect()),
        None => value.clone(),
    }
}

fn map_quad(value: &Value, direction: Direction) -> Value {
    let corner = |name: &str| value.get(name).map_or(Value::Null, |p| map_point(p, direction));
    Value::Object(json::object([
        ("upperLeft", corner("upperLeft")),
        ("upperRight", corner("upperRight")),
        ("lowerLeft", corner("lowerLeft")),
        ("lowerRight", corner("lowerRight")),
    ]))
}

fn map_measure(value: &Value, direction: Direction) -> Value {
    let Some(measure) = value.as_object() else { return value.clone() };
    let rectilinear = json::get_str(measure, "subtype") == Some("rectilinear");
    match measure.get("origin") {
        Some(origin) if rectilinear && json::truthy(Some(origin)) => {
            let mut out = measure.clone();
            out.insert("origin".into(), map_point(origin, direction));
            Value::Object(out)
        }
        _ => value.clone(),
    }
}

fn convert(space: FieldSpace, value: &Value, direction: Direction) -> Value {
    match space {
        FieldSpace::Length | FieldSpace::LinkTarget | FieldSpace::Actions => value.clone(),
        FieldSpace::Point => map_point(value, direction),
        FieldSpace::Points | FieldSpace::CalloutLine => map_points(value, direction),
        FieldSpace::Strokes => match value.as_array() {
            Some(strokes) => {
                Value::Array(strokes.iter().map(|s| map_points(s, direction)).collect())
            }
            None => value.clone(),
        },
        FieldSpace::Box => match direction {
            Direction::ToPage => json::rect_value(page_box(pdf_rect_loose(value))),
            Direction::ToPdf => pdf_rect_value(pdf_rect(json::rect_loose(value))),
        },
        FieldSpace::Quads => match value.as_array() {
            Some(quads) => Value::Array(quads.iter().map(|q| map_quad(q, direction)).collect()),
            None => value.clone(),
        },
        FieldSpace::LinePoints => {
            let end = |name: &str| value.get(name).map_or(Value::Null, |p| map_point(p, direction));
            Value::Object(json::object([("start", end("start")), ("end", end("end"))]))
        }
        FieldSpace::Measure => map_measure(value, direction),
    }
}

fn not_in_page_space(name: &str, space: FieldSpace) -> CoreError {
    let shape =
        if space == FieldSpace::Box { "{ x, y, width, height }" } else { "points as { x, y }" };
    CoreError::invalid_field(
        format!("field '{name}': expected page space, {shape} from the page's top-left"),
        name,
    )
}

fn convert_fields(value: &Object, direction: Direction) -> CoreResult<Object> {
    let subtype = json::get_str(value, "subtype");
    let mut out = Object::new();
    for (name, field) in value {
        let converted = match space_of(subtype, name) {
            Some(space) if !json::nullish(Some(field)) => {
                let converted = convert(space, field, direction);
                if direction == Direction::ToPdf && json::has_nan(&converted) {
                    return Err(not_in_page_space(name, space));
                }
                converted
            }
            _ => field.clone(),
        };
        out.insert(name.clone(), converted);
    }
    Ok(out)
}

/// A page-space annotation, draft or patch in the file's coordinates
/// (`pdfAnnotationOf`, `pdfAnnotationDraftOf`, `pdfAnnotationPatchOf`). A
/// place that holds `NaN` is refused: it wasn't given in page space.
pub fn to_pdf(value: &Object) -> CoreResult<Object> {
    convert_fields(value, Direction::ToPdf)
}

/// An annotation, draft or patch in the file's coordinates, in page space
/// (`pageAnnotationOf`, `pageAnnotationDraftOf`, `pageAnnotationPatchOf`).
pub fn to_page(value: &Object) -> Object {
    convert_fields(value, Direction::ToPage).unwrap_or_default()
}
