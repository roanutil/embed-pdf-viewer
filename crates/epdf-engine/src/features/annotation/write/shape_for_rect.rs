//! An update's `rect` on a drawn kind, as the shape that puts the drawing
//! there.
//!
//! G11. Ports `core/src/annotation/shapeForRect.ts` (`pdfShapeForRect`,
//! `shapeFieldsOf`, `DRAWN_RECT_KINDS`) and
//! `core/src/annotation/resolve/rectCommand.ts` (`pdfResolveRectCommand`),
//! over the read's and the patch's JSON in the file's coordinates.

use serde_json::{Map, Value};

use super::appearance::semantic_equal;
use crate::error::{EngineError, EngineErrorCode};
use crate::geom::{PdfPoint, PdfRect, PdfRotation, normalize_pdf_rect, pdf_rect_turned_bounds};
use crate::page_space::annotations::{FieldSpace, field_spaces};

/// The kinds whose `rect` the engine works out from their shape: a box,
/// points or quads (`DRAWN_RECT_KINDS`).
pub const DRAWN_RECT_KINDS: &[&str] = &[
    "highlight",
    "underline",
    "squiggly",
    "strikeout",
    "square",
    "circle",
    "free-text",
    "stamp",
    "caret",
    "widget",
    "line",
    "polyline",
    "polygon",
    "ink",
];

/// Whether `subtype`'s `rect` is the engine's.
pub fn is_drawn_rect_kind(subtype: &str) -> bool {
    DRAWN_RECT_KINDS.contains(&subtype)
}

/// The places that make up a drawing. A measurement's origin isn't one: it
/// is where the scale's coordinates start, so it stays when the drawing
/// moves.
fn is_drawing_space(space: FieldSpace) -> bool {
    matches!(
        space,
        FieldSpace::Box
            | FieldSpace::Point
            | FieldSpace::Points
            | FieldSpace::Strokes
            | FieldSpace::Quads
            | FieldSpace::LinePoints
            | FieldSpace::CalloutLine
    )
}

/// The fields that hold a drawn kind's shape: its places, and its turn
/// (`shapeFieldsOf`).
pub fn shape_fields_of(subtype: &str) -> Vec<&'static str> {
    let mut fields: Vec<&'static str> = field_spaces(subtype)
        .iter()
        .filter(|(name, space)| *name != "rect" && is_drawing_space(*space))
        .map(|(name, _)| *name)
        .collect();
    fields.push("rotation");
    fields
}

fn refused(subtype: &str, reason: &str) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::InvalidArg,
        format!("{subtype} field 'rect': {reason}"),
    )
    .with_details(serde_json::json!({ "field": "rect" }))
}

/// `quarterTurnOf`: 0, 90, 180 or 270 for a turn that is a whole number of
/// quarter turns (either way round), `None` for any other.
pub fn quarter_turn_of(degrees: f64) -> Option<u32> {
    let turn = ((degrees % 360.0) + 360.0) % 360.0;
    [0.0, 90.0, 180.0, 270.0]
        .into_iter()
        .find(|quarter| semantic_equal(Some(&Value::from(turn)), Some(&Value::from(*quarter))))
        .map(|quarter| quarter as u32)
}

fn rect_of(value: &Value) -> Option<PdfRect> {
    let rect: PdfRect = serde_json::from_value(value.clone()).ok()?;
    [rect.left, rect.bottom, rect.right, rect.top].iter().all(|n| n.is_finite()).then_some(rect)
}

fn rotation_of(turn: u32) -> Option<PdfRotation> {
    match turn {
        90 => Some(PdfRotation::Clockwise90),
        180 => Some(PdfRotation::Clockwise180),
        270 => Some(PdfRotation::Clockwise270),
        _ => None,
    }
}

/// The shape fields that put `annotation` at `rect`, in the file's
/// coordinates (`pdfShapeForRect`).
///
/// A kind whose shape is its rect takes `rect` as it is. A drawn kind has
/// every place it holds mapped from its current `rect` onto `rect`, each axis
/// on its own: a rect of the same size moves the shape, a bigger one
/// stretches it. A box at a quarter turn stretches along its own sides, as
/// it stands on the page. Refused, `InvalidArg` on `rect`: a rect that would
/// squash it to no width or height, a rect of another size when it is turned
/// (it can only move) unless it is a box at a quarter turn, and one when its
/// own rect has no width or height to stretch.
pub fn pdf_shape_for_rect(
    annotation: &Map<String, Value>,
    rect: PdfRect,
) -> Result<Map<String, Value>, EngineError> {
    let subtype = annotation.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if !is_drawn_rect_kind(subtype) {
        let mut shape = Map::new();
        shape.insert("rect".into(), serde_json::to_value(rect).unwrap_or_default());
        return Ok(shape);
    }
    let to = normalize_pdf_rect(rect);
    let from = annotation
        .get("rect")
        .and_then(rect_of)
        .map(normalize_pdf_rect)
        .ok_or_else(|| refused(subtype, "its rect has no width or height to stretch"))?;
    let to_width = to.right - to.left;
    let to_height = to.top - to.bottom;
    let from_width = from.right - from.left;
    let from_height = from.top - from.bottom;
    let same = |a: f64, b: f64| semantic_equal(Some(&Value::from(a)), Some(&Value::from(b)));
    let same_width = same(to_width, from_width);
    let same_height = same(to_height, from_height);
    if (!same_width && to_width <= 0.0) || (!same_height && to_height <= 0.0) {
        return Err(refused(subtype, "a rect needs a width and a height"));
    }
    let rotation = annotation.get("rotation").and_then(Value::as_f64).unwrap_or(0.0);
    let spaces = field_spaces(subtype);
    let is_box = spaces.iter().any(|(name, space)| *name == "box" && *space == FieldSpace::Box);
    // A box at a quarter turn stands upright on the page: a rect pins it down.
    let quarter_turn = if is_box { quarter_turn_of(rotation) } else { None };
    if !same(((rotation % 360.0) + 360.0) % 360.0, 0.0)
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
    let point = |p: PdfPoint| PdfPoint {
        x: to.left + (p.x - from.left) * scale_x,
        y: to.bottom + (p.y - from.bottom) * scale_y,
    };
    let place_rect = |r: PdfRect| {
        let corner = point(PdfPoint { x: r.left, y: r.bottom });
        let opposite = point(PdfPoint { x: r.right, y: r.top });
        normalize_pdf_rect(PdfRect {
            left: corner.x,
            bottom: corner.y,
            right: opposite.x,
            top: opposite.y,
        })
    };
    let place_points = |value: &Value| -> Option<Value> {
        let points: Vec<PdfPoint> = serde_json::from_value(value.clone()).ok()?;
        serde_json::to_value(points.into_iter().map(point).collect::<Vec<_>>()).ok()
    };

    let mut shape = Map::new();
    for name in shape_fields_of(subtype) {
        let Some(value) = annotation.get(name).filter(|v| !v.is_null()) else { continue };
        let Some((_, space)) = spaces.iter().find(|(n, _)| *n == name) else {
            // `rotation`: a turn is no place, and the patch keeps it.
            continue;
        };
        let placed = match space {
            FieldSpace::Box => rect_of(value).map(|r| {
                let placed = match quarter_turn {
                    Some(turn @ (90 | 270)) => {
                        crate::features::annotation::read::record::pdf_quarter_turn_box(
                            place_rect(pdf_rect_turned_bounds(r, f64::from(turn))),
                            rotation_of(turn),
                        )
                    }
                    _ => place_rect(r),
                };
                serde_json::to_value(placed).unwrap_or_default()
            }),
            FieldSpace::Point => serde_json::from_value::<PdfPoint>(value.clone())
                .ok()
                .and_then(|p| serde_json::to_value(point(p)).ok()),
            FieldSpace::Points | FieldSpace::CalloutLine => place_points(value),
            FieldSpace::Strokes => value
                .as_array()
                .map(|strokes| strokes.iter().filter_map(place_points).collect::<Vec<_>>())
                .map(Value::Array),
            FieldSpace::Quads => value.as_array().map(|quads| {
                Value::Array(
                    quads
                        .iter()
                        .map(|quad| {
                            let mut placed = Map::new();
                            for corner in ["upperLeft", "upperRight", "lowerLeft", "lowerRight"] {
                                if let Some(p) = quad.get(corner).and_then(|c| {
                                    serde_json::from_value::<PdfPoint>(c.clone()).ok()
                                }) {
                                    placed.insert(
                                        corner.into(),
                                        serde_json::to_value(point(p)).unwrap_or_default(),
                                    );
                                }
                            }
                            Value::Object(placed)
                        })
                        .collect(),
                )
            }),
            FieldSpace::LinePoints => {
                let start = value.get("start").and_then(|p| serde_json::from_value(p.clone()).ok());
                let end = value.get("end").and_then(|p| serde_json::from_value(p.clone()).ok());
                match (start, end) {
                    (Some(start), Some(end)) => Some(serde_json::json!({
                        "start": point(start), "end": point(end),
                    })),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(placed) = placed {
            shape.insert(name.to_owned(), placed);
        }
    }
    Ok(shape)
}

/// An update's `rect` on a drawn kind, resolved as a command
/// (`pdfResolveRectCommand`): the rect it read, sent back, is dropped
/// (nothing moves); another rect becomes the shape fields that put the
/// drawing there. It can't come with a change to the shape itself, which is
/// refused (`InvalidArg` on `rect`), as is a rect that isn't four finite
/// numbers. A kind whose shape is its rect keeps it as a plain field.
pub fn pdf_resolve_rect_command(
    current: &Map<String, Value>,
    mut patch: Map<String, Value>,
) -> Result<Map<String, Value>, EngineError> {
    let subtype = current.get("subtype").and_then(Value::as_str).unwrap_or_default();
    if !is_drawn_rect_kind(subtype) || !patch.contains_key("rect") {
        return Ok(patch);
    }
    let sent = patch.get("rect").cloned().unwrap_or_default();
    let Some(rect) = rect_of(&sent) else {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{subtype} update field 'rect': not a rect of finite numbers"),
        )
        .with_details(serde_json::json!({ "field": "rect" })));
    };
    patch.remove("rect");
    // The rect it read, sent back: nothing moves.
    let normalized = serde_json::to_value(normalize_pdf_rect(rect)).unwrap_or_default();
    if semantic_equal(Some(&normalized), current.get("rect")) {
        return Ok(patch);
    }
    if let Some(changed) = shape_fields_of(subtype)
        .into_iter()
        .find(|name| patch.get(*name).is_some_and(|v| !semantic_equal(Some(v), current.get(*name))))
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "{subtype} update field 'rect': a new rect moves the shape, so it can't come with a new '{changed}'"
            ),
        )
        .with_details(serde_json::json!({ "field": "rect" })));
    }
    patch.extend(pdf_shape_for_rect(current, rect)?);
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_same_size_rect_moves_the_quads_and_a_rect_sent_back_is_dropped() {
        let current = map(json!({ "subtype": "highlight",
            "rect": { "left": 10, "bottom": 20, "right": 110, "top": 70 },
            "quadPoints": [{ "upperLeft": { "x": 10, "y": 70 }, "upperRight": { "x": 110, "y": 70 },
                "lowerLeft": { "x": 10, "y": 20 }, "lowerRight": { "x": 110, "y": 20 } }] }));
        let moved = pdf_resolve_rect_command(
            &current,
            map(json!({ "rect": { "left": 40, "bottom": 5, "right": 140, "top": 55 } })),
        )
        .unwrap();
        assert_eq!(moved["quadPoints"][0]["upperLeft"], json!({ "x": 40.0, "y": 55.0 }));
        assert!(!moved.contains_key("rect"));
        let kept = pdf_resolve_rect_command(
            &current,
            map(json!({ "rect": { "left": 10, "bottom": 20, "right": 110, "top": 70 } })),
        )
        .unwrap();
        assert!(kept.is_empty());
    }

    #[test]
    fn a_rect_with_a_new_shape_or_squashed_flat_is_refused_on_rect() {
        let current = map(json!({ "subtype": "polygon",
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 },
            "vertices": [{ "x": 0, "y": 0 }, { "x": 10, "y": 0 }, { "x": 5, "y": 10 }] }));
        let both = pdf_resolve_rect_command(
            &current,
            map(json!({ "rect": { "left": 5, "bottom": 5, "right": 15, "top": 15 },
                "vertices": [{ "x": 1, "y": 1 }, { "x": 2, "y": 1 }, { "x": 1, "y": 2 }] })),
        )
        .unwrap_err();
        assert_eq!(both.details.unwrap()["field"], "rect");
        let flat = pdf_resolve_rect_command(
            &current,
            map(json!({ "rect": { "left": 0, "bottom": 0, "right": 0, "top": 10 } })),
        )
        .unwrap_err();
        assert!(flat.message.contains("needs a width and a height"), "{flat}");
    }

    #[test]
    fn a_turned_drawing_only_moves_but_a_quarter_turned_box_stretches_along_its_sides() {
        let turned = map(json!({ "subtype": "ink", "rotation": 30,
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 },
            "inkList": [[{ "x": 0, "y": 0 }, { "x": 10, "y": 10 }]] }));
        let refused = pdf_resolve_rect_command(
            &turned,
            map(json!({ "rect": { "left": 0, "bottom": 0, "right": 20, "top": 10 } })),
        )
        .unwrap_err();
        assert!(refused.message.contains("can only move"), "{refused}");

        // A 20×10 box turned a quarter stands 10×20 on the page.
        let quarter = map(json!({ "subtype": "square", "rotation": 90,
            "rect": { "left": 5, "bottom": -5, "right": 15, "top": 15 },
            "box": { "left": 0, "bottom": 0, "right": 20, "top": 10 } }));
        let stretched = pdf_resolve_rect_command(
            &quarter,
            map(json!({ "rect": { "left": 5, "bottom": -5, "right": 15, "top": 35 } })),
        )
        .unwrap();
        let placed: PdfRect = serde_json::from_value(stretched["box"].clone()).unwrap();
        assert_eq!(placed.right - placed.left, 40.0, "the box's own width doubles");
        assert_eq!(placed.top - placed.bottom, 10.0);
    }

    #[test]
    fn a_kind_whose_shape_is_its_rect_keeps_the_rect() {
        let current = map(json!({ "subtype": "link",
            "rect": { "left": 0, "bottom": 0, "right": 10, "top": 10 } }));
        let patch = map(json!({ "rect": { "left": 5, "bottom": 5, "right": 15, "top": 15 } }));
        assert_eq!(pdf_resolve_rect_command(&current, patch.clone()).unwrap(), patch);
    }
}
