//! `shapes/`: the shape families, everything the core does with one kind of
//! shape. A kind names the family that reads its shape off the annotation
//! ([`Family`]); from then on the shape's own variant says which family
//! answers for it (`familyOf`).

pub mod boxes;
pub mod caret;
pub mod points;
pub mod quads;
pub mod text_box;

use crate::json::{self, Object};
use crate::painted::PaintedPiece;
use crate::rect::rect_center;
use crate::types::{Handle, Placement, Point, Rect, RenderNode, Shape, Stroke};

/// The family that reads a kind's shape (`ShapeFamily`, as a kind names it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Box,
    Points,
    Quads,
    Caret,
    TextBox,
    /// A redaction mark: quads over text, a box for an area (`familyChosenBy`).
    RedactMark,
}

impl Family {
    /// The family that reads `annotation`, a chosen family resolved.
    fn picked(self, annotation: &Object) -> Family {
        match self {
            Family::RedactMark => {
                let has_quads = json::get_str(annotation, "subtype") == Some("redact")
                    && annotation
                        .get("quadPoints")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|q| !q.is_empty());
                if has_quads { Family::Quads } else { Family::Box }
            }
            other => other,
        }
    }

    /// `family.read(annotation)`.
    pub fn read(self, annotation: &Object) -> Shape {
        match self.picked(annotation) {
            Family::Box => Shape::Box(boxes::read_box(annotation)),
            Family::Points => points::read_points(annotation),
            Family::Quads => Shape::Quads(quads::read_quads(annotation)),
            Family::Caret => Shape::Caret(caret::read_caret(annotation)),
            Family::TextBox => Shape::TextBox(text_box::read_text_box(annotation)),
            Family::RedactMark => unreachable!("a chosen family is resolved"),
        }
    }

    /// `family.placed(placement, annotation)`.
    pub fn placed(self, placement: &Placement, annotation: &Object) -> Option<Shape> {
        match self.picked(annotation) {
            Family::Box => boxes::place_box(placement, annotation).map(Shape::Box),
            Family::Points => points::place_points(placement, annotation),
            Family::TextBox => text_box::place_text_box(placement).map(Shape::TextBox),
            _ => None,
        }
    }
}

/// `familyOf(shape).write(shape, subtype)`: the engine fields that state it.
pub fn write(shape: &Shape, subtype: &str) -> Object {
    match shape {
        Shape::Box(s) => boxes::write_box(s, subtype),
        Shape::Line(_) | Shape::Poly(_) | Shape::Ink(_) => points::write_points(shape),
        Shape::Quads(s) => quads::write_quads(s),
        Shape::Caret(s) => caret::write_caret(s),
        Shape::TextBox(s) => text_box::write_text_box(s),
    }
}

/// `bounds`: the box around its box or points, the stroke left out.
pub fn bounds(shape: &Shape) -> Rect {
    match shape {
        Shape::Box(s) => s.box_,
        Shape::Caret(s) => s.box_,
        Shape::TextBox(s) => s.box_,
        Shape::Quads(s) => quads::quads_bounds(s),
        _ => points::points_bounds(shape),
    }
}

/// `drawnBounds`: the box the live drawing is painted into.
pub fn drawn_bounds(shape: &Shape, stroke: Stroke) -> Rect {
    match shape {
        Shape::Box(s) => boxes::box_drawn_bounds(s, stroke),
        Shape::Caret(s) => caret::caret_drawn_bounds(s),
        Shape::TextBox(s) => text_box::text_box_drawn_bounds(s, stroke),
        Shape::Quads(s) => quads::quads_drawn_bounds(s, stroke),
        _ => points::points_drawn_bounds(shape, stroke),
    }
}

/// `rect`: the upright page box around all it paints.
pub fn rect(shape: &Shape, stroke: Stroke) -> Rect {
    match shape {
        Shape::Box(s) => boxes::box_rect(s, stroke),
        Shape::Caret(s) => caret::caret_rect(s),
        Shape::TextBox(s) => text_box::text_box_rect(s, stroke),
        Shape::Quads(s) => quads::quads_drawn_bounds(s, stroke),
        _ => points::points_drawn_bounds(shape, stroke),
    }
}

/// `selectionBounds`: where its handles sit, and where a selected shape is grabbed.
pub fn selection_bounds(shape: &Shape, stroke: Stroke) -> Rect {
    match shape {
        Shape::Box(s) => s.box_,
        Shape::Caret(s) => s.box_,
        Shape::TextBox(s) => text_box::text_box_selection_bounds(s, stroke),
        Shape::Quads(s) => quads::quads_bounds(s),
        _ => points::points_drawn_bounds(shape, stroke),
    }
}

/// `oriented`: does the shape have a turned box of its own?
pub fn oriented(shape: &Shape) -> bool {
    match shape {
        Shape::Quads(_) => false,
        Shape::TextBox(s) => s.callout_line.is_null(),
        _ => true,
    }
}

/// `turnedCorners`: the turned box's corners, or `None`.
pub fn turned_corners(shape: &Shape, stroke: Stroke) -> Option<[Point; 4]> {
    match shape {
        Shape::Box(s) => Some(boxes::box_corners(s.box_, s.rotation)),
        Shape::Caret(s) => Some(boxes::box_corners(s.box_, s.rotation)),
        Shape::TextBox(s) => {
            s.callout_line.is_null().then(|| boxes::box_corners(s.box_, s.rotation))
        }
        Shape::Quads(_) => None,
        _ => Some(points::points_corners(shape, stroke)),
    }
}

/// `pivot`: where a turn of the shape pivots.
pub fn pivot(shape: &Shape) -> Point {
    match shape {
        Shape::Box(s) => rect_center(s.box_),
        Shape::Caret(s) => rect_center(s.box_),
        Shape::TextBox(s) => rect_center(s.box_),
        Shape::Quads(s) => quads::quads_centroid(s),
        _ => points::points_middle_of(shape),
    }
}

pub fn translate(shape: &Shape, delta: Point) -> Shape {
    match shape {
        Shape::Box(s) => Shape::Box(crate::types::BoxShape {
            box_: boxes::translated(s.box_, delta),
            ..s.clone()
        }),
        Shape::Caret(s) => Shape::Caret(crate::types::CaretShape {
            box_: boxes::translated(s.box_, delta),
            ..s.clone()
        }),
        Shape::TextBox(s) => Shape::TextBox(text_box::text_box_translate(s, delta)),
        Shape::Quads(s) => Shape::Quads(quads::quads_translate(s, delta)),
        _ => points::points_translate(shape, delta),
    }
}

pub fn rotate_about(shape: &Shape, pivot: Point, degrees: f64) -> Shape {
    match shape {
        Shape::Box(s) => {
            let (rect, rotation) = boxes::rotated_about(s.box_, s.rotation, pivot, degrees);
            Shape::Box(crate::types::BoxShape { box_: rect, rotation, ..s.clone() })
        }
        Shape::TextBox(s) => Shape::TextBox(text_box::text_box_rotate_about(s, pivot, degrees)),
        Shape::Caret(_) | Shape::Quads(_) => shape.clone(),
        _ => points::points_rotate_about(shape, pivot, degrees),
    }
}

pub fn scale_about(shape: &Shape, anchor: Point, sx: f64, sy: f64, min_size: Option<f64>) -> Shape {
    let min_size = min_size.unwrap_or(0.0);
    match shape {
        Shape::Box(s) => Shape::Box(crate::types::BoxShape {
            box_: boxes::scaled_about(s.box_, anchor, sx, sy, min_size),
            ..s.clone()
        }),
        Shape::Caret(s) => Shape::Caret(crate::types::CaretShape {
            box_: boxes::scaled_about(s.box_, anchor, sx, sy, min_size),
            ..s.clone()
        }),
        Shape::TextBox(s) => {
            Shape::TextBox(text_box::text_box_scale_about(s, anchor, sx, sy, min_size))
        }
        Shape::Quads(_) => shape.clone(),
        _ => points::points_scale_about(shape, anchor, sx, sy),
    }
}

pub fn upright(shape: &Shape, pivot: Option<Point>) -> Shape {
    match shape {
        Shape::Box(s) => Shape::Box(crate::types::BoxShape { rotation: 0.0, ..s.clone() }),
        Shape::Caret(s) => Shape::Caret(crate::types::CaretShape { rotation: 0.0, ..s.clone() }),
        Shape::TextBox(s) => Shape::TextBox(text_box::text_box_upright(s)),
        Shape::Quads(_) => shape.clone(),
        _ => points::points_upright(shape, pivot),
    }
}

pub fn handles(shape: &Shape) -> Vec<Handle> {
    match shape {
        Shape::Box(s) => boxes::box_handles(s.box_, s.rotation),
        Shape::TextBox(s) => text_box::text_box_handles(s),
        Shape::Caret(_) | Shape::Quads(_) => Vec::new(),
        _ => points::points_handles(shape),
    }
}

pub fn drag(shape: &Shape, handle: &str, to: Point) -> Shape {
    match shape {
        Shape::Box(s) => Shape::Box(crate::types::BoxShape {
            box_: boxes::resized(s.box_, s.rotation, handle, to),
            ..s.clone()
        }),
        Shape::TextBox(s) => Shape::TextBox(text_box::text_box_drag(s, handle, to)),
        Shape::Caret(_) | Shape::Quads(_) => shape.clone(),
        _ => points::points_drag(shape, handle, to),
    }
}

pub fn painted(shape: &Shape, stroke: Stroke, filled: bool) -> Vec<PaintedPiece> {
    match shape {
        Shape::Box(s) => boxes::box_painted(s, stroke, filled),
        Shape::Caret(s) => crate::painted::body_pieces(&boxes::box_corners(s.box_, s.rotation)),
        Shape::TextBox(s) => text_box::text_box_painted(s, stroke),
        Shape::Quads(s) => quads::quads_painted(s),
        _ => points::points_painted(shape, stroke, filled),
    }
}

pub fn scene(shape: &Shape, stroke: Stroke) -> Vec<RenderNode> {
    match shape {
        Shape::Box(s) => boxes::box_scene(s, stroke),
        Shape::Caret(s) => caret::caret_scene(s),
        Shape::TextBox(s) => text_box::text_box_scene(s, stroke),
        Shape::Quads(s) => quads::quads_scene(s),
        _ => points::points_scene(shape, stroke),
    }
}
