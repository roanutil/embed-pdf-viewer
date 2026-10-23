//! `shapes/text-box.ts`: free text. Its box, its turn, and a callout's line
//! (the tip, the knee when it bends, and the end on the box) with the
//! ending at the tip. The line's end follows the box as the engine's rule
//! has it ([`callout_end`]).

use crate::endings::{ending_nodes, ending_pieces, ending_points};
use crate::js;
use crate::json::{self, Object};
use crate::object::{Json, Nullable};
use crate::painted::{PaintedPiece, body_pieces};
use crate::rect::{
    expand_rect, inset_rect, rect_center, rect_corner_points, rotate_point, rotated_aabb,
    union_rect,
};
use crate::shapes::boxes::{
    box_corners, box_handles, resized, rotated_about, rotation_field, scaled_about, translated,
};
use crate::shapes::points::stroked_outline_of;
use crate::types::{Handle, Placement, Point, Rect, RenderNode, Stroke, TextBoxShape};

pub fn place_text_box(placement: &Placement) -> Option<TextBoxShape> {
    let Placement::Box(p) = placement else { return None };
    Some(TextBoxShape {
        box_: p.rect,
        rotation: p.rot,
        callout_line: Nullable::Null,
        line_ending: Nullable::Null,
        order: Default::default(),
    })
}

/// A free text's shape, read off its annotation. A line counts only on a callout.
pub fn read_text_box(annotation: &Object) -> TextBoxShape {
    let line = if json::get_str(annotation, "intent") == Some("free-text-callout") {
        json::get(annotation, "calloutLine")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().map(json::point_loose).collect::<Vec<Point>>())
    } else {
        None
    };
    let ending = line
        .as_ref()
        .map(|_| json::get_str(annotation, "lineEnding").unwrap_or("none").to_string());
    TextBoxShape {
        box_: annotation.get("box").map(json::rect_loose).unwrap_or(Rect {
            x: f64::NAN,
            y: f64::NAN,
            width: f64::NAN,
            height: f64::NAN,
        }),
        rotation: json::get_num(annotation, "rotation").unwrap_or(0.0),
        callout_line: Nullable::from_option(line),
        line_ending: Nullable::from_option(ending),
        order: Default::default(),
    }
}

pub fn write_text_box(shape: &TextBoxShape) -> Object {
    let mut out = json::object([
        ("box", json::rect_value(shape.box_)),
        ("rotation", rotation_field(shape.rotation)),
    ]);
    if let Some(line) = shape.callout_line.as_option() {
        out.insert("calloutLine".into(), line.json());
        let ending = shape.line_ending.as_option().cloned().unwrap_or_else(|| "none".into());
        out.insert("lineEnding".into(), serde_json::Value::String(ending));
    }
    out
}

/// `calloutEnd(box, toward, rotation)`: where a callout's line meets its box.
pub fn callout_end(rect: Rect, toward: Point, rotation: f64) -> Point {
    let middle = rect_center(rect);
    let turned = rotation != 0.0 && !rotation.is_nan();
    let local = if turned { rotate_point(toward, middle, -rotation) } else { toward };
    let dx = local.x - middle.x;
    let dy = local.y - middle.y;
    let end = if dx.abs() >= dy.abs() {
        Point { x: if dx >= 0.0 { rect.x + rect.width } else { rect.x }, y: middle.y }
    } else {
        Point { x: middle.x, y: if dy >= 0.0 { rect.y + rect.height } else { rect.y } }
    };
    if turned { rotate_point(end, middle, rotation) } else { end }
}

fn line_to(rect: Rect, rotation: f64, tip: Point, knee: Option<Point>) -> Vec<Point> {
    match knee {
        Some(knee) => vec![tip, knee, callout_end(rect, knee, rotation)],
        None => vec![tip, callout_end(rect, tip, rotation)],
    }
}

/// `calloutShape(box, rotation, tip, knee, lineEnding)`: a new callout.
pub fn callout_shape(
    rect: Rect,
    rotation: f64,
    tip: Point,
    knee: Option<Point>,
    line_ending: &str,
) -> TextBoxShape {
    TextBoxShape {
        box_: rect,
        rotation,
        callout_line: Nullable::Value(line_to(rect, rotation, tip, knee)),
        line_ending: Nullable::Value(line_ending.into()),
        order: Default::default(),
    }
}

/// The shape with its line's end where the line now meets the box.
fn end_follows(shape: TextBoxShape) -> TextBoxShape {
    let Some(line) = shape.callout_line.as_option().cloned() else { return shape };
    let knee = if line.len() == 3 { Some(line[1]) } else { None };
    let mut next = shape;
    next.callout_line = Nullable::Value(line_to(next.box_, next.rotation, line[0], knee));
    next
}

pub fn text_box_translate(shape: &TextBoxShape, delta: Point) -> TextBoxShape {
    let mut moved = shape.clone();
    moved.box_ = translated(shape.box_, delta);
    if let Some(line) = shape.callout_line.as_option() {
        moved.callout_line = Nullable::Value(
            line.iter().map(|p| Point { x: p.x + delta.x, y: p.y + delta.y }).collect(),
        );
    }
    moved
}

pub fn text_box_rotate_about(shape: &TextBoxShape, pivot: Point, degrees: f64) -> TextBoxShape {
    if !shape.callout_line.is_null() {
        return shape.clone();
    }
    let (rect, rotation) = rotated_about(shape.box_, shape.rotation, pivot, degrees);
    TextBoxShape { box_: rect, rotation, ..shape.clone() }
}

pub fn text_box_scale_about(
    shape: &TextBoxShape,
    anchor: Point,
    sx: f64,
    sy: f64,
    min_size: f64,
) -> TextBoxShape {
    if !shape.callout_line.is_null() {
        return shape.clone();
    }
    TextBoxShape { box_: scaled_about(shape.box_, anchor, sx, sy, min_size), ..shape.clone() }
}

pub fn text_box_upright(shape: &TextBoxShape) -> TextBoxShape {
    end_follows(TextBoxShape { rotation: 0.0, ..shape.clone() })
}

pub fn text_box_handles(shape: &TextBoxShape) -> Vec<Handle> {
    let mut handles = box_handles(shape.box_, shape.rotation);
    if let Some(line) = shape.callout_line.as_option() {
        handles.push(Handle::new("callout-tip", line[0], "crosshair"));
        if line.len() == 3 {
            handles.push(Handle::new("callout-knee", line[1], "crosshair"));
        }
    }
    handles
}

pub fn text_box_drag(shape: &TextBoxShape, handle: &str, to: Point) -> TextBoxShape {
    if let Some(line) = shape.callout_line.as_option() {
        if handle == "callout-tip" {
            let next = if line.len() == 3 { vec![to, line[1], line[2]] } else { vec![to, line[1]] };
            return end_follows(TextBoxShape {
                callout_line: Nullable::Value(next),
                ..shape.clone()
            });
        }
        if handle == "callout-knee" {
            return if line.len() == 3 {
                end_follows(TextBoxShape {
                    callout_line: Nullable::Value(vec![line[0], to, line[2]]),
                    ..shape.clone()
                })
            } else {
                shape.clone()
            };
        }
    }
    end_follows(TextBoxShape {
        box_: resized(shape.box_, shape.rotation, handle, to),
        ..shape.clone()
    })
}

fn tip_angle(line: &[Point]) -> f64 {
    (line[0].y - line[1].y).atan2(line[0].x - line[1].x)
}

fn ending_of(shape: &TextBoxShape) -> Option<&str> {
    shape.line_ending.as_option().map(String::as_str)
}

pub fn text_box_drawn_bounds(shape: &TextBoxShape, stroke: Stroke) -> Rect {
    let Some(line) = shape.callout_line.as_option() else { return shape.box_ };
    let mut points: Vec<Point> = box_corners(shape.box_, shape.rotation).to_vec();
    points.extend(line.iter().copied());
    if let Some(ending) = ending_of(shape) {
        points.extend(ending_points(line[0], tip_angle(line), Some(ending), stroke.stroke_width));
    }
    expand_rect(union_rect(&points), stroke.stroke_width / 2.0)
}

pub fn text_box_rect(shape: &TextBoxShape, stroke: Stroke) -> Rect {
    let rect = rotated_aabb(shape.box_, shape.rotation, None);
    let Some(line) = shape.callout_line.as_option() else { return rect };
    let end = line[line.len() - 1];
    let before = line[line.len() - 2];
    let length = js::hypot(end.x - before.x, end.y - before.y);
    let length = if length == 0.0 || length.is_nan() { 1.0 } else { length };
    let reach = stroke.stroke_width / 2.0 / length;
    let mut drawn: Vec<Point> = line[..line.len() - 1].to_vec();
    drawn.push(Point {
        x: end.x + (end.x - before.x) * reach,
        y: end.y + (end.y - before.y) * reach,
    });
    let mut nodes = vec![RenderNode::poly(drawn, false)];
    nodes.extend(ending_nodes(line[0], tip_angle(line), ending_of(shape), stroke.stroke_width));
    let mut all: Vec<Point> = rect_corner_points(rect).to_vec();
    all.extend(stroked_outline_of(&nodes, stroke.stroke_width));
    union_rect(&all)
}

pub fn text_box_selection_bounds(shape: &TextBoxShape, stroke: Stroke) -> Rect {
    if shape.callout_line.is_null() { shape.box_ } else { text_box_rect(shape, stroke) }
}

pub fn text_box_painted(shape: &TextBoxShape, stroke: Stroke) -> Vec<PaintedPiece> {
    let mut pieces = body_pieces(&box_corners(shape.box_, shape.rotation));
    let Some(line) = shape.callout_line.as_option() else { return pieces };
    pieces.push(PaintedPiece::Stroke {
        points: line.clone(),
        closed: false,
        half_width: stroke.stroke_width / 2.0,
    });
    let ending = ending_nodes(line[0], tip_angle(line), ending_of(shape), stroke.stroke_width);
    pieces.extend(ending_pieces(&ending, stroke.stroke_width));
    pieces
}

pub fn text_box_scene(shape: &TextBoxShape, stroke: Stroke) -> Vec<RenderNode> {
    let mut nodes = Vec::new();
    let sw = stroke.stroke_width;
    let line = shape.callout_line.as_option();
    if let Some(line) = line {
        let mut points = line.clone();
        let last = points[points.len() - 1];
        let previous = points[points.len() - 2];
        let length = js::hypot(last.x - previous.x, last.y - previous.y);
        if sw > 0.0 && length > 0.0 {
            let n = points.len();
            points[n - 1] = Point {
                x: last.x + ((last.x - previous.x) / length) * (sw / 2.0),
                y: last.y + ((last.y - previous.y) / length) * (sw / 2.0),
            };
        }
        nodes.push(RenderNode::poly(points, false));
        if let Some(ending) = ending_of(shape).filter(|e| !e.is_empty()) {
            nodes.extend(ending_nodes(line[0], tip_angle(line), Some(ending), sw));
        }
    }
    let border = inset_rect(shape.box_, sw / 2.0);
    if line.is_none() || shape.rotation == 0.0 || shape.rotation.is_nan() {
        nodes.push(RenderNode::rect(border));
        return nodes;
    }
    let middle = rect_center(shape.box_);
    nodes.push(RenderNode::poly(
        rect_corner_points(border).map(|c| rotate_point(c, middle, shape.rotation)).to_vec(),
        true,
    ));
    nodes
}

/// `textPlateInset(strokeWidth)`: twice the border width, none below 0.
pub fn text_plate_inset(stroke_width: f64) -> f64 {
    2.0 * js::max(0.0, stroke_width)
}
