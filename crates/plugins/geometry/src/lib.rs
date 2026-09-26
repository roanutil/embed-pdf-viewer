//! `packages/core/geometry` in Rust: the viewer's pure 2D coordinate
//! primitives.
//!
//! The bottom of the pyramid: stage-core (layout), plugin-stage
//! (`pageToWorld`) and every adapter's hit-testing sit on these. The
//! page-rotation transforms are why the package exists: the same quarter-turn
//! maps content to box in both directions, and that one rule is written once.
//!
//! Names mirror the TypeScript (`boundsOfRects` is [`bounds_of_rects`]).
//! Values serialize to the JSON the TypeScript objects print. Arithmetic that
//! goes through `Math.round`, `Math.min`/`max`, `Math.hypot` or number
//! printing uses [`js`], because Rust's versions differ.
//!
//! Not carried over: the phantom space brands (`PointIn<S>`, `RectIn<S>`,
//! `QuadIn<S>`, `TextQuadIn<S>`, `Mat2D<From, To>`). They are type-only in the
//! TypeScript and carry no runtime field; here those are plain [`Point`],
//! [`Rect`], [`Quad`], [`TextQuad`] and [`Mat2D`]. [`Space`] names the spaces.

use std::fmt;

use serde::{Deserialize, Serialize};

pub mod js;

#[cfg(feature = "harness")]
pub mod harness;

// ── points, sizes, rects ────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

/// A rectangle in the viewer's coordinates: top-left origin, y-down,
/// PDF-point scale. Shown on screen is a `Rect`; stored in the file is the
/// engine's y-up `PdfRect` (`{left, bottom, right, top}`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The bounding box of a set of rects, `None` for the empty set.
/// Space-agnostic: any y-down `{x, y, width, height}` rects.
pub fn bounds_of_rects(rects: &[Rect]) -> Option<Rect> {
    if rects.is_empty() {
        return None;
    }
    let mut x1 = f64::INFINITY;
    let mut y1 = f64::INFINITY;
    let mut x2 = f64::NEG_INFINITY;
    let mut y2 = f64::NEG_INFINITY;
    for r in rects {
        x1 = js::min(x1, r.x);
        y1 = js::min(y1, r.y);
        x2 = js::max(x2, r.x + r.width);
        y2 = js::max(y2, r.y + r.height);
    }
    Some(Rect { x: x1, y: y1, width: x2 - x1, height: y2 - y1 })
}

/// Intersection of two y-down rects: zero-sized (origin at the would-be
/// corner) when disjoint, so callers can test `width > 0`.
pub fn intersect_rects(a: Rect, b: Rect) -> Rect {
    let x = js::max(a.x, b.x);
    let y = js::max(a.y, b.y);
    Rect {
        x,
        y,
        width: js::max(0.0, js::min(a.x + a.width, b.x + b.width) - x),
        height: js::max(0.0, js::min(a.y + a.height, b.y + b.height) - y),
    }
}

/// The axis-aligned rect spanned by two corners, in any order: a drag box.
pub fn rect_from_corners(a: Point, b: Point) -> Rect {
    Rect {
        x: js::min(a.x, b.x),
        y: js::min(a.y, b.y),
        width: (b.x - a.x).abs(),
        height: (b.y - a.y).abs(),
    }
}

/// Whether a rect contains a point, edges inclusive. Space-agnostic.
pub fn rect_contains(rect: Rect, point: Point) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

/// Positive-area overlap of two rects: touching edges do not count.
pub fn rects_overlap(a: Rect, b: Rect) -> bool {
    let i = intersect_rects(a, b);
    i.width > 0.0 && i.height > 0.0
}

// ── rotation, frames, device rounding ───────────────────────────────────────

/// Quarter-turn display rotation, degrees clockwise: 0, 90, 180 or 270.
///
/// The TypeScript type is the union `0 | 90 | 180 | 270`, but nothing checks
/// it at runtime, and the functions give every number an answer
/// (`addRotations(-90, 0)` is `-90`, `isQuarterTurn(45)` is false). This keeps
/// the number so those answers are the same; the constants are the four
/// values the type means.
#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageRotation(pub f64);

impl PageRotation {
    pub const DEG_0: PageRotation = PageRotation(0.0);
    pub const DEG_90: PageRotation = PageRotation(90.0);
    pub const DEG_180: PageRotation = PageRotation(180.0);
    pub const DEG_270: PageRotation = PageRotation(270.0);
}

/// Reserved chrome around a page, one thickness per side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PageFrame {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

pub const NO_FRAME: PageFrame = PageFrame { top: 0.0, right: 0.0, bottom: 0.0, left: 0.0 };

/// True for the quarter-turns that swap a page's width and height.
pub fn is_quarter_turn(rotation: PageRotation) -> bool {
    rotation.0 == 90.0 || rotation.0 == 270.0
}

/// Compose two rotations, `(a + b) % 360` with JavaScript's `%` (the sign of
/// the dividend, as Rust's `%` on `f64`).
pub fn add_rotations(a: PageRotation, b: PageRotation) -> PageRotation {
    PageRotation((a.0 + b.0) % 360.0)
}

/// Snap a view-px value to the device pixel grid.
pub fn snap_to_device(value: f64, dpr: f64) -> f64 {
    js::round(value * dpr) / dpr
}

/// A page's on-screen footprint: width and height swapped for quarter-turns.
pub fn display_size(content: Size, rotation: PageRotation) -> Size {
    if is_quarter_turn(rotation) {
        Size { width: content.height, height: content.width }
    } else {
        Size { width: content.width, height: content.height }
    }
}

/// The engine's `kind: 'width'` height rule: `max(1, round(w * h / pw))`.
pub fn device_height_for_width(page_size: Size, device_width: f64) -> f64 {
    js::max(1.0, js::round((device_width * page_size.height) / page_size.width))
}

// ── Mat2D ───────────────────────────────────────────────────────────────────

/// The coordinate spaces a viewer touches. `Pdf` is the engine's user space
/// (y-up); the rest are viewer-local, y-down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    Pdf,
    Content,
    View,
    Screen,
}

/// A 2D affine transform `[a, b, c, d, e, f]`, the six numbers of CSS
/// `matrix()`: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
pub type Mat2D = [f64; 6];

/// Four positional points with no corner semantics: the viewer twin of the
/// engine's `PdfQuad`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quad {
    pub p1: Point,
    pub p2: Point,
    pub p3: Point,
    pub p4: Point,
}

/// `invert` of a matrix whose determinant is exactly zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NonInvertible;

impl fmt::Display for NonInvertible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Mat2D.invert: non-invertible matrix (det = 0)")
    }
}

impl std::error::Error for NonInvertible {}

/// The identity transform.
pub fn identity() -> Mat2D {
    [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
}

/// `compose(m, n)` applies `n` first, then `m` (the product `m · n`).
pub fn compose(m: Mat2D, n: Mat2D) -> Mat2D {
    let [a, b, c, d, e, f] = m;
    let [a2, b2, c2, d2, e2, f2] = n;
    [
        a * a2 + c * b2,
        b * a2 + d * b2,
        a * c2 + c * d2,
        b * c2 + d * d2,
        a * e2 + c * f2 + e,
        b * e2 + d * f2 + f,
    ]
}

/// Invert a transform. Fails only when the determinant is exactly zero; a
/// NaN determinant inverts to NaNs, as in the TypeScript.
pub fn invert(m: Mat2D) -> Result<Mat2D, NonInvertible> {
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    if det == 0.0 {
        return Err(NonInvertible);
    }
    let ia = d / det;
    let ib = -b / det;
    let ic = -c / det;
    let id = a / det;
    Ok([ia, ib, ic, id, -(ia * e + ic * f), -(ib * e + id * f)])
}

/// Apply a transform to a point.
pub fn apply_point(m: Mat2D, p: Point) -> Point {
    let [a, b, c, d, e, f] = m;
    Point { x: a * p.x + c * p.y + e, y: b * p.x + d * p.y + f }
}

/// Apply a transform to a rect: the axis-aligned bounds of the four mapped
/// corners. Exact for translate, scale and quarter-turns; lossy under skew.
pub fn apply_rect(m: Mat2D, r: Rect) -> Rect {
    let c0 = apply_point(m, Point { x: r.x, y: r.y });
    let c1 = apply_point(m, Point { x: r.x + r.width, y: r.y });
    let c2 = apply_point(m, Point { x: r.x, y: r.y + r.height });
    let c3 = apply_point(m, Point { x: r.x + r.width, y: r.y + r.height });
    let xs = [c0.x, c1.x, c2.x, c3.x];
    let ys = [c0.y, c1.y, c2.y, c3.y];
    let min_x = js::min_of(&xs);
    let min_y = js::min_of(&ys);
    Rect { x: min_x, y: min_y, width: js::max_of(&xs) - min_x, height: js::max_of(&ys) - min_y }
}

/// Apply a transform to a quad, point by point.
pub fn apply_quad(m: Mat2D, q: Quad) -> Quad {
    Quad {
        p1: apply_point(m, q.p1),
        p2: apply_point(m, q.p2),
        p3: apply_point(m, q.p3),
        p4: apply_point(m, q.p4),
    }
}

/// The CSS `matrix(a, b, c, d, e, f)` string, numbers printed as JavaScript
/// prints them.
pub fn matrix_to_css(m: Mat2D) -> String {
    let [a, b, c, d, e, f] = m.map(js::number_to_string);
    format!("matrix({a}, {b}, {c}, {d}, {e}, {f})")
}

/// Translate within a space.
pub fn translate(tx: f64, ty: f64) -> Mat2D {
    [1.0, 0.0, 0.0, 1.0, tx, ty]
}

/// Scale about the space origin.
pub fn scale(sx: f64, sy: f64) -> Mat2D {
    [sx, 0.0, 0.0, sy, 0.0, 0.0]
}

/// Rotate about the origin (radians; clockwise in y-down content space).
pub fn rotate(rad: f64) -> Mat2D {
    let c = rad.cos();
    let s = rad.sin();
    [c, s, -s, c, 0.0, 0.0]
}

/// Rotate about a pivot: `T(c) · R(rad) · T(-c)`.
pub fn rotate_about(c: Point, rad: f64) -> Mat2D {
    compose(translate(c.x, c.y), compose(rotate(rad), translate(-c.x, -c.y)))
}

/// Scale about an anchor: `T(a) · S(sx, sy) · T(-a)`.
pub fn scale_about(a: Point, sx: f64, sy: f64) -> Mat2D {
    compose(translate(a.x, a.y), compose(scale(sx, sy), translate(-a.x, -a.y)))
}

/// The rotation angle encoded in a matrix, `atan2(b, a)`.
pub fn angle_of(m: Mat2D) -> f64 {
    m[1].atan2(m[0])
}

/// The page's scale plus quarter-turn: a content point to its place in the
/// rotated display box. `box_w`/`box_h` are the un-rotated extents in output
/// units. Any rotation other than 90, 180 or 270 is treated as 0.
pub fn rotate_scale_matrix(scale: f64, box_w: f64, box_h: f64, rotation: PageRotation) -> Mat2D {
    if rotation.0 == 90.0 {
        [0.0, scale, -scale, 0.0, box_h, 0.0]
    } else if rotation.0 == 180.0 {
        [-scale, 0.0, 0.0, -scale, box_w, box_h]
    } else if rotation.0 == 270.0 {
        [0.0, -scale, scale, 0.0, 0.0, box_w]
    } else {
        [scale, 0.0, 0.0, scale, 0.0, 0.0]
    }
}

// ── page geometry ───────────────────────────────────────────────────────────

/// A crop box in PDF user space (y-up edges), origin preserved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CropBox {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// `PageGeometryInput`: structurally a subset of the engine's `PageLayout`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometryInput {
    pub crop: CropBox,
    /// The document's `/Rotate`.
    pub rotation: PageRotation,
    pub user_unit: f64,
}

/// The matrices bridging one page's spaces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometry {
    pub pdf_to_content: Mat2D,
    pub content_to_view: Mat2D,
    pub pdf_to_view: Mat2D,
    pub view_to_pdf: Mat2D,
}

/// PDF user space (y-up, origin at the crop bottom-left) to content space
/// (y-down, origin at the crop top-left). The TypeScript takes
/// `{left, top}`; those are the two numbers it reads.
pub fn pdf_to_content_matrix(left: f64, top: f64) -> Mat2D {
    [1.0, 0.0, 0.0, -1.0, -left, top]
}

/// A page's space matrices from its crop box, rotation and `userUnit` plus
/// the viewer zoom.
pub fn page_geometry(input: PageGeometryInput, zoom: f64) -> Result<PageGeometry, NonInvertible> {
    let PageGeometryInput { crop, rotation, user_unit } = input;
    let s = zoom * user_unit;
    let pdf_to_content = pdf_to_content_matrix(crop.left, crop.top);
    let wc = (crop.right - crop.left) * s;
    let hc = (crop.top - crop.bottom) * s;
    let content_to_view = rotate_scale_matrix(s, wc, hc, rotation);
    let pdf_to_view = compose(content_to_view, pdf_to_content);
    Ok(PageGeometry {
        pdf_to_content,
        content_to_view,
        pdf_to_view,
        view_to_pdf: invert(pdf_to_view)?,
    })
}

// ── page transform ──────────────────────────────────────────────────────────

/// The argument of [`page_transform`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTransformInput {
    /// Un-rotated page size in PDF points.
    pub page_size: Size,
    pub rotation: PageRotation,
    /// View px per PDF point.
    pub scale: f64,
    /// View px per PDF point at 100% zoom; `None` (absent, `undefined` or
    /// `null`) means `scale`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_scale: Option<f64>,
    /// Device px per view px.
    pub dpr: f64,
}

/// The per-page bridge between content, view and device space. The
/// TypeScript object's closures are methods here, over the matrices built
/// once in [`page_transform`]; it serializes to its data members.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTransform {
    pub rotation: PageRotation,
    pub view_width: f64,
    pub view_height: f64,
    pub content_width: f64,
    pub content_height: f64,
    pub device_width: f64,
    pub device_height: f64,
    pub render_scale: f64,
    pub view_scale: f64,
    pub base_scale: f64,
    pub zoom: f64,
    pub css_matrix: String,
    #[serde(skip)]
    content_mat: Mat2D,
    #[serde(skip)]
    content_inv: Mat2D,
    #[serde(skip)]
    view_mat: Mat2D,
    #[serde(skip)]
    inv_mat: Mat2D,
}

impl PageTransform {
    /// Content point to the un-rotated content wrapper's px.
    pub fn to_pixels(&self, p: Point) -> Point {
        apply_point(self.content_mat, p)
    }
    /// Inverse of [`to_pixels`](Self::to_pixels).
    pub fn from_pixels(&self, p: Point) -> Point {
        apply_point(self.content_inv, p)
    }
    /// Content point to display-box view px (rotation applied).
    pub fn content_to_view(&self, p: Point) -> Point {
        apply_point(self.view_mat, p)
    }
    /// Content rect to its axis-aligned view-px rect.
    pub fn content_to_view_rect(&self, r: Rect) -> Rect {
        apply_rect(self.view_mat, r)
    }
    /// Display-box view px to content point.
    pub fn view_to_content(&self, p: Point) -> Point {
        apply_point(self.inv_mat, p)
    }
    /// Display-box view-px rect to its content bounds.
    pub fn view_to_content_rect(&self, r: Rect) -> Rect {
        apply_rect(self.inv_mat, r)
    }
}

/// Build a page's transform. Device dimensions snap to the grid and follow
/// the engine's width rule. Fails when a matrix is singular (a view scale
/// whose square underflows to zero).
pub fn page_transform(input: PageTransformInput) -> Result<PageTransform, NonInvertible> {
    let PageTransformInput { page_size, rotation, scale, dpr, .. } = input;

    let device_width = js::max(1.0, js::round(page_size.width * scale * dpr));
    let device_height = device_height_for_width(page_size, device_width);
    let render_scale = device_width / page_size.width;

    let content = Size { width: device_width / dpr, height: device_height / dpr };
    let view_scale = content.width / page_size.width;
    let base_scale = input.base_scale.unwrap_or(scale);
    let zoom = if base_scale > 0.0 { view_scale / base_scale } else { 1.0 };

    let footprint = display_size(content, rotation);

    let content_mat = [view_scale, 0.0, 0.0, view_scale, 0.0, 0.0];
    let view_mat = rotate_scale_matrix(view_scale, content.width, content.height, rotation);
    let inv_mat = invert(view_mat)?;
    let content_inv = invert(content_mat)?;

    Ok(PageTransform {
        rotation,
        view_width: footprint.width,
        view_height: footprint.height,
        content_width: content.width,
        content_height: content.height,
        device_width,
        device_height,
        render_scale,
        view_scale,
        base_scale,
        zoom,
        css_matrix: matrix_to_css(view_mat),
        content_mat,
        content_inv,
        view_mat,
        inv_mat,
    })
}

// ── TextQuad ────────────────────────────────────────────────────────────────

/// A quad whose orientation is known, corners named in the text's own
/// upright frame: `upper` is the ascent side, `start` to `end` runs along +x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextQuad {
    pub upper_start: Point,
    pub upper_end: Point,
    pub lower_start: Point,
    pub lower_end: Point,
}

/// Which side edge [`text_quad_edge`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextQuadSide {
    Start,
    End,
}

/// Axis-aligned TextQuad over a rect (y-down: upper is the smaller y).
pub fn text_quad_from_rect(r: Rect) -> TextQuad {
    TextQuad {
        upper_start: Point { x: r.x, y: r.y },
        upper_end: Point { x: r.x + r.width, y: r.y },
        lower_start: Point { x: r.x, y: r.y + r.height },
        lower_end: Point { x: r.x + r.width, y: r.y + r.height },
    }
}

/// The corners in slot order: upper-start, upper-end, lower-start, lower-end.
pub fn text_quad_points(t: TextQuad) -> [Point; 4] {
    [t.upper_start, t.upper_end, t.lower_start, t.lower_end]
}

/// The corners as a polygon ring: US, UE, LE, LS.
pub fn text_quad_ring(t: TextQuad) -> [Point; 4] {
    [t.upper_start, t.upper_end, t.lower_end, t.lower_start]
}

/// One side edge, ascent corner first.
pub fn text_quad_edge(t: TextQuad, side: TextQuadSide) -> [Point; 2] {
    match side {
        TextQuadSide::Start => [t.upper_start, t.lower_start],
        TextQuadSide::End => [t.upper_end, t.lower_end],
    }
}

/// Corner-wise `===` equality (NaN is never equal, `-0` equals `0`).
pub fn text_quad_equals(a: TextQuad, b: TextQuad) -> bool {
    let eq = |p: Point, q: Point| p.x == q.x && p.y == q.y;
    eq(a.upper_start, b.upper_start)
        && eq(a.upper_end, b.upper_end)
        && eq(a.lower_start, b.lower_start)
        && eq(a.lower_end, b.lower_end)
}

/// Axis-aligned bounds of a TextQuad.
pub fn text_quad_bounds(t: TextQuad) -> Rect {
    let xs = [t.upper_start.x, t.upper_end.x, t.lower_start.x, t.lower_end.x];
    let ys = [t.upper_start.y, t.upper_end.y, t.lower_start.y, t.lower_end.y];
    let min_x = js::min_of(&xs);
    let min_y = js::min_of(&ys);
    Rect { x: min_x, y: min_y, width: js::max_of(&xs) - min_x, height: js::max_of(&ys) - min_y }
}

/// Map a TextQuad through a transform; corner names ride along.
pub fn apply_text_quad(m: Mat2D, t: TextQuad) -> TextQuad {
    TextQuad {
        upper_start: apply_point(m, t.upper_start),
        upper_end: apply_point(m, t.upper_end),
        lower_start: apply_point(m, t.lower_start),
        lower_end: apply_point(m, t.lower_end),
    }
}

/// The positional zigzag convention: `p1..p4` are US, UE, LS, LE.
pub fn positional_quad(t: TextQuad) -> Quad {
    Quad { p1: t.upper_start, p2: t.upper_end, p3: t.lower_start, p4: t.lower_end }
}

const QUAD_EPSILON: f64 = 1e-6;

fn finite_point(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Is `[us, ue, ls, le]` a well-formed zigzag reading of a quad?
fn zigzag_well_formed([us, ue, ls, le]: [Point; 4]) -> bool {
    if ![us, ue, ls, le].into_iter().all(finite_point) {
        return false;
    }
    let upper = Point { x: ue.x - us.x, y: ue.y - us.y };
    let lower = Point { x: le.x - ls.x, y: le.y - ls.y };
    let start_side = Point { x: ls.x - us.x, y: ls.y - us.y };
    let end_side = Point { x: le.x - ue.x, y: le.y - ue.y };
    let len_u = js::hypot(upper.x, upper.y);
    let len_l = js::hypot(lower.x, lower.y);
    let len_s = js::hypot(start_side.x, start_side.y);
    let len_e = js::hypot(end_side.x, end_side.y);
    if len_u <= QUAD_EPSILON
        || len_l <= QUAD_EPSILON
        || len_s <= QUAD_EPSILON
        || len_e <= QUAD_EPSILON
    {
        return false;
    }
    if upper.x * lower.x + upper.y * lower.y <= 0.0 {
        return false;
    }
    if start_side.x * end_side.x + start_side.y * end_side.y <= 0.0 {
        return false;
    }
    let start_area = upper.x * start_side.y - upper.y * start_side.x;
    let end_area = lower.x * end_side.y - lower.y * end_side.x;
    if start_area.abs() <= QUAD_EPSILON * len_u * len_s {
        return false;
    }
    if end_area.abs() <= QUAD_EPSILON * len_l * len_e {
        return false;
    }
    // `Math.sign(a) === Math.sign(b)`: NaN never matches.
    js::sign(start_area) == js::sign(end_area)
}

/// Interpret an arbitrary positional quad as a TextQuad: zigzag order passes
/// through, ring order is repaired, and anything else gets a deterministic
/// labeling (corners sorted around their centroid, upper is the edge whose
/// midpoint is highest on screen).
pub fn normalize_quad(q: Quad) -> TextQuad {
    if zigzag_well_formed([q.p1, q.p2, q.p3, q.p4]) {
        return TextQuad { upper_start: q.p1, upper_end: q.p2, lower_start: q.p3, lower_end: q.p4 };
    }
    if zigzag_well_formed([q.p1, q.p2, q.p4, q.p3]) {
        return TextQuad { upper_start: q.p1, upper_end: q.p2, lower_start: q.p4, lower_end: q.p3 };
    }

    let pts = [q.p1, q.p2, q.p3, q.p4].map(|p| if finite_point(p) { p } else { Point::default() });
    let cx = (pts[0].x + pts[1].x + pts[2].x + pts[3].x) / 4.0;
    let cy = (pts[0].y + pts[1].y + pts[2].y + pts[3].y) / 4.0;
    let mut ring = pts;
    // `Array.prototype.sort` is stable, as `sort_by` is; the comparator's
    // number maps to an ordering by sign, NaN as equal.
    ring.sort_by(|a, b| {
        let d = (a.y - cy).atan2(a.x - cx) - (b.y - cy).atan2(b.x - cx);
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut upper_edge = 0;
    let mut best_y = f64::INFINITY;
    for i in 0..4 {
        let mid_y = (ring[i].y + ring[(i + 1) % 4].y) / 2.0;
        if mid_y < best_y {
            best_y = mid_y;
            upper_edge = i;
        }
    }
    let a = ring[upper_edge];
    let b = ring[(upper_edge + 1) % 4];
    let c = ring[(upper_edge + 2) % 4];
    let d = ring[(upper_edge + 3) % 4];
    if a.x <= b.x {
        TextQuad { upper_start: a, upper_end: b, lower_start: d, lower_end: c }
    } else {
        TextQuad { upper_start: b, upper_end: a, lower_start: c, lower_end: d }
    }
}

// ── page space ──────────────────────────────────────────────────────────────

/// A rectangle in PDF user space by its edges: the engine's `PdfRect` shape.
/// The same four numbers as [`CropBox`]; the TypeScript names both
/// `PdfEdges` structurally.
pub type PdfEdges = CropBox;

/// The one owner of page to PDF conversion for one page (`pageSpace` in
/// `page-space.ts`). Page space is content space: the unrotated page, origin
/// at the CropBox top-left, y down.
///
/// ```text
/// pageX = pdfX - crop.left
/// pageY = crop.top - pdfY
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PageSpace {
    pub crop: PdfEdges,
    pub width: f64,
    pub height: f64,
    #[serde(skip)]
    to_page: Mat2D,
    #[serde(skip)]
    to_pdf: Mat2D,
}

/// Build a page's [`PageSpace`] from its CropBox.
pub fn page_space(crop: PdfEdges) -> PageSpace {
    let to_page = pdf_to_content_matrix(crop.left, crop.top);
    // `[1, 0, 0, -1, e, f]` has determinant -1 whatever the offsets, so the
    // inverse always exists (NaN offsets invert to NaNs, as in the TypeScript).
    let to_pdf = invert(to_page).unwrap_or([f64::NAN; 6]);
    PageSpace {
        crop,
        width: crop.right - crop.left,
        height: crop.top - crop.bottom,
        to_page,
        to_pdf,
    }
}

impl PageSpace {
    pub fn pdf_to_page(&self, p: Point) -> Point {
        apply_point(self.to_page, p)
    }

    pub fn page_to_pdf(&self, p: Point) -> Point {
        apply_point(self.to_pdf, p)
    }

    pub fn pdf_rect_to_page(&self, r: PdfEdges) -> Rect {
        Rect {
            x: r.left - self.crop.left,
            y: self.crop.top - r.top,
            width: r.right - r.left,
            height: r.top - r.bottom,
        }
    }

    pub fn page_rect_to_pdf(&self, r: Rect) -> PdfEdges {
        let c = self.crop;
        PdfEdges {
            left: r.x + c.left,
            right: r.x + r.width + c.left,
            top: c.top - r.y,
            bottom: c.top - (r.y + r.height),
        }
    }

    pub fn pdf_quad_to_page(&self, q: Quad) -> Quad {
        apply_quad(self.to_page, q)
    }

    pub fn page_quad_to_pdf(&self, q: Quad) -> Quad {
        apply_quad(self.to_pdf, q)
    }
}

/// Positive-area overlap of two PDF rects given by their edges: the engine's
/// collateral rule (a redaction touches what it overlaps, not what it abuts).
pub fn edges_overlap(a: PdfEdges, b: PdfEdges) -> bool {
    js::min(a.right, b.right) > js::max(a.left, b.left)
        && js::min(a.top, b.top) > js::max(a.bottom, b.bottom)
}

/// The axis-aligned PDF edges enclosing a quad.
pub fn edges_of_quad(q: Quad) -> PdfEdges {
    let xs = [q.p1.x, q.p2.x, q.p3.x, q.p4.x];
    let ys = [q.p1.y, q.p2.y, q.p3.y, q.p4.y];
    PdfEdges {
        left: js::min_of(&xs),
        right: js::max_of(&xs),
        bottom: js::min_of(&ys),
        top: js::max_of(&ys),
    }
}
