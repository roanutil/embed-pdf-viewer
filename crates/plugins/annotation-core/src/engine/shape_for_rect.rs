//! An update's `rect` on a drawn kind, as the shape fields that put the
//! drawing there (`annotation/shapeForRect.ts`), in the file's coordinates.

use epdf_geometry::{PdfRect, Point};
use serde_json::Value;

use super::appearance::semantic_equal;
use super::facts::{FieldSpace, is_drawn_rect_kind, kind_facts_of};
use super::geom::{
    normalize_pdf_rect, pdf_quarter_turn_box, pdf_rect_turned_bounds, quarter_turn_of,
};
use super::space::{pdf_rect_loose, pdf_rect_value};
use crate::error::{CoreError, CoreResult};
use crate::json::{self, Object};

const DRAWING_SPACES: [FieldSpace; 7] = [
    FieldSpace::Box,
    FieldSpace::Point,
    FieldSpace::Points,
    FieldSpace::Strokes,
    FieldSpace::Quads,
    FieldSpace::LinePoints,
    FieldSpace::CalloutLine,
];

/// `shapeFieldsOf(subtype)`: the places that make up a drawn kind's shape, then its turn.
pub fn shape_fields_of(subtype: &str) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = kind_facts_of(subtype)
        .map(|facts| {
            facts
                .spaces
                .iter()
                .filter(|(name, space)| *name != "rect" && DRAWING_SPACES.contains(space))
                .map(|(name, _)| *name)
                .collect()
        })
        .unwrap_or_default();
    names.push("rotation");
    names
}

fn refused(subtype: &str, reason: &str) -> CoreError {
    CoreError::invalid_field(format!("{subtype} field 'rect': {reason}"), "rect")
}

/// `pdfShapeForRect(annotation, rect)`.
pub fn pdf_shape_for_rect(annotation: &Object, rect: PdfRect) -> CoreResult<Object> {
    let subtype = json::get_str(annotation, "subtype").unwrap_or("");
    if !is_drawn_rect_kind(subtype) {
        return Ok(json::object([("rect", pdf_rect_value(rect))]));
    }
    let to = normalize_pdf_rect(rect);
    let from = normalize_pdf_rect(pdf_rect_loose(annotation.get("rect").unwrap_or(&Value::Null)));
    let to_width = to.right - to.left;
    let to_height = to.top - to.bottom;
    let from_width = from.right - from.left;
    let from_height = from.top - from.bottom;
    let same_width = semantic_equal(Some(&json::num(to_width)), Some(&json::num(from_width)));
    let same_height = semantic_equal(Some(&json::num(to_height)), Some(&json::num(from_height)));
    if (!same_width && to_width <= 0.0) || (!same_height && to_height <= 0.0) {
        return Err(refused(subtype, "a rect needs a width and a height"));
    }
    let rotation = json::get_num(annotation, "rotation").unwrap_or(0.0);
    let facts = kind_facts_of(subtype);
    let has_box = facts.and_then(|f| f.space("box")) == Some(FieldSpace::Box);
    let quarter_turn = if has_box { quarter_turn_of(rotation) } else { None };
    let normalized = ((rotation % 360.0) + 360.0) % 360.0;
    if !semantic_equal(Some(&json::num(normalized)), Some(&json::num(0.0)))
        && quarter_turn.is_none()
        && (!same_width || !same_height)
    {
        return Err(refused(
            subtype,
            "a turned drawing's rect can only move; resize its shape instead",
        ));
    }
    if (!same_width && from_width <= 0.0) || (!same_height && from_height <= 0.0) {
        return Err(refused(
            subtype,
            "its rect has no width or height to stretch; it can only move",
        ));
    }
    let scale_x = if same_width { 1.0 } else { to_width / from_width };
    let scale_y = if same_height { 1.0 } else { to_height / from_height };
    let point = |p: Point| Point {
        x: to.left + (p.x - from.left) * scale_x,
        y: to.bottom + (p.y - from.bottom) * scale_y,
    };
    let place_rect = |r: PdfRect| {
        let corner = point(Point { x: r.left, y: r.bottom });
        let opposite = point(Point { x: r.right, y: r.top });
        normalize_pdf_rect(PdfRect {
            left: corner.x,
            bottom: corner.y,
            right: opposite.x,
            top: opposite.y,
        })
    };
    let map_point = |v: &Value| json::point_value(point(json::point_loose(v)));
    let map_points = |v: &Value| {
        Value::Array(v.as_array().map(|a| a.iter().map(map_point).collect()).unwrap_or_default())
    };
    let place = |space: FieldSpace, value: &Value| -> Value {
        match space {
            FieldSpace::Box => {
                let r = pdf_rect_loose(value);
                match quarter_turn {
                    None | Some(0.0) | Some(180.0) => pdf_rect_value(place_rect(r)),
                    Some(turn) => pdf_rect_value(pdf_quarter_turn_box(
                        place_rect(pdf_rect_turned_bounds(r, turn)),
                        turn,
                    )),
                }
            }
            FieldSpace::Point => map_point(value),
            FieldSpace::Points | FieldSpace::CalloutLine => map_points(value),
            FieldSpace::Strokes => Value::Array(
                value.as_array().map(|a| a.iter().map(map_points).collect()).unwrap_or_default(),
            ),
            FieldSpace::Quads => Value::Array(
                value
                    .as_array()
                    .map(|quads| {
                        quads
                            .iter()
                            .map(|q| {
                                let corner = |n: &str| q.get(n).map_or(Value::Null, map_point);
                                Value::Object(json::object([
                                    ("upperLeft", corner("upperLeft")),
                                    ("upperRight", corner("upperRight")),
                                    ("lowerLeft", corner("lowerLeft")),
                                    ("lowerRight", corner("lowerRight")),
                                ]))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            FieldSpace::LinePoints => {
                let end = |n: &str| value.get(n).map_or(Value::Null, map_point);
                Value::Object(json::object([("start", end("start")), ("end", end("end"))]))
            }
            _ => value.clone(),
        }
    };
    let mut shape = Object::new();
    for name in shape_fields_of(subtype) {
        let Some(value) = annotation.get(name).filter(|v| !json::nullish(Some(v))) else {
            continue;
        };
        if let Some(space) = facts.and_then(|f| f.space(name)) {
            shape.insert(name.into(), place(space, value));
        }
    }
    Ok(shape)
}
