//! `shapes/caret.ts`: an insertion mark on its text, in a box turned by its
//! text's baseline tilt.

use crate::js;
use crate::json::{self, Object};
use crate::rect::{normalize_deg, rect_center, rotated_aabb};
use crate::shapes::boxes::rotation_field;
use crate::types::{CaretShape, Rect, RenderNode, TextEndAnchor};

pub fn read_caret(annotation: &Object) -> CaretShape {
    CaretShape {
        box_: annotation.get("box").map(json::rect_loose).unwrap_or(Rect {
            x: f64::NAN,
            y: f64::NAN,
            width: f64::NAN,
            height: f64::NAN,
        }),
        rotation: json::get_num(annotation, "rotation").unwrap_or(0.0),
        order: Default::default(),
    }
}

pub fn write_caret(shape: &CaretShape) -> Object {
    json::object([
        ("box", json::rect_value(shape.box_)),
        ("rotation", rotation_field(shape.rotation)),
    ])
}

/// `caretRectFromAnchor(anchor)`: the upright caret box for a text-edit anchor.
pub fn caret_rect_from_anchor(anchor: &TextEndAnchor) -> Rect {
    let q = anchor.glyph_quad;
    let ink = js::hypot(q.lower_left.x - q.upper_left.x, q.lower_left.y - q.upper_left.y);
    let size = js::max(ink / 2.0, 1.0);
    let corner = if anchor.advance > 0.0 { q.lower_right } else { q.lower_left };
    Rect { x: corner.x - size / 2.0, y: corner.y - size, width: size, height: size }
}

const UPRIGHT_EPSILON: f64 = 0.05;

/// `caretFromAnchor(anchor)`: the caret turned by its text's baseline tilt.
pub fn caret_from_anchor(anchor: &TextEndAnchor) -> CaretShape {
    let q = anchor.glyph_quad;
    let ink = js::hypot(q.lower_left.x - q.upper_left.x, q.lower_left.y - q.upper_left.y);
    let size = js::max(ink / 2.0, 1.0);
    let corner = if anchor.advance > 0.0 { q.lower_right } else { q.lower_left };
    let bx = q.lower_right.x - q.lower_left.x;
    let by = q.lower_right.y - q.lower_left.y;
    let rotation = if js::hypot(bx, by) > 0.0 {
        normalize_deg((by.atan2(bx) * 180.0) / std::f64::consts::PI)
    } else {
        0.0
    };
    if rotation < UPRIGHT_EPSILON || rotation > 360.0 - UPRIGHT_EPSILON {
        return CaretShape {
            box_: caret_rect_from_anchor(anchor),
            rotation: 0.0,
            order: Default::default(),
        };
    }
    let ux = (q.upper_left.x - q.lower_left.x) / ink;
    let uy = (q.upper_left.y - q.lower_left.y) / ink;
    let cx = corner.x + (ux * size) / 2.0;
    let cy = corner.y + (uy * size) / 2.0;
    CaretShape {
        box_: Rect { x: cx - size / 2.0, y: cy - size / 2.0, width: size, height: size },
        rotation,
        order: Default::default(),
    }
}

/// The width of the caret mark's outline.
pub const CARET_STROKE_WIDTH: f64 = 0.5;

fn n(x: f64) -> String {
    js::number_to_string(x)
}

/// The caret mark in its box, before its turn.
pub fn caret_scene(shape: &CaretShape) -> Vec<RenderNode> {
    let Rect { x, y, width, height } = shape.box_;
    let mid_x = x + width / 2.0;
    let bottom = y + height;
    let d = [
        format!("M {} {}", n(x), n(bottom)),
        format!(
            "C {} {} {} {} {} {}",
            n(x + width * 0.27),
            n(bottom),
            n(mid_x),
            n(y + height * 0.56),
            n(mid_x),
            n(y)
        ),
        format!(
            "C {} {} {} {} {} {}",
            n(mid_x),
            n(y + height * 0.56),
            n(x + width * 0.73),
            n(bottom),
            n(x + width),
            n(bottom)
        ),
    ]
    .join(" ");
    vec![RenderNode::path(d)]
}

/// What the caret draws, before its turn: its box and half the outline below its base.
pub fn caret_drawn_bounds(shape: &CaretShape) -> Rect {
    Rect { height: shape.box_.height + CARET_STROKE_WIDTH / 2.0, ..shape.box_ }
}

pub fn caret_rect(shape: &CaretShape) -> Rect {
    rotated_aabb(caret_drawn_bounds(shape), shape.rotation, Some(rect_center(shape.box_)))
}
