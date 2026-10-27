//! `packages/core/geometry` in Rust: the viewer's pure 2D coordinate
//! primitives.
//!
//! The bottom of the pyramid: stage-core (layout), plugin-stage
//! (`pageToWorld`) and every adapter's hit-testing sit on these. The
//! page-rotation transforms are why the package exists: the same quarter-turn
//! maps page to box in both directions, and that one rule is written once.
//!
//! Names mirror the TypeScript (`boundsOfRects` is [`bounds_of_rects`]).
//! Values serialize to the JSON the TypeScript objects print. Arithmetic that
//! goes through `Math.round`, `Math.min`/`max`, `Math.hypot` or number
//! printing uses [`js`], because Rust's versions differ.
//!
//! Not carried over: the phantom space brands (`PointIn<S>`, `RectIn<S>`,
//! `QuadIn<S>`, `Mat2D<From, To>`). They are type-only in the TypeScript and
//! carry no runtime field; here those are plain [`Point`], [`Rect`], [`Quad`]
//! and [`Mat2D`]. [`Space`] names the spaces.
//!
//! [`pdf`] and [`page_space`] carry the pure part of `engine-core`'s geometry
//! that the viewer side calls: the file's coordinates, quad naming and the
//! one conversion to and from page space.

use std::fmt;

use serde::{Deserialize, Serialize};

pub mod js;
pub mod page_space;
pub mod pdf;

pub use page_space::*;
pub use pdf::*;

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

/// A rectangle in page space: top-left origin, y-down, in points. What every
/// viewer and engine API speaks. Stored in the file is the y-up [`PdfRect`].
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

/// The largest rect of `content`'s proportions inside `box_`, centred in it:
/// CSS `object-fit: contain`. `box_` itself when `content` has no area.
pub fn contained_rect(content: Size, box_: Rect) -> Rect {
    if content.width <= 0.0 || content.height <= 0.0 {
        return box_;
    }
    let scale = js::min(box_.width / content.width, box_.height / content.height);
    let width = content.width * scale;
    let height = content.height * scale;
    Rect {
        x: box_.x + (box_.width - width) / 2.0,
        y: box_.y + (box_.height - height) / 2.0,
        width,
        height,
    }
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

/// The coordinate spaces a viewer touches, all y-down. `Page` is the
/// engine's page space: points from the top-left of the visible page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    Page,
    View,
    Screen,
}

/// A 2D affine transform `[a, b, c, d, e, f]`, the six numbers of CSS
/// `matrix()`: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
pub type Mat2D = [f64; 6];

/// Four corners named in the text's own upright frame, the viewer twin of
/// the engine's `PageQuad`. `upper` is the ascent side, `lower` the baseline
/// side, and `left` to `right` runs along the frame's x axis: text turned
/// 180° has its `upper_left` at the bottom right. In y-down page space an
/// upright quad has `upper_*` at the smaller y.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quad {
    pub upper_left: Point,
    pub upper_right: Point,
    pub lower_left: Point,
    pub lower_right: Point,
}

/// Four points as a polygon ring, for fills and hit tests.
pub type QuadRing = [Point; 4];

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

/// Apply a transform to a quad: each corner moves and keeps its name.
pub fn apply_quad(m: Mat2D, q: Quad) -> Quad {
    Quad {
        upper_left: apply_point(m, q.upper_left),
        upper_right: apply_point(m, q.upper_right),
        lower_left: apply_point(m, q.lower_left),
        lower_right: apply_point(m, q.lower_right),
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

/// Rotate about the origin (radians; clockwise in y-down page space).
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

/// The page's scale plus quarter-turn: a page point to its place in the
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

/// The per-page bridge between page, view and device space. The
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
    /// Page point to this page's pixels: the un-rotated content wrapper's
    /// local px.
    pub fn to_pixels(&self, p: Point) -> Point {
        apply_point(self.content_mat, p)
    }
    /// Inverse of [`to_pixels`](Self::to_pixels): wrapper-local px to page
    /// point.
    pub fn from_pixels(&self, p: Point) -> Point {
        apply_point(self.content_inv, p)
    }
    /// Page point to display-box view px (rotation applied).
    pub fn page_to_view(&self, p: Point) -> Point {
        apply_point(self.view_mat, p)
    }
    /// Page rect to its axis-aligned view-px rect (exact for quarter-turns).
    pub fn page_to_view_rect(&self, r: Rect) -> Rect {
        apply_rect(self.view_mat, r)
    }
    /// Display-box view px to page point: hit-testing.
    pub fn view_to_page(&self, p: Point) -> Point {
        apply_point(self.inv_mat, p)
    }
    /// Display-box view-px rect to its page bounds (exact for quarter-turns).
    pub fn view_to_page_rect(&self, r: Rect) -> Rect {
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

/// Whether two transforms place things on the page the same: the same view
/// scale, zoom and turn, and the same content box. Strict `===` on each, so a
/// NaN member never matches.
pub fn same_page_placement(left: &PageTransform, right: &PageTransform) -> bool {
    left.view_scale == right.view_scale
        && left.zoom == right.zoom
        && left.rotation == right.rotation
        && left.content_width == right.content_width
        && left.content_height == right.content_height
}

// ── quad helpers ────────────────────────────────────────────────────────────

/// Which side edge [`quad_edge`] returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuadSide {
    Left,
    Right,
}

/// An upright quad over a rect (y-down: upper is the smaller y).
pub fn quad_from_rect(r: Rect) -> Quad {
    Quad {
        upper_left: Point { x: r.x, y: r.y },
        upper_right: Point { x: r.x + r.width, y: r.y },
        lower_left: Point { x: r.x, y: r.y + r.height },
        lower_right: Point { x: r.x + r.width, y: r.y + r.height },
    }
}

/// The four corners: upper-left, upper-right, lower-left, lower-right.
pub fn quad_corners(q: Quad) -> [Point; 4] {
    [q.upper_left, q.upper_right, q.lower_left, q.lower_right]
}

/// The corners as a polygon ring: upper-left, upper-right, lower-right,
/// lower-left.
pub fn quad_ring(q: Quad) -> QuadRing {
    [q.upper_left, q.upper_right, q.lower_right, q.lower_left]
}

/// One side edge, ascent corner first: left is `UL -> LL`, right is
/// `UR -> LR`.
pub fn quad_edge(q: Quad, side: QuadSide) -> [Point; 2] {
    match side {
        QuadSide::Left => [q.upper_left, q.lower_left],
        QuadSide::Right => [q.upper_right, q.lower_right],
    }
}

/// Corner-wise `===` equality (NaN is never equal, `-0` equals `0`).
pub fn quad_equals(a: Quad, b: Quad) -> bool {
    let eq = |p: Point, q: Point| p.x == q.x && p.y == q.y;
    eq(a.upper_left, b.upper_left)
        && eq(a.upper_right, b.upper_right)
        && eq(a.lower_left, b.lower_left)
        && eq(a.lower_right, b.lower_right)
}

/// Axis-aligned bounds of a quad.
pub fn quad_bounds(q: Quad) -> Rect {
    let xs = [q.upper_left.x, q.upper_right.x, q.lower_left.x, q.lower_right.x];
    let ys = [q.upper_left.y, q.upper_right.y, q.lower_left.y, q.lower_right.y];
    let min_x = js::min_of(&xs);
    let min_y = js::min_of(&ys);
    Rect { x: min_x, y: min_y, width: js::max_of(&xs) - min_x, height: js::max_of(&ys) - min_y }
}
