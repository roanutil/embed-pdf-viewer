//! `geometry.ts`: page-space geometry for any shape. What one shape does its
//! family answers (`shapes/`); what spans families lives here: the
//! selection outline, the rotate knob, a multi-selection's resize, and
//! upright placement.

use epdf_geometry::{Size, js};

use crate::painted::{PaintedPiece, painted_near};
use crate::rect::{
    DEG2RAD, MIN_SIZE, RECT_HANDLES, normalize_deg, opposite_handle, point_in_poly,
    rect_corner_points, rect_cursor, rect_handle_point, resize_rect,
};
use crate::shapes;
use crate::types::{ChromeGeometry, Handle, Point, Rect, RenderNode, Shape, Stroke};

/// How far the rotate knob hangs off the top edge of the box.
pub const ROTATE_KNOB_OFFSET: f64 = 24.0;

/// `DEFAULT_CHROME_GEOMETRY`.
pub fn default_chrome_geometry() -> ChromeGeometry {
    ChromeGeometry {
        handle_tol: 6.0,
        knob_tol: 6.0,
        knob_offset: ROTATE_KNOB_OFFSET,
        rotation_handle: None,
        order: Default::default(),
    }
}

/// `geomRotation(geometry)`: the applied turn, 0 for quads.
pub fn geom_rotation(geometry: &Shape) -> f64 {
    match geometry {
        Shape::Quads(_) => 0.0,
        Shape::Box(s) => s.rotation,
        Shape::Line(s) => s.rotation,
        Shape::Poly(s) => s.rotation,
        Shape::Ink(s) => s.rotation,
        Shape::Caret(s) => s.rotation,
        Shape::TextBox(s) => s.rotation,
    }
}

/// `turnPivotOf(geometry)`.
pub fn turn_pivot_of(geometry: &Shape) -> Point {
    shapes::pivot(geometry)
}

/// `isRotatableGeom(geometry)`.
pub fn is_rotatable_geom(geometry: &Shape) -> bool {
    shapes::oriented(geometry)
}

/// `geomRotateAbout(geometry, pivot, deltaDeg)`.
pub fn geom_rotate_about(geometry: &Shape, pivot: Point, delta_deg: f64) -> Shape {
    if delta_deg == 0.0 {
        geometry.clone()
    } else {
        shapes::rotate_about(geometry, pivot, delta_deg)
    }
}

/// `uprightRotation(displayRotation)`.
pub fn upright_rotation(display_rotation: f64) -> f64 {
    normalize_deg(-display_rotation)
}

/// `uprightAnchoredRect(anchor, width, height, displayRotation)`.
pub fn upright_anchored_rect(
    anchor: Point,
    width: f64,
    height: f64,
    display_rotation: f64,
) -> Rect {
    let rotation = normalize_deg(display_rotation);
    let p = if rotation == 90.0 {
        Point { x: anchor.x + height / 2.0, y: anchor.y - width / 2.0 }
    } else if rotation == 180.0 {
        Point { x: anchor.x - width / 2.0, y: anchor.y - height / 2.0 }
    } else if rotation == 270.0 {
        Point { x: anchor.x - height / 2.0, y: anchor.y + width / 2.0 }
    } else {
        Point { x: anchor.x + width / 2.0, y: anchor.y + height / 2.0 }
    };
    Rect { x: p.x - width / 2.0, y: p.y - height / 2.0, width, height }
}

fn clamp_scalar(value: f64, lo: f64, hi: f64) -> f64 {
    js::max(lo, js::min(hi, value))
}

/// `fitStampBox(center, desired, page, rotCW)`.
pub fn fit_stamp_box(center: Point, desired: Size, page: Size, rot_cw: f64) -> Rect {
    let turn = normalize_deg(rot_cw);
    let quarter = epdf_geometry::is_quarter_turn(epdf_geometry::PageRotation(turn));
    let fw0 = if quarter { desired.height } else { desired.width };
    let fh0 = if quarter { desired.width } else { desired.height };
    let fit_scale = js::min_of(&[
        1.0,
        if fw0 > 0.0 { page.width / fw0 } else { 1.0 },
        if fh0 > 0.0 { page.height / fh0 } else { 1.0 },
    ]);
    let width = desired.width * fit_scale;
    let height = desired.height * fit_scale;
    let fw = if quarter { height } else { width };
    let fh = if quarter { width } else { height };
    let cx = clamp_scalar(center.x, fw / 2.0, page.width - fw / 2.0);
    let cy = clamp_scalar(center.y, fh / 2.0, page.height - fh / 2.0);
    Rect { x: cx - width / 2.0, y: cy - height / 2.0, width, height }
}

/// `geomResetRotation(geometry, pivot)`.
pub fn geom_reset_rotation(geometry: &Shape, pivot: Option<Point>) -> Shape {
    let rotation = geom_rotation(geometry);
    if rotation != 0.0 && !rotation.is_nan() {
        shapes::upright(geometry, pivot)
    } else {
        geometry.clone()
    }
}

/// An oriented selection box: four corners and the angle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Obb {
    pub corners: [Point; 4],
    pub angle: f64,
}

/// `obbFromGeom(geometry, stroke)`.
pub fn obb_from_geom(geometry: &Shape, stroke: Stroke) -> Option<Obb> {
    shapes::turned_corners(geometry, stroke)
        .map(|corners| Obb { corners, angle: geom_rotation(geometry) })
}

/// `rectHandlesFor(rect)`: the 8 box resize handles of a plain rect.
pub fn rect_handles_for(rect: Rect) -> Vec<Handle> {
    RECT_HANDLES
        .iter()
        .map(|h| Handle::new(*h, rect_handle_point(rect, h), rect_cursor(h)))
        .collect()
}

/// `groupResizeAnchor(base, handle)`.
///
/// `OPPOSITE_HANDLE[handle] ?? 'nw'` looks the handle up in a plain object:
/// a name `Object.prototype` has finds a function, which names no edge, so
/// the anchor is the box's middle.
pub fn group_resize_anchor(base: Rect, handle: &str) -> Point {
    let opposite = if crate::js::is_prototype_key(handle) {
        ""
    } else {
        opposite_handle(handle).unwrap_or("nw")
    };
    rect_handle_point(base, opposite)
}

/// `groupResizeBox(base, handle, to, isotropic)`.
pub fn group_resize_box(base: Rect, handle: &str, to: Point, isotropic: bool) -> Rect {
    let raw = resize_rect(base, handle, to);
    if !isotropic {
        return raw;
    }
    let sx = if base.width > 0.0 { raw.width / base.width } else { 1.0 };
    let sy = if base.height > 0.0 { raw.height / base.height } else { 1.0 };
    let scale = js::max(MIN_SIZE / js::max_of(&[base.width, base.height, 1.0]), js::max(sx, sy));
    let anchor = group_resize_anchor(base, handle);
    Rect {
        x: anchor.x + (base.x - anchor.x) * scale,
        y: anchor.y + (base.y - anchor.y) * scale,
        width: base.width * scale,
        height: base.height * scale,
    }
}

/// `groupResizeFactors(base, current)`: `(sx, sy)`.
pub fn group_resize_factors(base: Rect, current: Rect) -> (f64, f64) {
    (
        if base.width > 0.0 { current.width / base.width } else { 1.0 },
        if base.height > 0.0 { current.height / base.height } else { 1.0 },
    )
}

/// `geomScaleAbout(geometry, anchor, sx, sy, minSize)`.
pub fn geom_scale_about(
    geometry: &Shape,
    anchor: Point,
    sx: f64,
    sy: f64,
    min_size: Option<f64>,
) -> Shape {
    shapes::scale_about(geometry, anchor, sx, sy, min_size)
}

/// A rotate knob: the grab dot and the edge anchor its stalk draws to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knob {
    pub at: Point,
    pub from: Point,
}

/// `rotateKnob(corners, offset)`.
pub fn rotate_knob(corners: &[Point; 4], offset: f64) -> Knob {
    let [nw, ne, _, sw] = *corners;
    let from = Point { x: (nw.x + ne.x) / 2.0, y: (nw.y + ne.y) / 2.0 };
    let down = Point { x: sw.x - nw.x, y: sw.y - nw.y };
    let len = js::hypot(down.x, down.y);
    let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
    let up = Point { x: -down.x / len, y: -down.y / len };
    Knob { at: Point { x: from.x + up.x * offset, y: from.y + up.y * offset }, from }
}

fn inside_rect(rect: Rect, p: Point) -> bool {
    p.x >= rect.x && p.x <= rect.x + rect.width && p.y >= rect.y && p.y <= rect.y + rect.height
}

/// A chord: the line through a point at an angle, clipped to a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chord {
    pub a: Point,
    pub b: Point,
}

/// `chordThrough(box, point, angleDeg)`.
pub fn chord_through(rect: Rect, point: Point, angle_deg: f64) -> Option<Chord> {
    let direction = Point { x: (angle_deg * DEG2RAD).cos(), y: (angle_deg * DEG2RAD).sin() };
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for (dc, pc, lo, hi) in [
        (direction.x, point.x, rect.x, rect.x + rect.width),
        (direction.y, point.y, rect.y, rect.y + rect.height),
    ] {
        if dc.abs() < 1e-12 {
            if pc < lo || pc > hi {
                return None;
            }
            continue;
        }
        let t1 = (lo - pc) / dc;
        let t2 = (hi - pc) / dc;
        t_min = js::max(t_min, js::min(t1, t2));
        t_max = js::min(t_max, js::max(t1, t2));
    }
    if t_min >= t_max || !t_min.is_finite() || !t_max.is_finite() {
        return None;
    }
    Some(Chord {
        a: Point { x: point.x + direction.x * t_min, y: point.y + direction.y * t_min },
        b: Point { x: point.x + direction.x * t_max, y: point.y + direction.y * t_max },
    })
}

/// `placeRotateKnob(corners, offset, pageBox)`: kept inside the page.
pub fn place_rotate_knob(corners: &[Point; 4], offset: f64, page_box: Option<Rect>) -> Knob {
    let top = rotate_knob(corners, offset);
    let Some(page) = page_box else { return top };
    if inside_rect(page, top.at) {
        return top;
    }
    let [nw, _, se, sw] = *corners;
    let from = Point { x: (sw.x + se.x) / 2.0, y: (sw.y + se.y) / 2.0 };
    let down = Point { x: sw.x - nw.x, y: sw.y - nw.y };
    let len = js::hypot(down.x, down.y);
    let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
    let at = Point { x: from.x + (down.x / len) * offset, y: from.y + (down.y / len) * offset };
    if inside_rect(page, at) {
        return Knob { at, from };
    }
    Knob {
        at: Point {
            x: js::min(js::max(top.at.x, page.x), page.x + page.width),
            y: js::min(js::max(top.at.y, page.y), page.y + page.height),
        },
        from: top.from,
    }
}

/// `geomVisualBounds(geometry, stroke)`.
pub fn geom_visual_bounds(geometry: &Shape, stroke: Stroke) -> Rect {
    shapes::drawn_bounds(geometry, stroke)
}

/// `selectionBounds(geometry, stroke)`.
pub fn selection_bounds(geometry: &Shape, stroke: Stroke) -> Rect {
    shapes::selection_bounds(geometry, stroke)
}

/// `selectionQuad(geometry, stroke)`: the oriented selection box's corners.
pub fn selection_quad(geometry: &Shape, stroke: Stroke) -> [Point; 4] {
    match obb_from_geom(geometry, stroke) {
        Some(obb) => obb.corners,
        None => rect_corner_points(selection_bounds(geometry, stroke)),
    }
}

/// `pointInQuad(point, quad)`.
pub fn point_in_quad(point: Point, quad: &[Point; 4]) -> bool {
    point_in_poly(point, quad)
}

/// `geomBounds(geometry)`.
pub fn geom_bounds(geometry: &Shape) -> Rect {
    shapes::bounds(geometry)
}

/// `geomPainted(geometry, stroke, filled)`: what the shape paints.
pub fn geom_painted(geometry: &Shape, stroke: Stroke, filled: bool) -> Vec<PaintedPiece> {
    shapes::painted(geometry, stroke, filled)
}

/// `geomHit(geometry, point, margin, filled, stroke)`.
pub fn geom_hit(geometry: &Shape, point: Point, margin: f64, filled: bool, stroke: Stroke) -> bool {
    painted_near(&geom_painted(geometry, stroke, filled), point, margin)
}

/// `geomHandles(geometry)`.
pub fn geom_handles(geometry: &Shape) -> Vec<Handle> {
    shapes::handles(geometry)
}

/// `geomTranslate(geometry, delta)`.
pub fn geom_translate(geometry: &Shape, delta: Point) -> Shape {
    shapes::translate(geometry, delta)
}

/// `geomDragHandle(geometry, handle, to)`.
pub fn geom_drag_handle(geometry: &Shape, handle: &str, to: Point) -> Shape {
    shapes::drag(geometry, handle, to)
}

/// `geomScene(geometry, stroke)`.
pub fn geom_scene(geometry: &Shape, stroke: Stroke) -> Vec<RenderNode> {
    shapes::scene(geometry, stroke)
}
