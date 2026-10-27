//! `shapes/box.ts`: the box family. Squares, circles and stamps, and the
//! kinds whose shape is their `rect` (note and attachment icons, links,
//! widgets, an area redaction). The moves, turns, scales and resizes of a
//! turned box serve the text box and caret families too.

use serde_json::Value;

use crate::cloudy::{cloudy_bounds, cloudy_outline, cloudy_path};
use crate::json::{self, Object};
use crate::painted::PaintedPiece;
use crate::rect::{
    RECT_HANDLES, expand_rect, inset_rect, normalize_deg, rect_center, rect_contains,
    rect_corner_points, rect_handle_point, resize_rotated_rect, rotate_point, rotated_aabb,
    rotated_handle_cursor, transposed_about_center,
};
use crate::types::{BoxShape, Handle, Placement, Point, Rect, RenderNode, Stroke};

/// The kinds that keep their box in `box` and turn it by `rotation`.
pub const TURNING_KINDS: [&str; 3] = ["square", "circle", "stamp"];

/// A circle, and a radio button, draw the ellipse in their box.
pub fn draws_ellipse(annotation: &Object) -> bool {
    let subtype = json::get_str(annotation, "subtype");
    subtype == Some("circle")
        || (subtype == Some("widget") && json::get_str(annotation, "fieldFamily") == Some("radio"))
}

fn rect_member(annotation: &Object, name: &str) -> Rect {
    annotation.get(name).map(json::rect_loose).unwrap_or(Rect {
        x: f64::NAN,
        y: f64::NAN,
        width: f64::NAN,
        height: f64::NAN,
    })
}

/// A box kind's shape, read off its annotation.
pub fn read_box(annotation: &Object) -> BoxShape {
    let subtype = json::get_str(annotation, "subtype").unwrap_or("");
    let ellipse = draws_ellipse(annotation);
    if !TURNING_KINDS.contains(&subtype) {
        return BoxShape {
            box_: rect_member(annotation, "rect"),
            rotation: 0.0,
            ellipse,
            order: Default::default(),
        };
    }
    BoxShape {
        box_: rect_member(annotation, "box"),
        rotation: json::get_num(annotation, "rotation").unwrap_or(0.0),
        ellipse,
        order: Default::default(),
    }
}

/// A box kind's shape where a create gesture places a box.
pub fn place_box(placement: &Placement, annotation: &Object) -> Option<BoxShape> {
    let Placement::Box(p) = placement else { return None };
    let keeps_turn = json::get_str(annotation, "subtype") == Some("stamp");
    let quarter = p.rot % 180.0 != 0.0;
    Some(BoxShape {
        box_: if !keeps_turn && quarter { transposed_about_center(p.rect) } else { p.rect },
        rotation: if keeps_turn { p.rot } else { 0.0 },
        ellipse: draws_ellipse(annotation),
        order: Default::default(),
    })
}

/// `rotation || null`: an upright turn is cleared rather than kept.
pub fn rotation_field(rotation: f64) -> Value {
    if rotation == 0.0 || rotation.is_nan() { Value::Null } else { json::num(rotation) }
}

/// The engine fields that state a box shape for a kind.
pub fn write_box(shape: &BoxShape, subtype: &str) -> Object {
    if TURNING_KINDS.contains(&subtype) {
        json::object([
            ("box", json::rect_value(shape.box_)),
            ("rotation", rotation_field(shape.rotation)),
        ])
    } else {
        json::object([("rect", json::rect_value(shape.box_))])
    }
}

/// What the shape draws, before its turn.
pub fn box_drawn_bounds(shape: &BoxShape, stroke: Stroke) -> Rect {
    match stroke.cloud() {
        Some(intensity) if shape.box_.width > 0.0 && shape.box_.height > 0.0 => {
            cloudy_bounds(shape.box_, shape.ellipse, intensity, stroke.stroke_width)
        }
        _ => shape.box_,
    }
}

/// The upright page box around what it paints.
pub fn box_rect(shape: &BoxShape, stroke: Stroke) -> Rect {
    rotated_aabb(box_drawn_bounds(shape, stroke), shape.rotation, Some(rect_center(shape.box_)))
}

/// A turned box's corners as the page shows them: nw, ne, se, sw.
pub fn box_corners(rect: Rect, rotation: f64) -> [Point; 4] {
    let middle = rect_center(rect);
    rect_corner_points(rect).map(|c| rotate_point(c, middle, rotation))
}

/// `isInTurnedBox(shape, point, margin)`.
pub fn is_in_turned_box(rect: Rect, rotation: f64, point: Point, margin: f64) -> bool {
    let turned = rotation != 0.0 && !rotation.is_nan();
    let local = if turned { rotate_point(point, rect_center(rect), -rotation) } else { point };
    rect_contains(expand_rect(rect, margin), local)
}

/// A box moved by `delta`.
pub fn translated(rect: Rect, delta: Point) -> Rect {
    Rect { x: rect.x + delta.x, y: rect.y + delta.y, ..rect }
}

fn box_around(middle: Point, width: f64, height: f64) -> Rect {
    Rect { x: middle.x - width / 2.0, y: middle.y - height / 2.0, width, height }
}

/// `boxRotateAbout`: the box's middle orbits the pivot and its turn grows.
pub fn rotated_about(rect: Rect, rotation: f64, pivot: Point, degrees: f64) -> (Rect, f64) {
    let middle = rotate_point(rect_center(rect), pivot, degrees);
    (box_around(middle, rect.width, rect.height), normalize_deg(rotation + degrees))
}

/// `boxScaleAbout`: the box's middle moves with the scale and its size scales.
pub fn scaled_about(rect: Rect, anchor: Point, sx: f64, sy: f64, min_size: f64) -> Rect {
    let middle = rect_center(rect);
    box_around(
        Point {
            x: anchor.x + (middle.x - anchor.x) * sx,
            y: anchor.y + (middle.y - anchor.y) * sy,
        },
        crate::js::max(min_size, rect.width * sx.abs()),
        crate::js::max(min_size, rect.height * sy.abs()),
    )
}

/// The eight resize handles, on the box as the page shows it.
pub fn box_handles(rect: Rect, rotation: f64) -> Vec<Handle> {
    let middle = rect_center(rect);
    RECT_HANDLES
        .iter()
        .map(|h| {
            Handle::maybe(
                *h,
                rotate_point(rect_handle_point(rect, h), middle, rotation),
                rotated_handle_cursor(h, rotation).map(str::to_string),
            )
        })
        .collect()
}

/// `boxResize`: the opposite handle stays where it is.
pub fn resized(rect: Rect, rotation: f64, handle: &str, to: Point) -> Rect {
    resize_rotated_rect(rect, rotation, handle, to)
}

/// What a box paints, turned with it.
pub fn box_painted(shape: &BoxShape, stroke: Stroke, filled: bool) -> Vec<PaintedPiece> {
    let half_width = stroke.stroke_width / 2.0;
    let BoxShape { box_: rect, rotation, ellipse, .. } = *shape;
    let middle = rect_center(rect);
    let turned = |points: Vec<Point>| -> Vec<Point> {
        if rotation != 0.0 && !rotation.is_nan() {
            points.into_iter().map(|p| rotate_point(p, middle, rotation)).collect()
        } else {
            points
        }
    };
    if let Some(intensity) = stroke.cloud().filter(|_| rect.width > 0.0 && rect.height > 0.0) {
        let curves: Vec<Vec<Point>> = cloudy_outline(rect, ellipse, intensity, stroke.stroke_width)
            .into_iter()
            .map(turned)
            .collect();
        let ink: Vec<PaintedPiece> = curves
            .iter()
            .map(|points| PaintedPiece::Stroke { points: points.clone(), closed: true, half_width })
            .collect();
        if !filled {
            return ink;
        }
        let inside = if ellipse {
            PaintedPiece::Oval {
                center: middle,
                rx: rect.width / 2.0,
                ry: rect.height / 2.0,
                rotation,
                half_width: 0.0,
                filled: true,
            }
        } else {
            PaintedPiece::Area { ring: box_corners(rect, rotation).to_vec() }
        };
        let mut out = vec![inside];
        out.extend(curves.into_iter().map(|ring| PaintedPiece::Area { ring }));
        out.extend(ink);
        return out;
    }
    let inset = inset_rect(rect, half_width);
    if ellipse {
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return Vec::new();
        }
        let rx = crate::js::max(0.01, inset.width / 2.0);
        let ry = crate::js::max(0.01, inset.height / 2.0);
        return vec![PaintedPiece::Oval { center: middle, rx, ry, rotation, half_width, filled }];
    }
    let ink = PaintedPiece::Stroke {
        points: turned(rect_corner_points(inset).to_vec()),
        closed: true,
        half_width,
    };
    if filled {
        vec![PaintedPiece::Area { ring: box_corners(rect, rotation).to_vec() }, ink]
    } else {
        vec![ink]
    }
}

/// What a box draws, before its turn: a cloud, or its outline inset half the stroke.
pub fn box_scene(shape: &BoxShape, stroke: Stroke) -> Vec<RenderNode> {
    if let Some(intensity) =
        stroke.cloud().filter(|_| shape.box_.width > 0.0 && shape.box_.height > 0.0)
    {
        return vec![RenderNode::path(cloudy_path(
            shape.box_,
            shape.ellipse,
            intensity,
            stroke.stroke_width,
        ))];
    }
    let rect = inset_rect(shape.box_, stroke.stroke_width / 2.0);
    vec![if shape.ellipse { RenderNode::ellipse(rect) } else { RenderNode::rect(rect) }]
}
