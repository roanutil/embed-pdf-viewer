//! `packages/core/stage` in Rust: the pure spatial model.
//!
//! A layout strategy ([`linear_layout`], [`grid_layout`]) turns pages into a
//! [`Scene`] of positioned items with a spatial index; a [`Camera`] is the
//! world point at the viewport origin plus a zoom; an [`Anchor`] is a focal
//! point relative to a page, which survives re-layout and zoom. Navigation is
//! camera math over those.
//!
//! Names mirror the TypeScript (`linearLayout` is [`linear_layout`]). Values
//! serialize to the JSON the TypeScript objects print, except that a
//! `Scene`'s query closures are methods here. `Math.min`/`max` go through
//! [`epdf_geometry::js`], because Rust's ignore NaN.
//!
//! Page counts and page indexes are `usize`. The TypeScript takes any number
//! there; a fractional, negative or NaN index is not ported (see the README).

use std::fmt;

use epdf_geometry::js;
use serde::{Deserialize, Serialize};

pub use epdf_geometry::{NO_FRAME, PageFrame, PageRotation, Point, Rect, Size, display_size};

#[cfg(feature = "harness")]
pub mod harness;

// ── types ───────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub x: f64,
    pub y: f64,
    pub zoom: f64,
}

/// A page as the layout sees it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeom {
    /// Un-rotated (intrinsic) page size.
    pub size: Size,
    /// Total display rotation; layout swaps width and height for 90 and 270.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<PageRotation>,
    /// PDF `/UserUnit`, folded into the layout size and `content_scale`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_unit: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageBox {
    pub page_index: usize,
    pub x: f64,
    pub y: f64,
    /// Display width: the page's height for 90 and 270 rotations.
    pub width: f64,
    pub height: f64,
    pub rotation: PageRotation,
    /// World size over intrinsic size for this page, `userUnit` included.
    pub content_scale: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SceneItem {
    pub index: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub page_indexes: Vec<usize>,
    pub pages: Vec<PageBox>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    X,
    Y,
    Grid,
}

/// Positioned items plus the spatial index the queries run on.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub size: Size,
    pub items: Vec<SceneItem>,
    pub item_count: usize,
    pub axis: Axis,
    /// The largest item width and height; fit modes resolve against it.
    pub max_item_size: Size,
    #[serde(skip)]
    index: SceneIndex,
}

#[derive(Clone, Debug, PartialEq)]
enum SceneIndex {
    Linear {
        vertical: bool,
        mirrored: bool,
        starts: Vec<f64>,
        ends: Vec<f64>,
        first_page: Vec<f64>,
    },
    Grid {
        columns: f64,
        rows: usize,
        step_x: f64,
        rtl: bool,
        row_top: Vec<f64>,
        row_end: Vec<f64>,
        first_page: Vec<f64>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    #[default]
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpreadMode {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SizingMode {
    #[default]
    Intrinsic,
    Uniform,
}

/// Per-axis alignment. Logical on x (`Start` is the reading edge), physical
/// on y.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alignment {
    pub x: Align,
    pub y: Align,
}

/// A named stop, or a viewport fraction for the subject's center.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AlignValue {
    Named(Align),
    Fraction(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AlignmentValue {
    pub x: AlignValue,
    pub y: AlignValue,
}

impl Default for AlignmentValue {
    /// `placeCamera`'s default, `{x: 'start', y: 'start'}`.
    fn default() -> Self {
        AlignmentValue { x: AlignValue::Named(Align::Start), y: AlignValue::Named(Align::Start) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraConstraint {
    pub bounded: bool,
    /// Breathing room (screen px) around the content.
    pub padding: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fit_align: Option<Alignment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZoomMode {
    Automatic,
    FitPage,
    FitWidth,
    /// Fit the whole scene.
    FitAll,
}

/// The zoom intent. The TypeScript tests `level`, then `pageWidth`, then
/// `pageHeight`, then falls to `mode`; the variant order here is the same, so
/// an object carrying two of them decodes as the TypeScript reads it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ZoomSpec {
    Level {
        level: f64,
    },
    PageWidth {
        #[serde(rename = "pageWidth")]
        page_width: f64,
    },
    PageHeight {
        #[serde(rename = "pageHeight")]
        page_height: f64,
    },
    Mode {
        mode: ZoomMode,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub page_index: usize,
    pub fx: f64,
    pub fy: f64,
}

pub const ZOOM_MIN: f64 = 0.05;
pub const ZOOM_MAX: f64 = 64.0;

/// Where the TypeScript throws. Each carries the name and message a
/// JavaScript engine (V8) gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageError {
    /// A member read on `undefined`: a grouping that names a page that does
    /// not exist (`userUnit`), an empty scene (`pages`), an item with no
    /// pages (`pageIndex` or `x`).
    ReadOfUndefined(&'static str),
    /// `new Array(NaN)`: a grid whose column count comes out NaN.
    InvalidArrayLength,
}

impl StageError {
    pub fn name(&self) -> &'static str {
        match self {
            StageError::ReadOfUndefined(_) => "TypeError",
            StageError::InvalidArrayLength => "RangeError",
        }
    }
}

impl fmt::Display for StageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StageError::ReadOfUndefined(member) => {
                write!(f, "Cannot read properties of undefined (reading '{member}')")
            }
            StageError::InvalidArrayLength => f.write_str("Invalid array length"),
        }
    }
}

impl std::error::Error for StageError {}

/// `Math.min(Math.max(v, lo), hi)`.
fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    js::min(js::max(v, lo), hi)
}

/// The same clamp on the integer indexes it bounds.
fn clamp_index(v: isize, lo: isize, hi: isize) -> isize {
    v.max(lo).min(hi)
}

/// The page's display footprint, scaled by its `/UserUnit`.
fn display_dims(pg: &PageGeom) -> Size {
    let u = pg.user_unit.unwrap_or(1.0);
    let d = display_size(pg.size, pg.rotation.unwrap_or(PageRotation::DEG_0));
    if u == 1.0 { d } else { Size { width: d.width * u, height: d.height * u } }
}

// ── projection ──────────────────────────────────────────────────────────────

pub fn to_screen(c: Camera, w: Point) -> Point {
    Point { x: (w.x - c.x) * c.zoom, y: (w.y - c.y) * c.zoom }
}

pub fn to_world(c: Camera, s: Point) -> Point {
    Point { x: c.x + s.x / c.zoom, y: c.y + s.y / c.zoom }
}

pub fn camera_world_rect(c: Camera, vp: Size) -> Rect {
    Rect { x: c.x, y: c.y, width: vp.width / c.zoom, height: vp.height / c.zoom }
}

// ── camera operations ───────────────────────────────────────────────────────

/// Focal zoom: keep the world point under `screen_pt` fixed.
pub fn zoom_around(c: Camera, screen_pt: Point, factor: f64) -> Camera {
    let zoom = clamp(c.zoom * factor, ZOOM_MIN, ZOOM_MAX);
    Camera {
        x: c.x + screen_pt.x / c.zoom - screen_pt.x / zoom,
        y: c.y + screen_pt.y / c.zoom - screen_pt.y / zoom,
        zoom,
    }
}

pub fn pan_by_screen(c: Camera, dx_screen: f64, dy_screen: f64) -> Camera {
    Camera { x: c.x - dx_screen / c.zoom, y: c.y - dy_screen / c.zoom, zoom: c.zoom }
}

pub fn center_on_world(world_pt: Point, vp: Size, zoom: f64) -> Camera {
    Camera { zoom, x: world_pt.x - vp.width / 2.0 / zoom, y: world_pt.y - vp.height / 2.0 / zoom }
}

/// One axis of camera travel against content bounds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxisTravel {
    pub near: f64,
    pub far: f64,
    pub fits: bool,
}

pub fn travel_range(origin: f64, content: f64, view: f64, zoom: f64, padding: f64) -> AxisTravel {
    AxisTravel {
        near: origin - padding / zoom,
        far: origin + content - (view - padding) / zoom,
        fits: content * zoom <= view - 2.0 * padding,
    }
}

/// Clamp the camera into `bounds`: free travel where the padded content
/// overflows, locked to the `fit_align` rest where it fits.
pub fn clamp_camera(c: Camera, bounds: Rect, vp: Size, k: CameraConstraint) -> Camera {
    if !k.bounded {
        return c;
    }
    let p = k.padding;
    let fit = k.fit_align.unwrap_or(Alignment { x: Align::Center, y: Align::Center });
    let axis = |pos: f64, origin: f64, content: f64, view: f64, a: Align| -> f64 {
        let t = travel_range(origin, content, view, c.zoom, p);
        if t.fits {
            return match a {
                Align::Start => t.near,
                Align::End => t.far,
                Align::Center => (t.near + t.far) / 2.0,
            };
        }
        clamp(pos, t.near, t.far)
    };
    let ax = match (k.direction, fit.x) {
        (Some(Direction::Rtl), Align::Start) => Align::End,
        (Some(Direction::Rtl), Align::End) => Align::Start,
        (_, x) => x,
    };
    Camera {
        zoom: c.zoom,
        x: axis(c.x, bounds.x, bounds.width, vp.width, ax),
        y: axis(c.y, bounds.y, bounds.height, vp.height, fit.y),
    }
}

/// The camera as a native scroller, in screen px.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollMetrics {
    pub scroll_left: f64,
    pub scroll_top: f64,
    pub scroll_width: f64,
    pub scroll_height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub scrollable_x: bool,
    pub scrollable_y: bool,
}

pub fn scroll_metrics(c: Camera, bounds: Rect, vp: Size, padding: f64) -> ScrollMetrics {
    let axis = |pos: f64, origin: f64, content: f64, view: f64| {
        let lo = js::min(origin - padding / c.zoom, pos);
        let hi = js::max(origin + content + padding / c.zoom, pos + view / c.zoom);
        let total = (hi - lo) * c.zoom;
        ((pos - lo) * c.zoom, total, total > view + 0.5)
    };
    let (left, width, scrollable_x) = axis(c.x, bounds.x, bounds.width, vp.width);
    let (top, height, scrollable_y) = axis(c.y, bounds.y, bounds.height, vp.height);
    ScrollMetrics {
        scroll_left: left,
        scroll_top: top,
        scroll_width: width,
        scroll_height: height,
        client_width: vp.width,
        client_height: vp.height,
        scrollable_x,
        scrollable_y,
    }
}

/// `cameraFromScroll`'s target; an absent axis does not move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScrollTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
}

/// `Element.scrollTo`: absolute offsets into the current range, clamped.
pub fn camera_from_scroll(
    c: Camera,
    bounds: Rect,
    vp: Size,
    padding: f64,
    target: ScrollTarget,
) -> Camera {
    let axis = |want: Option<f64>, pos: f64, origin: f64, content: f64, view: f64| -> f64 {
        let Some(want) = want else { return pos };
        let lo = js::min(origin - padding / c.zoom, pos);
        let hi = js::max(origin + content + padding / c.zoom, pos + view / c.zoom);
        let max = js::max(0.0, (hi - lo) * c.zoom - view);
        lo + clamp(want, 0.0, max) / c.zoom
    };
    Camera {
        zoom: c.zoom,
        x: axis(target.left, c.x, bounds.x, bounds.width, vp.width),
        y: axis(target.top, c.y, bounds.y, bounds.height, vp.height),
    }
}

/// The placement algorithm: put `subject` at `align` in the viewport, x
/// resolved against the reading direction. Clamp-free by design. The
/// TypeScript defaults are `padding = 0`, `AlignmentValue::default()` and
/// `Direction::Ltr`.
pub fn place_camera(
    subject: Rect,
    vp: Size,
    zoom: f64,
    padding: f64,
    align: AlignmentValue,
    direction: Direction,
) -> Camera {
    let axis = |a: AlignValue, pos: f64, extent: f64, view: f64| -> f64 {
        match a {
            AlignValue::Named(Align::Start) => pos - padding / zoom,
            AlignValue::Named(Align::End) => pos + extent - (view - padding) / zoom,
            other => {
                let f = match other {
                    AlignValue::Fraction(f) => clamp(f, 0.0, 1.0),
                    _ => 0.5,
                };
                pos + extent / 2.0 - (view * f) / zoom
            }
        }
    };
    let ax = match (direction, align.x) {
        (Direction::Rtl, AlignValue::Named(Align::Start)) => AlignValue::Named(Align::End),
        (Direction::Rtl, AlignValue::Named(Align::End)) => AlignValue::Named(Align::Start),
        (_, x) => x,
    };
    Camera {
        zoom,
        x: axis(ax, subject.x, subject.width, vp.width),
        y: axis(align.y, subject.y, subject.height, vp.height),
    }
}

/// The minimal camera move that makes `rect` fully visible (TypeScript
/// default `padding = 0`).
pub fn reveal_camera(cam: Camera, rect: Rect, vp: Size, padding: f64) -> Camera {
    let axis = |pos: f64, start: f64, size: f64, view: f64| -> f64 {
        let lo = start + size - (view - padding) / cam.zoom;
        let hi = start - padding / cam.zoom;
        if lo > hi {
            return hi;
        }
        clamp(pos, lo, hi)
    };
    Camera {
        zoom: cam.zoom,
        x: axis(cam.x, rect.x, rect.width, vp.width),
        y: axis(cam.y, rect.y, rect.height, vp.height),
    }
}

/// Resolve a zoom intent against a fit box (TypeScript default
/// `padding = 0`).
pub fn resolve_zoom(spec: ZoomSpec, bx: Size, vp: Size, padding: f64) -> f64 {
    let mode = match spec {
        ZoomSpec::Level { level } => return clamp(level, ZOOM_MIN, ZOOM_MAX),
        ZoomSpec::PageWidth { page_width } => {
            return clamp(page_width / bx.width, ZOOM_MIN, ZOOM_MAX);
        }
        ZoomSpec::PageHeight { page_height } => {
            return clamp(page_height / bx.height, ZOOM_MIN, ZOOM_MAX);
        }
        ZoomSpec::Mode { mode } => mode,
    };
    let fit_w = js::max(1.0, vp.width - 2.0 * padding) / bx.width;
    let fit_h = js::max(1.0, vp.height - 2.0 * padding) / bx.height;
    match mode {
        ZoomMode::FitWidth => clamp(fit_w, ZOOM_MIN, ZOOM_MAX),
        ZoomMode::FitPage | ZoomMode::FitAll => clamp(js::min(fit_w, fit_h), ZOOM_MIN, ZOOM_MAX),
        ZoomMode::Automatic => clamp(js::min(fit_w, 1.0), ZOOM_MIN, ZOOM_MAX),
    }
}

// ── anchors ─────────────────────────────────────────────────────────────────

/// The anchor at a world point: its nearest page and the point, page-relative.
pub fn anchor_at_point(scene: &Scene, world_pt: Point) -> Result<Anchor, StageError> {
    let item = scene.nearest_item(world_pt).ok_or(StageError::ReadOfUndefined("pages"))?;
    let mut page = item.pages.first();
    let mut best = f64::INFINITY;
    for p in &item.pages {
        let d = (p.x + p.width / 2.0 - world_pt.x).abs();
        if d < best {
            best = d;
            page = Some(p);
        }
    }
    let page = page.ok_or(StageError::ReadOfUndefined("pageIndex"))?;
    Ok(Anchor {
        page_index: page.page_index,
        fx: (world_pt.x - page.x) / page.width,
        fy: (world_pt.y - page.y) / page.height,
    })
}

/// The camera that puts the anchor's world point at a screen point.
pub fn camera_for_anchor_at_screen(
    anchor: Anchor,
    scene: &Scene,
    screen_pt: Point,
    zoom: f64,
) -> Result<Camera, StageError> {
    let item = scene
        .item_of_page(anchor.page_index)
        .map(|i| &scene.items[i])
        .ok_or(StageError::ReadOfUndefined("pages"))?;
    let mut page = item.pages.first();
    for p in &item.pages {
        if p.page_index == anchor.page_index {
            page = Some(p);
        }
    }
    let page = page.ok_or(StageError::ReadOfUndefined("x"))?;
    let world = Point { x: page.x + anchor.fx * page.width, y: page.y + anchor.fy * page.height };
    Ok(Camera { zoom, x: world.x - screen_pt.x / zoom, y: world.y - screen_pt.y / zoom })
}

fn viewport_center(vp: Size) -> Point {
    Point { x: vp.width / 2.0, y: vp.height / 2.0 }
}

/// The anchor under a viewport point, the viewport center by default.
pub fn anchor_from_camera(
    cam: Camera,
    scene: &Scene,
    vp: Size,
    at: Option<Point>,
) -> Result<Anchor, StageError> {
    anchor_at_point(scene, to_world(cam, at.unwrap_or_else(|| viewport_center(vp))))
}

/// The camera that restores an anchor to the viewport point it was captured
/// at.
pub fn camera_from_anchor(
    anchor: Anchor,
    scene: &Scene,
    vp: Size,
    zoom: f64,
    at: Option<Point>,
) -> Result<Camera, StageError> {
    camera_for_anchor_at_screen(anchor, scene, at.unwrap_or_else(|| viewport_center(vp)), zoom)
}

// ── spatial index ───────────────────────────────────────────────────────────

fn lower_bound(arr: &[f64], t: f64) -> usize {
    let (mut lo, mut hi) = (0, arr.len());
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if arr[mid] < t {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn upper_bound(arr: &[f64], t: f64) -> usize {
    let (mut lo, mut hi) = (0, arr.len());
    while lo < hi {
        let mid = (lo + hi) >> 1;
        if arr[mid] <= t {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

const OVERSCAN: isize = 2;

impl Scene {
    /// Items overlapping `r` (with overscan on linear layouts), in position
    /// order.
    pub fn query(&self, r: Rect) -> Vec<&SceneItem> {
        let n = self.items.len();
        match &self.index {
            SceneIndex::Linear { vertical, mirrored, starts, ends, .. } => {
                let a0 = if *vertical { r.y } else { r.x };
                let a1 = if *vertical { r.y + r.height } else { r.x + r.width };
                let lo = (lower_bound(ends, a0) as isize - OVERSCAN).max(0);
                let hi = (upper_bound(starts, a1) as isize + OVERSCAN).min(n as isize);
                (lo..hi).map(|k| &self.items[pos_idx(*mirrored, n, k as usize)]).collect()
            }
            SceneIndex::Grid { columns, rows, step_x, rtl, row_top, row_end, .. } => {
                let col_at = |col: f64| if *rtl { columns - 1.0 - col } else { col };
                let c0 = js::max(0.0, (r.x / step_x).floor());
                let c1 = js::min(columns - 1.0, ((r.x + r.width) / step_x).floor());
                let r0 = lower_bound(row_end, r.y);
                let r1 =
                    (upper_bound(row_top, r.y + r.height) as isize - 1).min(*rows as isize - 1);
                let mut out = Vec::new();
                for row in r0 as isize..=r1 {
                    let mut col = c0;
                    while col <= c1 {
                        let idx = row as f64 * columns + col_at(col);
                        // A fractional column count indexes between items;
                        // the TypeScript pushes `undefined` there.
                        if idx < n as f64 && idx.fract() == 0.0 {
                            out.push(&self.items[idx as usize]);
                        }
                        col += 1.0;
                    }
                }
                out
            }
        }
    }

    /// The item nearest a world point; `None` where the TypeScript returns
    /// `undefined` (an empty scene, a NaN coordinate on a grid).
    pub fn nearest_item(&self, pt: Point) -> Option<&SceneItem> {
        let n = self.items.len() as isize;
        match &self.index {
            SceneIndex::Linear { vertical, mirrored, starts, .. } => {
                let v = if *vertical { pt.y } else { pt.x };
                let k = clamp_index(upper_bound(starts, v) as isize - 1, 0, n - 1);
                (k >= 0).then(|| &self.items[pos_idx(*mirrored, n as usize, k as usize)])
            }
            SceneIndex::Grid { columns, rows, step_x, rtl, row_top, .. } => {
                let col = clamp((pt.x / step_x).floor(), 0.0, columns - 1.0);
                let col = if *rtl { columns - 1.0 - col } else { col };
                let row =
                    clamp_index(upper_bound(row_top, pt.y) as isize - 1, 0, *rows as isize - 1);
                let idx = clamp(row as f64 * columns + col, 0.0, n as f64 - 1.0);
                (idx >= 0.0 && idx.fract() == 0.0).then(|| &self.items[idx as usize])
            }
        }
    }

    /// The index of the item holding a page; `None` for an empty scene,
    /// where the TypeScript returns -1.
    pub fn item_of_page(&self, page_index: usize) -> Option<usize> {
        let first_page = match &self.index {
            SceneIndex::Linear { first_page, .. } | SceneIndex::Grid { first_page, .. } => {
                first_page
            }
        };
        let n = self.items.len() as isize;
        let k = clamp_index(upper_bound(first_page, page_index as f64) as isize - 1, 0, n - 1);
        (k >= 0).then_some(k as usize)
    }
}

/// Position order to index order: reversed in a mirrored layout.
fn pos_idx(mirrored: bool, n: usize, k: usize) -> usize {
    if mirrored { n - 1 - k } else { k }
}

// ── layout ──────────────────────────────────────────────────────────────────

struct LocalBox {
    page_index: usize,
    lx: f64,
    ly: f64,
    w: f64,
    h: f64,
    rotation: PageRotation,
    user_unit: f64,
}

fn page_of(pages: &[PageGeom], index: usize) -> Result<&PageGeom, StageError> {
    pages.get(index).ok_or(StageError::ReadOfUndefined("userUnit"))
}

/// Intrinsic item dimensions (pages only, no frame).
fn measure_item(pages: &[PageGeom], group: &[usize], gap: f64) -> Result<Size, StageError> {
    let mut w = 0.0;
    let mut h = 0.0;
    for (j, &pi) in group.iter().enumerate() {
        let d = display_dims(page_of(pages, pi)?);
        w += d.width + if j > 0 { gap } else { 0.0 };
        if d.height > h {
            h = d.height;
        }
    }
    Ok(Size { width: w, height: h })
}

struct Packed {
    local: Vec<LocalBox>,
    width: f64,
    height: f64,
}

/// Pack one item from scaled pages plus constant per-page frames.
fn pack_scaled_item(
    pages: &[PageGeom],
    group: &[usize],
    scale: f64,
    gap: f64,
    frame: PageFrame,
    direction: Direction,
) -> Packed {
    // `measure_item` has already checked every index in the group.
    let mut max_h = 0.0;
    for &pi in group {
        max_h = js::max(max_h, display_dims(&pages[pi]).height * scale);
    }
    let mut local = Vec::with_capacity(group.len());
    let mut lx = 0.0;
    for &pi in group {
        let pg = &pages[pi];
        let d = display_dims(pg);
        let w = d.width * scale;
        let h = d.height * scale;
        local.push(LocalBox {
            page_index: pi,
            lx: lx + frame.left,
            ly: frame.top + (max_h - h) / 2.0,
            w,
            h,
            rotation: pg.rotation.unwrap_or(PageRotation::DEG_0),
            user_unit: pg.user_unit.unwrap_or(1.0),
        });
        lx += frame.left + w + frame.right + gap * scale;
    }
    let width = js::max(0.0, lx - gap * scale);
    let height = frame.top + max_h + frame.bottom;
    if direction == Direction::Rtl {
        for b in &mut local {
            b.lx = width - b.lx - b.w + (frame.left - frame.right);
        }
    }
    Packed { local, width, height }
}

fn place_pages(item: &SceneItem, local: &[LocalBox], content_scale: f64) -> Vec<PageBox> {
    local
        .iter()
        .map(|b| PageBox {
            page_index: b.page_index,
            x: item.x + b.lx,
            y: item.y + b.ly,
            width: b.w,
            height: b.h,
            rotation: b.rotation,
            content_scale: content_scale * b.user_unit,
        })
        .collect()
}

fn item_scale(
    cross_intrinsic: f64,
    ref_cross: f64,
    sizing: SizingMode,
    view_units_per_point: Option<f64>,
) -> f64 {
    let factor = if sizing == SizingMode::Uniform && cross_intrinsic > 0.0 {
        ref_cross / cross_intrinsic
    } else {
        1.0
    };
    factor * view_units_per_point.unwrap_or(1.0)
}

/// An empty group has no first page: `undefined`, which every comparison
/// in the binary search treats as NaN.
fn first_pages(items: &[SceneItem]) -> Vec<f64> {
    items.iter().map(|it| it.page_indexes.first().map_or(f64::NAN, |&p| p as f64)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinearAxis {
    X,
    Y,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinearAlign {
    Center,
    Start,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinearOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub axis: Option<LinearAxis>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<LinearAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizing: Option<SizingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_frame: Option<PageFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_units_per_point: Option<f64>,
}

/// Lay items along one axis. Fails only when `grouping` names a page that
/// is not in `pages`.
pub fn linear_layout(
    pages: &[PageGeom],
    grouping: &[Vec<usize>],
    opts: LinearOptions,
) -> Result<Scene, StageError> {
    let gap = opts.gap.unwrap_or(16.0);
    let vertical = opts.axis.unwrap_or(LinearAxis::Y) == LinearAxis::Y;
    let align = opts.align.unwrap_or(LinearAlign::Center);
    let sizing = opts.sizing.unwrap_or_default();
    let direction = opts.direction.unwrap_or_default();
    let mirrored = !vertical && direction == Direction::Rtl;
    let frame = opts.page_frame.unwrap_or(NO_FRAME);

    let measures =
        grouping.iter().map(|g| measure_item(pages, g, gap)).collect::<Result<Vec<_>, _>>()?;
    let cross_of = |s: Size| if vertical { s.width } else { s.height };
    let ref_cross = measures.iter().fold(0.0, |m, s| js::max(m, cross_of(*s)));

    let mut items = Vec::with_capacity(grouping.len());
    let mut locals = Vec::with_capacity(grouping.len());
    let mut scales = Vec::with_capacity(grouping.len());
    let mut main = 0.0;
    let mut cross_max = 0.0;
    let mut max_w = 0.0;
    let mut max_h = 0.0;
    for (i, group) in grouping.iter().enumerate() {
        let s = item_scale(cross_of(measures[i]), ref_cross, sizing, opts.view_units_per_point);
        let packed = pack_scaled_item(pages, group, s, gap, frame, direction);
        let (width, height) = (packed.width, packed.height);
        scales.push(s);
        locals.push(packed.local);
        let mut it = SceneItem {
            index: i,
            x: 0.0,
            y: 0.0,
            width,
            height,
            page_indexes: group.clone(),
            pages: Vec::new(),
        };
        if vertical {
            it.y = main;
            main += height + gap;
        } else {
            it.x = main;
            main += width + gap;
        }
        cross_max = js::max(cross_max, if vertical { width } else { height });
        max_w = js::max(max_w, width);
        max_h = js::max(max_h, height);
        items.push(it);
    }

    let scene_main = js::max(0.0, main - gap);
    let size = if vertical {
        Size { width: cross_max, height: scene_main }
    } else {
        Size { width: scene_main, height: cross_max }
    };

    for (i, it) in items.iter_mut().enumerate() {
        if mirrored {
            it.x = size.width - it.x - it.width;
        }
        let centered = align == LinearAlign::Center;
        if vertical {
            it.x = if centered { (size.width - it.width) / 2.0 } else { 0.0 };
        } else {
            it.y = if centered { (size.height - it.height) / 2.0 } else { 0.0 };
        }
        it.pages = place_pages(it, &locals[i], scales[i]);
    }

    let n = items.len();
    let at = |k: usize| &items[pos_idx(mirrored, n, k)];
    let starts = (0..n).map(|k| if vertical { at(k).y } else { at(k).x }).collect();
    let ends = (0..n)
        .map(|k| if vertical { at(k).y + at(k).height } else { at(k).x + at(k).width })
        .collect();
    let first_page = first_pages(&items);

    Ok(Scene {
        size,
        item_count: n,
        axis: if vertical { Axis::Y } else { Axis::X },
        max_item_size: Size { width: max_w, height: max_h },
        index: SceneIndex::Linear { vertical, mirrored, starts, ends, first_page },
        items,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GridOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<f64>,
    /// Wrapped mode: derive the column count from the line width. Takes
    /// precedence over `columns`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizing: Option<SizingMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_frame: Option<PageFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view_units_per_point: Option<f64>,
}

/// Lay items in a grid of uniform-width columns and per-row heights. Fails
/// when `grouping` names a missing page, or when the column count comes out
/// NaN (a NaN `line_width` or `columns`, or `line_width + gap` and
/// `cell + gap` both zero).
pub fn grid_layout(
    pages: &[PageGeom],
    grouping: &[Vec<usize>],
    opts: GridOptions,
) -> Result<Scene, StageError> {
    let gap = opts.gap.unwrap_or(48.0);
    let n = grouping.len();
    let nf = n as f64;
    let sizing = opts.sizing.unwrap_or_default();
    let rtl = opts.direction.unwrap_or_default() == Direction::Rtl;
    let frame = opts.page_frame.unwrap_or(NO_FRAME);

    let measures =
        grouping.iter().map(|g| measure_item(pages, g, gap)).collect::<Result<Vec<_>, _>>()?;
    let ref_w = measures.iter().fold(0.0, |m, s| js::max(m, s.width));

    let mut locals = Vec::with_capacity(n);
    let mut sizes = Vec::with_capacity(n);
    let mut scales = Vec::with_capacity(n);
    let mut cell_w = 1.0;
    let mut cell_h = 1.0;
    for (i, group) in grouping.iter().enumerate() {
        let s = item_scale(measures[i].width, ref_w, sizing, opts.view_units_per_point);
        let packed = pack_scaled_item(
            pages,
            group,
            s,
            gap,
            frame,
            if rtl { Direction::Rtl } else { Direction::Ltr },
        );
        scales.push(s);
        locals.push(packed.local);
        sizes.push(Size { width: packed.width, height: packed.height });
        cell_w = js::max(cell_w, packed.width);
        cell_h = js::max(cell_h, packed.height);
    }

    let wanted = match opts.line_width {
        Some(line_width) => ((line_width + gap) / (cell_w + gap)).floor(),
        None => opts.columns.unwrap_or_else(|| nf.sqrt().ceil()),
    };
    let columns = js::max(1.0, js::min(wanted, js::max(1.0, nf)));
    let col_at = |col: f64| if rtl { columns - 1.0 - col } else { col };

    let step_x = cell_w + gap;
    let rows = js::max(1.0, (nf / columns).ceil());
    if rows.is_nan() {
        return Err(StageError::InvalidArrayLength);
    }
    let rows = rows as usize;
    let mut row_height = vec![1.0; rows];
    for (i, size) in sizes.iter().enumerate() {
        let row = (i as f64 / columns).floor() as usize;
        row_height[row] = js::max(row_height[row], size.height);
    }
    let mut row_top = Vec::with_capacity(rows);
    let mut y_cursor = 0.0;
    for height in &row_height {
        row_top.push(y_cursor);
        y_cursor += height + gap;
    }
    let row_end: Vec<f64> = row_top.iter().zip(&row_height).map(|(t, h)| t + h).collect();

    let mut items = Vec::with_capacity(n);
    for (i, group) in grouping.iter().enumerate() {
        let col = col_at(i as f64 % columns);
        let row = (i as f64 / columns).floor() as usize;
        let mut it = SceneItem {
            index: i,
            x: col * step_x + (cell_w - sizes[i].width) / 2.0,
            y: row_top[row] + (row_height[row] - sizes[i].height) / 2.0,
            width: sizes[i].width,
            height: sizes[i].height,
            page_indexes: group.clone(),
            pages: Vec::new(),
        };
        it.pages = place_pages(&it, &locals[i], scales[i]);
        items.push(it);
    }

    let size = Size { width: columns * step_x - gap, height: js::max(1.0, y_cursor - gap) };
    let first_page = first_pages(&items);

    Ok(Scene {
        size,
        items,
        item_count: n,
        axis: Axis::Grid,
        max_item_size: Size { width: cell_w, height: cell_h },
        index: SceneIndex::Grid { columns, rows, step_x, rtl, row_top, row_end, first_page },
    })
}

/// Spread grouping, the pure input to layout.
pub fn group_pages(page_count: usize, mode: SpreadMode) -> Vec<Vec<usize>> {
    if mode == SpreadMode::None {
        return (0..page_count).map(|i| vec![i]).collect();
    }
    let mut out = Vec::new();
    let mut i = 0;
    if mode == SpreadMode::Even && page_count > 0 {
        out.push(vec![0]);
        i = 1;
    }
    while i < page_count {
        out.push(if i + 1 < page_count { vec![i, i + 1] } else { vec![i] });
        i += 2;
    }
    out
}
