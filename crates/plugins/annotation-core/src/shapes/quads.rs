//! `shapes/quads.ts`: text markup and text redaction, bound to their text.

use epdf_geometry::{quad_corners, quad_ring};

use crate::json::{self, Object};
use crate::painted::PaintedPiece;
use crate::rect::{expand_rect, union_rect};
use crate::types::{Point, Quad, QuadsShape, Rect, RenderNode, Stroke};

/// A text markup's or text redaction's shape, read off its annotation.
pub fn read_quads(annotation: &Object) -> QuadsShape {
    let quads = annotation
        .get("quadPoints")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|q| {
                    let corner = |n: &str| {
                        q.get(n)
                            .map(json::point_loose)
                            .unwrap_or(Point { x: f64::NAN, y: f64::NAN })
                    };
                    Quad {
                        upper_left: corner("upperLeft"),
                        upper_right: corner("upperRight"),
                        lower_left: corner("lowerLeft"),
                        lower_right: corner("lowerRight"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    QuadsShape { quad_points: quads, order: Default::default() }
}

pub fn quads_bounds(shape: &QuadsShape) -> Rect {
    let corners: Vec<Point> = shape.quad_points.iter().flat_map(|q| quad_corners(*q)).collect();
    union_rect(&corners)
}

pub fn quads_drawn_bounds(shape: &QuadsShape, stroke: Stroke) -> Rect {
    expand_rect(quads_bounds(shape), stroke.stroke_width / 2.0)
}

/// The middle of the quads' corners.
pub fn quads_centroid(shape: &QuadsShape) -> Point {
    let points: Vec<Point> = shape.quad_points.iter().flat_map(|q| quad_corners(*q)).collect();
    let count = if points.is_empty() { 1.0 } else { points.len() as f64 };
    let mut sx = 0.0;
    let mut sy = 0.0;
    for p in &points {
        sx += p.x;
        sy += p.y;
    }
    Point { x: sx / count, y: sy / count }
}

pub fn quads_translate(shape: &QuadsShape, delta: Point) -> QuadsShape {
    let mv = |p: Point| Point { x: p.x + delta.x, y: p.y + delta.y };
    let mut next = shape.clone();
    next.quad_points = shape
        .quad_points
        .iter()
        .map(|q| Quad {
            upper_left: mv(q.upper_left),
            upper_right: mv(q.upper_right),
            lower_left: mv(q.lower_left),
            lower_right: mv(q.lower_right),
        })
        .collect();
    next
}

pub fn quads_scene(shape: &QuadsShape) -> Vec<RenderNode> {
    shape.quad_points.iter().map(|q| RenderNode::poly(quad_ring(*q).to_vec(), true)).collect()
}

pub fn quads_painted(shape: &QuadsShape) -> Vec<PaintedPiece> {
    shape.quad_points.iter().map(|q| PaintedPiece::Area { ring: quad_ring(*q).to_vec() }).collect()
}

pub fn write_quads(shape: &QuadsShape) -> Object {
    use crate::object::Json;
    json::object([("quadPoints", shape.quad_points.json())])
}
