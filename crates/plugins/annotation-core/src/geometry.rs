//! `geometry.ts`: pure content-space geometry dispatched on [`Geom`]: bounds,
//! hit-testing, handles, translate, handle drags, rotation, group scaling,
//! the rotate knob, callout leaders, carets, the geometry scene, and the
//! PDF/content bridge.
//!
//! Handles are strings, as in the TypeScript: `resizeRect` and the handle
//! tables accept any name and treat an unknown one as naming no edge.

use serde::{Deserialize, Serialize};

use epdf_geometry::{
    PageRotation, apply_point, apply_rect, invert, is_quarter_turn, pdf_to_content_matrix,
    rotate_about, text_quad_points, text_quad_ring,
};

use crate::cloudy::{cloudy_border_extent, cloudy_path, cloudy_poly_path};
use crate::endings::{ending_nodes, ending_points};
use crate::js;
use crate::types::{
    Border, Callout, Cursor, Geom, Handle, LineEnding, PdfPoint, PdfRect, Point, Quad, Rect,
    RenderNode, Size, Style, TextEndAnchor, TextQuad,
};

const MIN_SIZE: f64 = 4.0;

// ── rect handles ────────────────────────────────────────────────────────────

pub const RECT_HANDLES: [&str; 8] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

fn rect_cursor(h: &str) -> &'static str {
    match h {
        "nw" | "se" => "nwse-resize",
        "ne" | "sw" => "nesw-resize",
        "n" | "s" => "ns-resize",
        _ => "ew-resize",
    }
}

fn handle_base_angle(h: &str) -> Option<f64> {
    Some(match h {
        "n" => 0.0,
        "ne" => 45.0,
        "e" => 90.0,
        "se" => 135.0,
        "s" => 180.0,
        "sw" => 225.0,
        "w" => 270.0,
        "nw" => 315.0,
        _ => return None,
    })
}

const SECTOR_CURSORS: [&str; 8] = [
    "ns-resize",
    "nesw-resize",
    "ew-resize",
    "nwse-resize",
    "ns-resize",
    "nesw-resize",
    "ew-resize",
    "nwse-resize",
];

/// The resize cursor for a handle on a box tilted by `rot` degrees: the
/// handle's compass direction turned with the box, to the nearest 45°
/// sector. `None` where the TypeScript indexes past the table (an unknown
/// handle, a non-finite angle) and reads `undefined`.
pub fn rotated_handle_cursor(h: &str, rot: f64) -> Option<&'static str> {
    let sector = js::round(normalize_deg(handle_base_angle(h)? + rot) / 45.0) % 8.0;
    if sector.is_nan() { None } else { SECTOR_CURSORS.get(sector as usize).copied() }
}

struct Edges {
    w: bool,
    e: bool,
    n: bool,
    s: bool,
}

fn rect_edges(h: &str) -> Edges {
    Edges {
        w: matches!(h, "nw" | "w" | "sw"),
        e: matches!(h, "ne" | "e" | "se"),
        n: matches!(h, "nw" | "n" | "ne"),
        s: matches!(h, "sw" | "s" | "se"),
    }
}

fn rect_handle_point(r: Rect, h: &str) -> Point {
    let e = rect_edges(h);
    Point {
        x: if e.w {
            r.x
        } else if e.e {
            r.x + r.width
        } else {
            r.x + r.width / 2.0
        },
        y: if e.n {
            r.y
        } else if e.s {
            r.y + r.height
        } else {
            r.y + r.height / 2.0
        },
    }
}

fn resize_rect(base: Rect, h: &str, to: Point) -> Rect {
    let e = rect_edges(h);
    let mut l = base.x;
    let mut r = base.x + base.width;
    let mut t = base.y;
    let mut b = base.y + base.height;
    if e.w {
        l = to.x;
    }
    if e.e {
        r = to.x;
    }
    if e.n {
        t = to.y;
    }
    if e.s {
        b = to.y;
    }
    Rect {
        x: js::min(l, r),
        y: js::min(t, b),
        width: js::max(MIN_SIZE, (r - l).abs()),
        height: js::max(MIN_SIZE, (b - t).abs()),
    }
}

fn opposite_handle(h: &str) -> Option<&'static str> {
    Some(match h {
        "nw" => "se",
        "ne" => "sw",
        "se" => "nw",
        "sw" => "ne",
        "n" => "s",
        "s" => "n",
        "e" => "w",
        "w" => "e",
        _ => return None,
    })
}

// ── small math ──────────────────────────────────────────────────────────────

pub fn rect_from_points(a: Point, b: Point) -> Rect {
    Rect {
        x: js::min(a.x, b.x),
        y: js::min(a.y, b.y),
        width: (b.x - a.x).abs(),
        height: (b.y - a.y).abs(),
    }
}

pub fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x <= b.x + b.width && a.x + a.width >= b.x && a.y <= b.y + b.height && a.y + a.height >= b.y
}

fn rect_contains(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

/// Distance from `p` to segment `ab`.
fn seg_dist(p: Point, a: Point, b: Point) -> f64 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        js::max(0.0, js::min(1.0, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2))
    };
    js::hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy))
}

/// Even-odd point in polygon.
pub fn point_in_poly(p: Point, pts: &[Point]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let a = pts[i];
        let b = pts[j];
        if (a.y > p.y) != (b.y > p.y) && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn union_rect(pts: &[Point]) -> Rect {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for p in pts {
        x0 = js::min(x0, p.x);
        y0 = js::min(y0, p.y);
        x1 = js::max(x1, p.x);
        y1 = js::max(y1, p.y);
    }
    Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

fn expand_rect(r: Rect, pad: f64) -> Rect {
    Rect { x: r.x - pad, y: r.y - pad, width: r.width + 2.0 * pad, height: r.height + 2.0 * pad }
}

fn rect_corner_points(r: Rect) -> Quad {
    [
        Point { x: r.x, y: r.y },
        Point { x: r.x + r.width, y: r.y },
        Point { x: r.x + r.width, y: r.y + r.height },
        Point { x: r.x, y: r.y + r.height },
    ]
}

// ── rotation ────────────────────────────────────────────────────────────────

const DEG2RAD: f64 = std::f64::consts::PI / 180.0;

/// How far (content units) the rotate knob hangs off the top edge.
pub const ROTATE_KNOB_OFFSET: f64 = 24.0;

/// Fallback chrome geometry when the caller supplies none.
pub const DEFAULT_CHROME_GEOM: crate::types::ChromeGeom =
    crate::types::ChromeGeom { handle_tol: 6.0, knob_tol: 6.0, knob_offset: ROTATE_KNOB_OFFSET };

/// Normalize degrees into `[0, 360)`.
pub fn normalize_deg(d: f64) -> f64 {
    ((d % 360.0) + 360.0) % 360.0
}

/// A geom's applied rotation (degrees), or 0 for the non-rotatable kinds.
pub fn geom_rotation(g: &Geom) -> f64 {
    g.rot().unwrap_or(0.0)
}

fn rect_center(r: Rect) -> Point {
    Point { x: r.x + r.width / 2.0, y: r.y + r.height / 2.0 }
}

/// Rotate one point about a pivot by `deg` (clockwise, content space).
pub fn rotate_point(p: Point, pivot: Point, deg: f64) -> Point {
    apply_point(rotate_about(pivot, deg * DEG2RAD), p)
}

fn quad_points(quads: &[TextQuad]) -> Vec<Point> {
    quads.iter().flat_map(|q| text_quad_points(*q)).collect()
}

/// The rotation pivot of a single shape: a box's centre, a vertex shape's
/// point mean.
pub fn centroid_of(g: &Geom) -> Point {
    let pts: Vec<Point> = match g {
        Geom::Rect { rect, .. } | Geom::Text { rect, .. } | Geom::Caret { rect, .. } => {
            return rect_center(*rect);
        }
        Geom::Line { a, b, .. } => return Point { x: (a.x + b.x) / 2.0, y: (a.y + b.y) / 2.0 },
        Geom::Poly { points, .. } => points.clone(),
        Geom::Ink { strokes, .. } => strokes.concat(),
        Geom::Quads { quads } => quad_points(quads),
    };
    let mut sx = 0.0;
    let mut sy = 0.0;
    for p in &pts {
        sx += p.x;
        sy += p.y;
    }
    let n = if pts.is_empty() { 1.0 } else { pts.len() as f64 };
    Point { x: sx / n, y: sy / n }
}

/// Does the geometry carry a meaningful `rot`?
pub fn is_rotatable_geom(g: &Geom) -> bool {
    match g {
        Geom::Rect { .. }
        | Geom::Line { .. }
        | Geom::Poly { .. }
        | Geom::Ink { .. }
        | Geom::Caret { .. } => true,
        Geom::Text { callout, .. } => callout.is_none(),
        Geom::Quads { .. } => false,
    }
}

/// Rotate a geom by `delta_deg` (clockwise) about `pivot`: a box orbits its
/// centre and adds to `rot`; a vertex shape maps its points and bumps the
/// advisory `rot`. Quads, callouts and carets have no rotate verb.
pub fn geom_rotate_about(g: &Geom, pivot: Point, delta_deg: f64) -> Geom {
    if delta_deg == 0.0 {
        return g.clone();
    }
    let next_rot = Some(normalize_deg(geom_rotation(g) + delta_deg));
    let orbit = |rect: &Rect| {
        let c = rotate_point(rect_center(*rect), pivot, delta_deg);
        Rect { x: c.x - rect.width / 2.0, y: c.y - rect.height / 2.0, ..*rect }
    };
    let rp = |p: &Point| rotate_point(*p, pivot, delta_deg);
    match g {
        Geom::Rect { rect, ellipse, .. } => {
            Geom::Rect { rect: orbit(rect), ellipse: *ellipse, rot: next_rot }
        }
        Geom::Text { callout: Some(_), .. } => g.clone(),
        Geom::Text { rect, callout: None, .. } => {
            Geom::Text { rect: orbit(rect), callout: None, rot: next_rot }
        }
        Geom::Line { a, b, ends, .. } => {
            Geom::Line { a: rp(a), b: rp(b), ends: *ends, rot: next_rot }
        }
        Geom::Poly { points, closed, ends, .. } => Geom::Poly {
            points: points.iter().map(rp).collect(),
            closed: *closed,
            ends: *ends,
            rot: next_rot,
        },
        Geom::Ink { strokes, .. } => Geom::Ink {
            strokes: strokes.iter().map(|s| s.iter().map(rp).collect()).collect(),
            rot: next_rot,
        },
        Geom::Quads { .. } | Geom::Caret { .. } => g.clone(),
    }
}

/// The size of the frame an engine-baked `/AP` is authored into.
fn ap_frame_size(g: &Geom) -> Size {
    if let Some(r) = g.rect() {
        return Size { width: r.width, height: r.height };
    }
    let pts: Vec<Point> = match g {
        Geom::Line { a, b, .. } => vec![*a, *b],
        Geom::Poly { points, .. } => points.clone(),
        Geom::Ink { strokes, .. } => strokes.concat(),
        Geom::Quads { quads } => quad_points(quads),
        _ => Vec::new(),
    };
    let b = union_rect(&pts);
    Size { width: b.width, height: b.height }
}

/// Did an edit change the size of the `/AP` authoring frame (0.01 pt
/// tolerance)?
pub fn ap_size_changed(before: &Geom, after: &Geom) -> bool {
    let a = ap_frame_size(before);
    let b = ap_frame_size(after);
    (a.width - b.width).abs() > 0.01 || (a.height - b.height).abs() > 0.01
}

/// The AABB of `rect` rotated `deg` about its own centre.
pub fn rotated_aabb(rect: Rect, deg: f64) -> Rect {
    if deg == 0.0 || deg.is_nan() {
        return rect;
    }
    let c = rect_center(rect);
    union_rect(&rect_corner_points(rect).map(|p| rotate_point(p, c, deg)))
}

/// The counter-rotation that makes an annotation read upright.
pub fn upright_rotation(display_rotation: f64) -> f64 {
    normalize_deg(-display_rotation)
}

/// `rect` with width and height swapped about its own centre.
pub fn transposed_about_center(rect: Rect) -> Rect {
    let c = rect_center(rect);
    Rect {
        x: c.x - rect.height / 2.0,
        y: c.y - rect.width / 2.0,
        width: rect.height,
        height: rect.width,
    }
}

/// The unrotated rect for a default-size upright box whose displayed
/// top-left is `anchor`.
pub fn upright_anchored_rect(
    anchor: Point,
    width: f64,
    height: f64,
    display_rotation: f64,
) -> Rect {
    let r = normalize_deg(display_rotation);
    let c = if r == 90.0 {
        Point { x: anchor.x + height / 2.0, y: anchor.y - width / 2.0 }
    } else if r == 180.0 {
        Point { x: anchor.x - width / 2.0, y: anchor.y - height / 2.0 }
    } else if r == 270.0 {
        Point { x: anchor.x - height / 2.0, y: anchor.y + width / 2.0 }
    } else {
        Point { x: anchor.x + width / 2.0, y: anchor.y + height / 2.0 }
    };
    Rect { x: c.x - width / 2.0, y: c.y - height / 2.0, width, height }
}

fn clamp_scalar(v: f64, lo: f64, hi: f64) -> f64 {
    js::max(lo, js::min(hi, v))
}

/// The logical box for a stamp placed at `center`, sized `desired`, fit onto
/// the page (scaled down, never up) and clamped within it.
pub fn fit_stamp_box(center: Point, desired: Size, page: Size, rot_cw: f64) -> Rect {
    let quarter = is_quarter_turn(PageRotation(normalize_deg(rot_cw)));
    let fw0 = if quarter { desired.height } else { desired.width };
    let fh0 = if quarter { desired.width } else { desired.height };
    let s = js::min_of(&[
        1.0,
        if fw0 > 0.0 { page.width / fw0 } else { 1.0 },
        if fh0 > 0.0 { page.height / fh0 } else { 1.0 },
    ]);
    let width = desired.width * s;
    let height = desired.height * s;
    let fw = if quarter { height } else { width };
    let fh = if quarter { width } else { height };
    let cx = clamp_scalar(center.x, fw / 2.0, page.width - fw / 2.0);
    let cy = clamp_scalar(center.y, fh / 2.0, page.height - fh / 2.0);
    Rect { x: cx - width / 2.0, y: cy - height / 2.0, width, height }
}

/// Reset a geom to its as-authored orientation (`rot` to 0). A vertex geom
/// spins back about `pivot`, or its centroid without one.
pub fn geom_reset_rotation(g: &Geom, pivot: Option<Point>) -> Geom {
    let rot = geom_rotation(g);
    if rot == 0.0 || rot.is_nan() {
        return g.clone();
    }
    match g {
        Geom::Rect { rect, ellipse, .. } => {
            Geom::Rect { rect: *rect, ellipse: *ellipse, rot: Some(0.0) }
        }
        Geom::Text { rect, callout, .. } => {
            Geom::Text { rect: *rect, callout: *callout, rot: Some(0.0) }
        }
        Geom::Caret { rect, .. } => Geom::Caret { rect: *rect, rot: Some(0.0) },
        _ => geom_rotate_about(g, pivot.unwrap_or_else(|| centroid_of(g)), -rot),
    }
}

/// An oriented box: four corners (nw, ne, se, sw of the local box) and the
/// angle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Obb {
    pub corners: Quad,
    pub angle: f64,
}

/// The oriented selection box of a rotatable geom; `None` for the others.
pub fn obb_from_geom(g: &Geom, stroke_width: f64, border: Option<&Border>) -> Option<Obb> {
    if !is_rotatable_geom(g) {
        return None;
    }
    let rot = geom_rotation(g);
    if let Some(rect) = g.rect() {
        let c = rect_center(*rect);
        return Some(Obb {
            corners: rect_corner_points(*rect).map(|p| rotate_point(p, c, rot)),
            angle: rot,
        });
    }
    let c = centroid_of(g);
    let unrotated =
        if rot == 0.0 || rot.is_nan() { g.clone() } else { geom_rotate_about(g, c, -rot) };
    let local_box = selection_bounds(&unrotated, stroke_width, border);
    Some(Obb {
        corners: rect_corner_points(local_box).map(|p| rotate_point(p, c, rot)),
        angle: rot,
    })
}

// ── group scaling ───────────────────────────────────────────────────────────

/// The 8 box resize handles of a plain rect.
pub fn rect_handles_for(r: Rect) -> Vec<Handle> {
    RECT_HANDLES
        .iter()
        .map(|h| Handle {
            id: (*h).into(),
            at: rect_handle_point(r, h),
            cursor: rect_cursor(h).into(),
        })
        .collect()
}

/// The fixed point of a group resize: the opposite handle's point. An
/// unknown handle falls back to `nw`; a name `Object.prototype` answers
/// (`toString`) finds a function there, which names no edge: the centre.
pub fn group_resize_anchor(base: Rect, handle: &str) -> Point {
    match opposite_handle(handle) {
        Some(o) => rect_handle_point(base, o),
        None if js::is_prototype_key(handle) => rect_handle_point(base, ""),
        None => rect_handle_point(base, "nw"),
    }
}

/// The live group resize box for a drag: the plain axis-aligned resize, or
/// (isotropic) a uniform scale about the anchor by the larger drag ratio.
pub fn group_resize_box(base: Rect, handle: &str, to: Point, isotropic: bool) -> Rect {
    let raw = resize_rect(base, handle, to);
    if !isotropic {
        return raw;
    }
    let sx = if base.width > 0.0 { raw.width / base.width } else { 1.0 };
    let sy = if base.height > 0.0 { raw.height / base.height } else { 1.0 };
    let s = js::max(MIN_SIZE / js::max_of(&[base.width, base.height, 1.0]), js::max(sx, sy));
    let anchor = group_resize_anchor(base, handle);
    Rect {
        x: anchor.x + (base.x - anchor.x) * s,
        y: anchor.y + (base.y - anchor.y) * s,
        width: base.width * s,
        height: base.height * s,
    }
}

/// The `(sx, sy)` a `base` to `cur` group resize applied.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScaleFactors {
    pub sx: f64,
    pub sy: f64,
}

pub fn group_resize_factors(base: Rect, cur: Rect) -> ScaleFactors {
    ScaleFactors {
        sx: if base.width > 0.0 { cur.width / base.width } else { 1.0 },
        sy: if base.height > 0.0 { cur.height / base.height } else { 1.0 },
    }
}

/// Scale a geom about `anchor` by `(sx, sy)`.
pub fn geom_scale_about(g: &Geom, anchor: Point, sx: f64, sy: f64) -> Geom {
    let sp = |p: &Point| Point {
        x: anchor.x + (p.x - anchor.x) * sx,
        y: anchor.y + (p.y - anchor.y) * sy,
    };
    let scale_box = |rect: &Rect| {
        let c = sp(&rect_center(*rect));
        let w = js::max(MIN_SIZE, rect.width * sx.abs());
        let h = js::max(MIN_SIZE, rect.height * sy.abs());
        Rect { x: c.x - w / 2.0, y: c.y - h / 2.0, width: w, height: h }
    };
    match g {
        Geom::Text { callout: Some(_), .. } => g.clone(),
        Geom::Rect { rect, ellipse, rot } => {
            Geom::Rect { rect: scale_box(rect), ellipse: *ellipse, rot: *rot }
        }
        Geom::Text { rect, callout, rot } => {
            Geom::Text { rect: scale_box(rect), callout: *callout, rot: *rot }
        }
        Geom::Line { a, b, ends, rot } => Geom::Line { a: sp(a), b: sp(b), ends: *ends, rot: *rot },
        Geom::Poly { points, closed, ends, rot } => Geom::Poly {
            points: points.iter().map(sp).collect(),
            closed: *closed,
            ends: *ends,
            rot: *rot,
        },
        Geom::Ink { strokes, rot } => Geom::Ink {
            strokes: strokes.iter().map(|s| s.iter().map(sp).collect()).collect(),
            rot: *rot,
        },
        Geom::Caret { rect, rot } => Geom::Caret { rect: scale_box(rect), rot: *rot },
        Geom::Quads { .. } => g.clone(),
    }
}

// ── the rotate knob and guide chords ────────────────────────────────────────

/// Where the rotate knob sits: `at` the grab dot, `from` the edge anchor.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Knob {
    pub at: Point,
    pub from: Point,
}

/// `Math.hypot(x, y) || 1`.
fn len_or_one(x: f64, y: f64) -> f64 {
    let len = js::hypot(x, y);
    if len == 0.0 || len.is_nan() { 1.0 } else { len }
}

/// The knob off the top edge of the OBB corners (nw, ne, se, sw).
pub fn rotate_knob(corners: &Quad, offset: f64) -> Knob {
    let [nw, ne, _, sw] = *corners;
    let from = Point { x: (nw.x + ne.x) / 2.0, y: (nw.y + ne.y) / 2.0 };
    let down = Point { x: sw.x - nw.x, y: sw.y - nw.y };
    let len = len_or_one(down.x, down.y);
    let up = Point { x: -down.x / len, y: -down.y / len };
    Knob { at: Point { x: from.x + up.x * offset, y: from.y + up.y * offset }, from }
}

fn inside_rect(r: Rect, p: Point) -> bool {
    p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height
}

/// A segment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chord {
    pub a: Point,
    pub b: Point,
}

/// The chord of `bx` cut by the line through `p` at `angle_deg`, slab
/// clipped; `None` when the line misses the box.
pub fn chord_through(bx: Rect, p: Point, angle_deg: f64) -> Option<Chord> {
    let d = Point { x: (angle_deg * DEG2RAD).cos(), y: (angle_deg * DEG2RAD).sin() };
    let mut t_min = f64::NEG_INFINITY;
    let mut t_max = f64::INFINITY;
    for (dc, pc, lo, hi) in [(d.x, p.x, bx.x, bx.x + bx.width), (d.y, p.y, bx.y, bx.y + bx.height)]
    {
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
        a: Point { x: p.x + d.x * t_min, y: p.y + d.y * t_min },
        b: Point { x: p.x + d.x * t_max, y: p.y + d.y * t_max },
    })
}

/// [`rotate_knob`] kept on the page: the top edge, else flipped to the
/// bottom edge, else the top knob clamped inside.
pub fn place_rotate_knob(corners: &Quad, offset: f64, page_box: Option<Rect>) -> Knob {
    let top = rotate_knob(corners, offset);
    let Some(page_box) = page_box else {
        return top;
    };
    if inside_rect(page_box, top.at) {
        return top;
    }
    let [nw, _, se, sw] = *corners;
    let from = Point { x: (sw.x + se.x) / 2.0, y: (sw.y + se.y) / 2.0 };
    let down = Point { x: sw.x - nw.x, y: sw.y - nw.y };
    let len = len_or_one(down.x, down.y);
    let at = Point { x: from.x + (down.x / len) * offset, y: from.y + (down.y / len) * offset };
    if inside_rect(page_box, at) {
        return Knob { at, from };
    }
    Knob {
        at: Point {
            x: js::min(js::max(top.at.x, page_box.x), page_box.x + page_box.width),
            y: js::min(js::max(top.at.y, page_box.y), page_box.y + page_box.height),
        },
        from: top.from,
    }
}

// ── callout leader ──────────────────────────────────────────────────────────

/// The box-edge midpoint a callout leader connects to: the side `reference`
/// points toward, decided in the box's local frame when it carries `rot`.
pub fn callout_connection(bx: Rect, reference: Point, rot: f64) -> Point {
    if rot != 0.0 && !rot.is_nan() {
        let c = rect_center(bx);
        return rotate_point(callout_connection(bx, rotate_point(reference, c, -rot), 0.0), c, rot);
    }
    let cx = bx.x + bx.width / 2.0;
    let cy = bx.y + bx.height / 2.0;
    let dx = reference.x - cx;
    let dy = reference.y - cy;
    if dx.abs() >= dy.abs() {
        return if dx >= 0.0 {
            Point { x: bx.x + bx.width, y: cy }
        } else {
            Point { x: bx.x, y: cy }
        };
    }
    if dy >= 0.0 { Point { x: cx, y: bx.y + bx.height } } else { Point { x: cx, y: bx.y } }
}

/// The leader polyline `[tip, knee?, conn]` of a text geom; empty without a
/// callout.
pub fn callout_line_points(g: &Geom) -> Vec<Point> {
    let Geom::Text { rect, callout: Some(c), rot } = g else {
        return Vec::new();
    };
    let conn = callout_connection(*rect, c.knee.unwrap_or(c.tip), rot.unwrap_or(0.0));
    match c.knee {
        Some(knee) => vec![c.tip, knee, conn],
        None => vec![c.tip, conn],
    }
}

struct EndingSeg {
    tip: Point,
    angle: f64,
    ending: Option<LineEnding>,
}

fn callout_ending_seg(pts: &[Point], ending: LineEnding) -> Option<EndingSeg> {
    if pts.len() < 2 {
        return None;
    }
    Some(EndingSeg {
        tip: pts[0],
        angle: (pts[0].y - pts[1].y).atan2(pts[0].x - pts[1].x),
        ending: Some(ending),
    })
}

fn inset_rect(r: Rect, pad: f64) -> Rect {
    let width = js::max(0.0, r.width - 2.0 * pad);
    let height = js::max(0.0, r.height - 2.0 * pad);
    Rect { x: r.x + (r.width - width) / 2.0, y: r.y + (r.height - height) / 2.0, width, height }
}

/// The stored rect for a freshly drawn shape: a cloudy border stores the
/// outer box (dragged plus the cloud extent).
pub fn shape_rect_for(dragged: Rect, ellipse: bool, style: &Style) -> Rect {
    match style.border {
        Border::Cloudy { intensity } => {
            expand_rect(dragged, cloudy_border_extent(intensity, style.stroke_width, ellipse))
        }
        _ => dragged,
    }
}

pub fn caret_rect_from_text_end(line_rect: Rect) -> Rect {
    let height = line_rect.height / 2.0;
    let width = height;
    let line_end_x = line_rect.x + line_rect.width;
    Rect { x: line_end_x - width / 2.0, y: line_rect.y + line_rect.height / 2.0, width, height }
}

/// Caret box for a text-edit anchor: half the glyph's ink height, centred on
/// the trailing baseline corner in reading direction.
pub fn caret_rect_from_anchor(anchor: &TextEndAnchor) -> Rect {
    let q = anchor.glyph_quad;
    let ink = js::hypot(q.lower_start.x - q.upper_start.x, q.lower_start.y - q.upper_start.y);
    let size = js::max(ink / 2.0, 1.0);
    let corner = if anchor.advance > 0.0 { q.lower_end } else { q.lower_start };
    Rect { x: corner.x - size / 2.0, y: corner.y - size, width: size, height: size }
}

const CARET_ROT_EPSILON: f64 = 0.05;

/// Caret geometry for a text-edit anchor: the unrotated box plus the text's
/// baseline tilt; no `rot` member when the tilt is within 0.05° of upright.
pub fn caret_geom_from_anchor(anchor: &TextEndAnchor) -> Geom {
    let q = anchor.glyph_quad;
    let ink = js::hypot(q.lower_start.x - q.upper_start.x, q.lower_start.y - q.upper_start.y);
    let size = js::max(ink / 2.0, 1.0);
    let corner = if anchor.advance > 0.0 { q.lower_end } else { q.lower_start };
    let bx = q.lower_end.x - q.lower_start.x;
    let by = q.lower_end.y - q.lower_start.y;
    let rot = if js::hypot(bx, by) > 0.0 {
        normalize_deg((by.atan2(bx) * 180.0) / std::f64::consts::PI)
    } else {
        0.0
    };
    let upright = rot < CARET_ROT_EPSILON || rot > 360.0 - CARET_ROT_EPSILON;
    if upright {
        return Geom::Caret {
            rect: Rect { x: corner.x - size / 2.0, y: corner.y - size, width: size, height: size },
            rot: None,
        };
    }
    let ux = (q.upper_start.x - q.lower_start.x) / ink;
    let uy = (q.upper_start.y - q.lower_start.y) / ink;
    let cx = corner.x + (ux * size) / 2.0;
    let cy = corner.y + (uy * size) / 2.0;
    Geom::Caret {
        rect: Rect { x: cx - size / 2.0, y: cy - size / 2.0, width: size, height: size },
        rot: Some(rot),
    }
}

fn map_text_quad(q: &TextQuad, f: impl Fn(Point) -> Point) -> TextQuad {
    TextQuad {
        upper_start: f(q.upper_start),
        upper_end: f(q.upper_end),
        lower_start: f(q.lower_start),
        lower_end: f(q.lower_end),
    }
}

// ── stroke outline and endings ──────────────────────────────────────────────

/// Miter limit shared by the bounds math and the renderer.
pub const MITER_LIMIT: f64 = 10.0;

/// The outline points of a mitred, butt-capped polyline stroked at
/// `stroke_width`: segment side offsets plus each interior join's outer miter
/// tip while within [`MITER_LIMIT`].
fn stroke_outline_points(pts: &[Point], closed: bool, stroke_width: f64) -> Vec<Point> {
    let h = stroke_width / 2.0;
    let n = pts.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 || h == 0.0 {
        return pts.to_vec();
    }
    let seg_count = if closed { n } else { n - 1 };
    let mut dir = Vec::with_capacity(seg_count);
    let mut nrm = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let len = len_or_one(b.x - a.x, b.y - a.y);
        let ux = (b.x - a.x) / len;
        let uy = (b.y - a.y) / len;
        dir.push(Point { x: ux, y: uy });
        nrm.push(Point { x: -uy, y: ux });
    }
    let mut out = Vec::new();
    for i in 0..seg_count {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let nx = nrm[i].x * h;
        let ny = nrm[i].y * h;
        out.push(Point { x: a.x + nx, y: a.y + ny });
        out.push(Point { x: a.x - nx, y: a.y - ny });
        out.push(Point { x: b.x + nx, y: b.y + ny });
        out.push(Point { x: b.x - nx, y: b.y - ny });
    }
    let join_start = if closed { 0 } else { 1 };
    let join_end = if closed { n } else { n - 1 };
    #[allow(clippy::needless_range_loop)] // `v` indexes three arrays, as in the TypeScript
    for v in join_start..join_end {
        let in_idx = if closed { (v + n - 1) % n } else { v - 1 };
        let out_idx = v;
        let a = dir[in_idx];
        let b = dir[out_idx];
        let n1 = nrm[in_idx];
        let n2 = nrm[out_idx];
        let bx = a.x - b.x;
        let by = a.y - b.y;
        if js::hypot(bx, by) < 1e-9 {
            continue;
        }
        let mut mhx = n1.x + n2.x;
        let mut mhy = n1.y + n2.y;
        let ml = js::hypot(mhx, mhy);
        if ml < 1e-9 {
            continue;
        }
        mhx /= ml;
        mhy /= ml;
        if mhx * bx + mhy * by < 0.0 {
            mhx = -mhx;
            mhy = -mhy;
        }
        let cos_half = (mhx * n1.x + mhy * n1.y).abs();
        if cos_half < 1e-9 {
            continue;
        }
        let miter_len = h / cos_half;
        if miter_len > MITER_LIMIT * h {
            continue;
        }
        let vv = pts[v];
        out.push(Point { x: vv.x + mhx * miter_len, y: vv.y + mhy * miter_len });
    }
    out
}

/// The start/end tips of a line or open poly with their outward angles.
fn ending_segs(g: &Geom) -> Vec<EndingSeg> {
    match g {
        Geom::Line { a, b, ends: Some(e), .. } => vec![
            EndingSeg { tip: *a, angle: (a.y - b.y).atan2(a.x - b.x), ending: Some(e.start) },
            EndingSeg { tip: *b, angle: (b.y - a.y).atan2(b.x - a.x), ending: Some(e.end) },
        ],
        Geom::Poly { points: p, closed: false, ends: Some(e), .. } if p.len() >= 2 => {
            let n = p.len();
            vec![
                EndingSeg {
                    tip: p[0],
                    angle: (p[0].y - p[1].y).atan2(p[0].x - p[1].x),
                    ending: Some(e.start),
                },
                EndingSeg {
                    tip: p[n - 1],
                    angle: (p[n - 1].y - p[n - 2].y).atan2(p[n - 1].x - p[n - 2].x),
                    ending: Some(e.end),
                },
            ]
        }
        _ => Vec::new(),
    }
}

/// A geom's visual bounds: what the drawn appearance occupies, the rect the
/// engine `/Rect` is emitted from.
pub fn geom_visual_bounds(g: &Geom, stroke_width: f64, border: Option<&Border>) -> Rect {
    if let (Geom::Poly { points, closed: true, .. }, Some(Border::Cloudy { intensity })) =
        (g, border)
    {
        return expand_rect(
            union_rect(points),
            cloudy_border_extent(*intensity, stroke_width, false),
        );
    }
    if let Geom::Text { rect, callout: Some(c), rot } = g {
        let pts = callout_line_points(g);
        let seg = callout_ending_seg(&pts, c.ending);
        let rot = rot.unwrap_or(0.0);
        let center = rect_center(*rect);
        let corners = rect_corner_points(*rect)
            .map(|p| if rot != 0.0 && !rot.is_nan() { rotate_point(p, center, rot) } else { p });
        let mut all: Vec<Point> = corners.to_vec();
        all.extend(pts);
        if let Some(seg) = seg {
            all.extend(ending_points(seg.tip, seg.angle, seg.ending, stroke_width));
        }
        return expand_rect(union_rect(&all), stroke_width / 2.0);
    }
    match g {
        Geom::Rect { rect, .. } | Geom::Text { rect, .. } | Geom::Caret { rect, .. } => {
            return *rect;
        }
        Geom::Quads { quads } => {
            return expand_rect(union_rect(&quad_points(quads)), stroke_width / 2.0);
        }
        Geom::Ink { strokes, .. } => {
            return expand_rect(union_rect(&strokes.concat()), stroke_width / 2.0);
        }
        _ => {}
    }
    let (raw, closed): (Vec<Point>, bool) = match g {
        Geom::Line { a, b, .. } => (vec![*a, *b], false),
        Geom::Poly { points, closed, .. } => (points.clone(), *closed),
        _ => unreachable!("boxes, quads and ink returned above"),
    };
    let mut pts = stroke_outline_points(&raw, closed, stroke_width);
    for seg in ending_segs(g) {
        for node in ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width) {
            match node {
                RenderNode::Poly { points, closed } => {
                    pts.extend(stroke_outline_points(&points, closed, stroke_width))
                }
                RenderNode::Ellipse { rect } => {
                    pts.extend(rect_corner_points(expand_rect(rect, stroke_width / 2.0)))
                }
                _ => {}
            }
        }
    }
    union_rect(&pts)
}

/// The rect the selection wraps: visual bounds for centre-line kinds, the
/// rotated footprint of a tilted callout box, the box otherwise.
pub fn selection_bounds(g: &Geom, stroke_width: f64, border: Option<&Border>) -> Rect {
    match g {
        Geom::Line { .. } | Geom::Ink { .. } | Geom::Poly { .. } => {
            geom_visual_bounds(g, stroke_width, border)
        }
        Geom::Text { rect, callout: Some(_), rot: Some(rot) } if *rot != 0.0 => {
            rotated_aabb(*rect, *rot)
        }
        _ => geom_bounds(g),
    }
}

/// The four corners of the oriented selection box.
pub fn selection_quad(g: &Geom, stroke_width: f64, border: Option<&Border>) -> Quad {
    match obb_from_geom(g, stroke_width, border) {
        Some(obb) => obb.corners,
        None => rect_corner_points(selection_bounds(g, stroke_width, border)),
    }
}

/// Is the point inside the selection quad (even-odd)?
pub fn point_in_quad(p: Point, quad: &Quad) -> bool {
    point_in_poly(p, quad)
}

/// Does the (convex) quad intersect an axis-aligned rect? Separating axes:
/// the rect's two plus the quad's two edge normals. Touching counts.
pub fn quad_intersects_rect(quad: &Quad, r: Rect) -> bool {
    let rect_pts = rect_corner_points(r);
    let axes = [
        Point { x: 1.0, y: 0.0 },
        Point { x: 0.0, y: 1.0 },
        Point { x: -(quad[1].y - quad[0].y), y: quad[1].x - quad[0].x },
        Point { x: -(quad[3].y - quad[0].y), y: quad[3].x - quad[0].x },
    ];
    for ax in axes {
        if ax.x.abs() < 1e-12 && ax.y.abs() < 1e-12 {
            continue;
        }
        let mut q_lo = f64::INFINITY;
        let mut q_hi = f64::NEG_INFINITY;
        for p in quad {
            let d = p.x * ax.x + p.y * ax.y;
            q_lo = js::min(q_lo, d);
            q_hi = js::max(q_hi, d);
        }
        let mut r_lo = f64::INFINITY;
        let mut r_hi = f64::NEG_INFINITY;
        for p in &rect_pts {
            let d = p.x * ax.x + p.y * ax.y;
            r_lo = js::min(r_lo, d);
            r_hi = js::max(r_hi, d);
        }
        if q_hi < r_lo || r_hi < q_lo {
            return false;
        }
    }
    true
}

/// The centre of the oriented selection box (no border).
pub fn selection_center(g: &Geom, stroke_width: f64) -> Point {
    let q = selection_quad(g, stroke_width, None);
    Point {
        x: (q[0].x + q[1].x + q[2].x + q[3].x) / 4.0,
        y: (q[0].y + q[1].y + q[2].y + q[3].y) / 4.0,
    }
}

fn ending_nodes_hit(nodes: &[RenderNode], p: Point, tol: f64) -> bool {
    for node in nodes {
        match node {
            RenderNode::Ellipse { rect: r } => {
                let rx = r.width / 2.0;
                let ry = r.height / 2.0;
                if rx <= 0.0 || ry <= 0.0 {
                    continue;
                }
                let nx = (p.x - (r.x + rx)) / rx;
                let ny = (p.y - (r.y + ry)) / ry;
                if js::hypot(nx, ny) <= 1.0 + tol / js::min(rx, ry) {
                    return true;
                }
            }
            RenderNode::Poly { points: pts, closed } => {
                for i in 0..pts.len().saturating_sub(1) {
                    if seg_dist(p, pts[i], pts[i + 1]) <= tol {
                        return true;
                    }
                }
                if *closed {
                    if pts.len() > 2 && seg_dist(p, pts[pts.len() - 1], pts[0]) <= tol {
                        return true;
                    }
                    if point_in_poly(p, pts) {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn ending_hit(g: &Geom, p: Point, tol: f64, stroke_width: f64) -> bool {
    ending_segs(g).iter().any(|seg| {
        ending_nodes_hit(&ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width), p, tol)
    })
}

// ── geom ops ────────────────────────────────────────────────────────────────

pub fn geom_bounds(g: &Geom) -> Rect {
    match g {
        Geom::Rect { rect, .. } | Geom::Text { rect, .. } | Geom::Caret { rect, .. } => *rect,
        Geom::Line { a, b, .. } => rect_from_points(*a, *b),
        Geom::Poly { points, .. } => union_rect(points),
        Geom::Ink { strokes, .. } => union_rect(&strokes.concat()),
        Geom::Quads { quads } => union_rect(&quad_points(quads)),
    }
}

/// Is the content point on the annotation: within `margin` of the stroke, or
/// inside the fill when `filled`.
pub fn geom_hit(g: &Geom, p: Point, margin: f64, filled: bool, stroke_width: f64) -> bool {
    let tol = margin + stroke_width / 2.0;
    let mut p = p;
    if let Geom::Rect { rect, rot, .. }
    | Geom::Caret { rect, rot }
    | Geom::Text { rect, callout: None, rot } = g
    {
        let rot = rot.unwrap_or(0.0);
        if rot != 0.0 {
            p = rotate_point(p, rect_center(*rect), -rot);
        }
    }
    match g {
        Geom::Caret { rect, .. } => rect_contains(expand_rect(*rect, margin), p),
        Geom::Text { rect, callout, rot } => {
            let rot = if callout.is_some() { rot.unwrap_or(0.0) } else { 0.0 };
            let p_box = if rot != 0.0 && !rot.is_nan() {
                rotate_point(p, rect_center(*rect), -rot)
            } else {
                p
            };
            if rect_contains(expand_rect(*rect, margin), p_box) {
                return true;
            }
            if let Some(c) = callout {
                let pts = callout_line_points(g);
                for i in 0..pts.len().saturating_sub(1) {
                    if seg_dist(p, pts[i], pts[i + 1]) <= tol {
                        return true;
                    }
                }
                if let Some(seg) = callout_ending_seg(&pts, c.ending)
                    && ending_nodes_hit(
                        &ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width),
                        p,
                        tol,
                    )
                {
                    return true;
                }
            }
            false
        }
        Geom::Rect { rect: r, ellipse: true, .. } => {
            let cx = r.x + r.width / 2.0;
            let cy = r.y + r.height / 2.0;
            let orx = r.width / 2.0;
            let ory = r.height / 2.0;
            if orx <= 0.0 || ory <= 0.0 {
                return false;
            }
            if filled && js::hypot((p.x - cx) / orx, (p.y - cy) / ory) <= 1.0 {
                return true;
            }
            let rx = js::max(0.01, orx - stroke_width / 2.0);
            let ry = js::max(0.01, ory - stroke_width / 2.0);
            let d = js::hypot((p.x - cx) / rx, (p.y - cy) / ry);
            let band = tol / js::min(rx, ry);
            (d - 1.0).abs() <= band
        }
        Geom::Rect { rect: r, .. } => {
            if filled && rect_contains(*r, p) {
                return true;
            }
            let c = inset_rect(*r, stroke_width / 2.0);
            let tl = Point { x: c.x, y: c.y };
            let tr = Point { x: c.x + c.width, y: c.y };
            let br = Point { x: c.x + c.width, y: c.y + c.height };
            let bl = Point { x: c.x, y: c.y + c.height };
            [(tl, tr), (tr, br), (br, bl), (bl, tl)].iter().any(|(a, b)| seg_dist(p, *a, *b) <= tol)
        }
        Geom::Line { a, b, .. } => {
            seg_dist(p, *a, *b) <= tol || ending_hit(g, p, tol, stroke_width)
        }
        Geom::Poly { points: pts, closed, .. } => {
            if filled && *closed && point_in_poly(p, pts) {
                return true;
            }
            let n = pts.len();
            for i in 0..n.saturating_sub(1) {
                if seg_dist(p, pts[i], pts[i + 1]) <= tol {
                    return true;
                }
            }
            if *closed && n > 2 && seg_dist(p, pts[n - 1], pts[0]) <= tol {
                return true;
            }
            ending_hit(g, p, tol, stroke_width)
        }
        Geom::Ink { strokes, .. } => strokes.iter().any(|stroke| {
            (0..stroke.len().saturating_sub(1))
                .any(|i| seg_dist(p, stroke[i], stroke[i + 1]) <= tol)
        }),
        Geom::Quads { quads } => quads.iter().any(|q| point_in_quad(p, &text_quad_ring(*q))),
    }
}

/// A geom's drag handles: 8 box handles (on the tilted box, cursors turned
/// with it) plus a callout's tip and knee; line and poly vertices.
pub fn geom_handles(g: &Geom) -> Vec<Handle> {
    match g {
        Geom::Rect { rect, rot, .. } | Geom::Text { rect, rot, .. } => {
            let rot = rot.unwrap_or(0.0);
            let c = rect_center(*rect);
            let mut handles: Vec<Handle> = RECT_HANDLES
                .iter()
                .map(|h| {
                    let at = rect_handle_point(*rect, h);
                    Handle {
                        id: (*h).into(),
                        at: if rot != 0.0 && !rot.is_nan() { rotate_point(at, c, rot) } else { at },
                        cursor: rotated_handle_cursor(h, rot).unwrap_or_default().into(),
                    }
                })
                .collect();
            if let Geom::Text { callout: Some(callout), .. } = g {
                handles.push(Handle {
                    id: "callout-tip".into(),
                    at: callout.tip,
                    cursor: "crosshair".into(),
                });
                if let Some(knee) = callout.knee {
                    handles.push(Handle {
                        id: "callout-knee".into(),
                        at: knee,
                        cursor: "crosshair".into(),
                    });
                }
            }
            handles
        }
        Geom::Line { a, b, .. } => vec![
            Handle { id: "v0".into(), at: *a, cursor: "crosshair".into() },
            Handle { id: "v1".into(), at: *b, cursor: "crosshair".into() },
        ],
        Geom::Poly { points, .. } => points
            .iter()
            .enumerate()
            .map(|(i, at)| Handle { id: format!("v{i}"), at: *at, cursor: "crosshair".into() })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn geom_translate(g: &Geom, d: Point) -> Geom {
    let mv = |p: &Point| Point { x: p.x + d.x, y: p.y + d.y };
    let mv_rect = |r: &Rect| Rect { x: r.x + d.x, y: r.y + d.y, ..*r };
    match g {
        Geom::Text { rect, callout, rot } => Geom::Text {
            rect: mv_rect(rect),
            callout: callout.map(|c| Callout {
                tip: mv(&c.tip),
                knee: c.knee.as_ref().map(mv),
                ending: c.ending,
            }),
            rot: *rot,
        },
        Geom::Rect { rect, ellipse, rot } => {
            Geom::Rect { rect: mv_rect(rect), ellipse: *ellipse, rot: *rot }
        }
        Geom::Caret { rect, rot } => Geom::Caret { rect: mv_rect(rect), rot: *rot },
        Geom::Line { a, b, ends, rot } => Geom::Line { a: mv(a), b: mv(b), ends: *ends, rot: *rot },
        Geom::Poly { points, closed, ends, rot } => Geom::Poly {
            points: points.iter().map(mv).collect(),
            closed: *closed,
            ends: *ends,
            rot: *rot,
        },
        Geom::Ink { strokes, rot } => Geom::Ink {
            strokes: strokes.iter().map(|s| s.iter().map(mv).collect()).collect(),
            rot: *rot,
        },
        Geom::Quads { quads } => {
            Geom::Quads { quads: quads.iter().map(|q| map_text_quad(q, |p| mv(&p))).collect() }
        }
    }
}

/// Resize a rotated box by `handle`, keeping the opposite handle fixed in
/// world space. An unknown handle's opposite is no edge: the centre.
fn resize_rotated_rect(base: Rect, rot: f64, handle: &str, to: Point) -> Rect {
    if rot == 0.0 || rot.is_nan() {
        return resize_rect(base, handle, to);
    }
    let c0 = rect_center(base);
    let local_to = rotate_point(to, c0, -rot);
    let next = resize_rect(base, handle, local_to);
    let anchor_local = rect_handle_point(base, opposite_handle(handle).unwrap_or(""));
    let anchor_world = rotate_point(anchor_local, c0, rot);
    let next_center_local = rect_center(next);
    let off_x = anchor_local.x - next_center_local.x;
    let off_y = anchor_local.y - next_center_local.y;
    let rad = rot * DEG2RAD;
    let cos = rad.cos();
    let sin = rad.sin();
    let r_off_x = cos * off_x - sin * off_y;
    let r_off_y = sin * off_x + cos * off_y;
    let cx = anchor_world.x - r_off_x;
    let cy = anchor_world.y - r_off_y;
    Rect {
        x: cx - next.width / 2.0,
        y: cy - next.height / 2.0,
        width: next.width,
        height: next.height,
    }
}

/// Drag one handle to `to`. A line takes `v0` as its start and any other
/// handle as its end; a poly vertex is `Number(handle.slice(1))`.
pub fn geom_drag_handle(g: &Geom, handle: &str, to: Point) -> Geom {
    match g {
        Geom::Text { rect, callout, rot } => {
            if let Some(c) = callout {
                if handle == "callout-tip" {
                    return Geom::Text {
                        rect: *rect,
                        callout: Some(Callout { tip: to, ..*c }),
                        rot: *rot,
                    };
                }
                if handle == "callout-knee" {
                    return Geom::Text {
                        rect: *rect,
                        callout: Some(Callout { knee: Some(to), ..*c }),
                        rot: *rot,
                    };
                }
            }
            Geom::Text {
                rect: resize_rotated_rect(*rect, rot.unwrap_or(0.0), handle, to),
                callout: *callout,
                rot: *rot,
            }
        }
        Geom::Rect { rect, ellipse, rot } => Geom::Rect {
            rect: resize_rotated_rect(*rect, rot.unwrap_or(0.0), handle, to),
            ellipse: *ellipse,
            rot: *rot,
        },
        Geom::Line { a, b, ends, rot } => {
            if handle == "v0" {
                Geom::Line { a: to, b: *b, ends: *ends, rot: *rot }
            } else {
                Geom::Line { a: *a, b: to, ends: *ends, rot: *rot }
            }
        }
        Geom::Poly { points, closed, ends, rot } => {
            let i = js::number_after_first_unit(handle);
            if i.fract() != 0.0 || !i.is_finite() || i < 0.0 || i >= points.len() as f64 {
                return g.clone();
            }
            let mut points = points.clone();
            points[i as usize] = to;
            Geom::Poly { points, closed: *closed, ends: *ends, rot: *rot }
        }
        _ => g.clone(),
    }
}

/// The text plate inset of a free-text box: twice the border width, none
/// for a width at or below zero. The plate is the box deflated by this on
/// every side.
pub fn text_plate_inset(stroke_width: f64) -> f64 {
    2.0 * js::max(0.0, stroke_width)
}

/// A callout's leader, its box end pushed under the border by half the
/// stroke, and the ending at its tip.
fn callout_leader_nodes(g: &Geom, ending: LineEnding, stroke_width: f64) -> Vec<RenderNode> {
    let mut pts = callout_line_points(g);
    if stroke_width > 0.0 && pts.len() >= 2 {
        let last = pts[pts.len() - 1];
        let prev = pts[pts.len() - 2];
        let dx = last.x - prev.x;
        let dy = last.y - prev.y;
        let len = js::hypot(dx, dy);
        if len > 0.0 {
            let at = pts.len() - 1;
            pts[at] = Point {
                x: last.x + (dx / len) * (stroke_width / 2.0),
                y: last.y + (dy / len) * (stroke_width / 2.0),
            };
        }
    }
    let seg = callout_ending_seg(&pts, ending);
    let mut nodes = vec![RenderNode::Poly { points: pts, closed: false }];
    if let Some(seg) = seg {
        nodes.extend(ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width));
    }
    nodes
}

/// The geometry-only scene: what a framework draws for a geom, stroke inset
/// into boxes, cloudy borders as paths, endings as nodes.
pub fn geom_scene(g: &Geom, stroke_width: f64, border: Option<&Border>) -> Vec<RenderNode> {
    let n = js::number_to_string;
    match g {
        Geom::Text { rect, callout, rot } => {
            let mut nodes = Vec::new();
            if let Some(c) = callout {
                nodes.extend(callout_leader_nodes(g, c.ending, stroke_width));
            }
            let r = inset_rect(*rect, stroke_width / 2.0);
            let rot = rot.unwrap_or(0.0);
            let center = rect_center(*rect);
            nodes.push(if rot != 0.0 && !rot.is_nan() {
                RenderNode::Poly {
                    points: rect_corner_points(r).map(|p| rotate_point(p, center, rot)).to_vec(),
                    closed: true,
                }
            } else {
                RenderNode::Rect { rect: r }
            });
            nodes
        }
        Geom::Caret { rect: r, .. } => {
            let mid_x = r.x + r.width / 2.0;
            let bottom = r.y + r.height;
            let d = [
                format!("M {} {}", n(r.x), n(bottom)),
                format!(
                    "C {} {} {} {} {} {}",
                    n(r.x + r.width * 0.27),
                    n(bottom),
                    n(mid_x),
                    n(r.y + r.height * 0.56),
                    n(mid_x),
                    n(r.y)
                ),
                format!(
                    "C {} {} {} {} {} {}",
                    n(mid_x),
                    n(r.y + r.height * 0.56),
                    n(r.x + r.width * 0.73),
                    n(bottom),
                    n(r.x + r.width),
                    n(bottom)
                ),
                "Z".to_string(),
            ]
            .join(" ");
            vec![RenderNode::Path { d }]
        }
        Geom::Rect { rect, ellipse, .. } => {
            if let Some(Border::Cloudy { intensity }) = border {
                let inset = cloudy_border_extent(*intensity, stroke_width, *ellipse);
                if rect.width > 2.0 * inset && rect.height > 2.0 * inset {
                    return vec![RenderNode::Path {
                        d: cloudy_path(*rect, *ellipse, *intensity, stroke_width),
                    }];
                }
            }
            let r = inset_rect(*rect, stroke_width / 2.0);
            vec![if *ellipse {
                RenderNode::Ellipse { rect: r }
            } else {
                RenderNode::Rect { rect: r }
            }]
        }
        Geom::Line { a, b, .. } => {
            let mut nodes = vec![RenderNode::Line { a: *a, b: *b }];
            for seg in ending_segs(g) {
                nodes.extend(ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width));
            }
            nodes
        }
        Geom::Poly { points, closed, .. } => {
            if let Some(Border::Cloudy { intensity }) = border
                && *closed
                && points.len() >= 3
            {
                return vec![RenderNode::Path {
                    d: cloudy_poly_path(points, *intensity, stroke_width),
                }];
            }
            let mut nodes = vec![RenderNode::Poly { points: points.clone(), closed: *closed }];
            for seg in ending_segs(g) {
                nodes.extend(ending_nodes(seg.tip, seg.angle, seg.ending, stroke_width));
            }
            nodes
        }
        Geom::Ink { strokes, .. } => {
            strokes.iter().map(|s| RenderNode::Poly { points: s.clone(), closed: false }).collect()
        }
        Geom::Quads { quads } => quads
            .iter()
            .map(|q| RenderNode::Poly { points: text_quad_ring(*q).to_vec(), closed: true })
            .collect(),
    }
}

// ── PDF and content ─────────────────────────────────────────────────────────

fn to_content(crop: PdfRect) -> epdf_geometry::Mat2D {
    pdf_to_content_matrix(crop.left, crop.top)
}

fn to_pdf(crop: PdfRect) -> epdf_geometry::Mat2D {
    // `[1, 0, 0, -1, -left, top]` has determinant -1: never singular.
    invert(to_content(crop)).unwrap_or([f64::NAN; 6])
}

pub fn pdf_to_content_point(p: PdfPoint, crop: PdfRect) -> Point {
    apply_point(to_content(crop), Point { x: p.x, y: p.y })
}

pub fn content_to_pdf_point(p: Point, crop: PdfRect) -> PdfPoint {
    let q = apply_point(to_pdf(crop), p);
    PdfPoint { x: q.x, y: q.y }
}

pub fn pdf_to_content_rect(pdf: PdfRect, crop: PdfRect) -> Rect {
    let corner = Rect {
        x: pdf.left,
        y: pdf.bottom,
        width: pdf.right - pdf.left,
        height: pdf.top - pdf.bottom,
    };
    apply_rect(to_content(crop), corner)
}

pub fn content_to_pdf_rect(r: Rect, crop: PdfRect) -> PdfRect {
    let c = apply_rect(to_pdf(crop), r);
    PdfRect { left: c.x, bottom: c.y, right: c.x + c.width, top: c.y + c.height }
}

/// A geom's visual bounds as a `PdfRect`: the engine's explicit `/Rect`.
pub fn geom_pdf_bounds(
    g: &Geom,
    stroke_width: f64,
    crop: PdfRect,
    border: Option<&Border>,
) -> PdfRect {
    content_to_pdf_rect(geom_visual_bounds(g, stroke_width, border), crop)
}

/// `Cursor` for a handle id on the unrotated box (`RECT_CURSOR`).
pub fn rect_handle_cursor(h: &str) -> Cursor {
    rect_cursor(h).into()
}
