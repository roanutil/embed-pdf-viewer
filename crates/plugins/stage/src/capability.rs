//! `capability.ts`: the stage capability, selectors (pure reads) plus
//! intents (the only writers).
//!
//! The pure spatial math is `epdf-stage-core`; this is the impure shell. It
//! dispatches, caches the scene, and owns the camera tween, the rest
//! detector and the momentum physics, whose frames come through the
//! [`Scheduler`] the host passes (see the README's host inputs).
//!
//! Every camera move is defined by what it holds fixed: gestures hold the
//! pointer, pointer-less zooms the `zoomAlign` point, reframes the
//! `anchorAlign` point, arrivals nothing prior (a fresh landing at
//! `arrivalAlign`), explicit reveals whatever the call says. `fitAlign` is
//! the clamp's rest constraint on an axis with no travel.
//!
//! The order of reads, dispatches and scheduler calls follows the
//! TypeScript line by line, because the vectors record every dispatch. No
//! `RefCell` borrow is held across a dispatch: a dispatch runs the effects'
//! watches, and the page-state feed reads back into this capability.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_geometry::{
    PageRotation, PageTransformInput, Point, Rect, Size, add_rotations, apply_point, apply_rect,
    display_size, intersect_rects, js, page_transform, rotate_scale_matrix, snap_to_device,
};
use epdf_kernel::{Ctx, Error};
use epdf_stage_core as sc;
use epdf_stage_core::{
    Align, AlignValue, AlignmentValue, Anchor, Camera, CameraConstraint, Direction, PageBox,
    PageGeom, Scene, SceneItem, ScrollMetrics, ZoomMode, ZoomSpec,
};

use crate::destination::{PageLayout, without_undefined};
use crate::motion::{
    FLING_STOP, ease_out_cubic, glide_step, rubber_in, rubber_out, spring_step, zoom_lerp,
};
use crate::responsive::{box_of, eq_setting, merge_settings, resolve_responsive};
use crate::settings::{
    SETTING_KEYS, SettingEffect, SettingKey, default_responsive, patch_has, patch_keys,
    settings_effect,
};
use crate::types::*;

/// A `TypeError` for a member read on `undefined`, with V8's message.
pub(crate) fn read_of_undefined(member: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of undefined (reading '{member}')"))
}

/// JavaScript falsiness of a number: `0`, `-0` and `NaN`.
fn falsy(x: f64) -> bool {
    x == 0.0 || x.is_nan()
}

fn num(x: f64) -> String {
    js::number_to_string(x)
}

const REST_MS: f64 = 150.0;
const TWEEN_MS: f64 = 240.0;

struct Grouping {
    key: (usize, sc::SpreadMode),
    grouping: Rc<Vec<Vec<usize>>>,
    first_pages: Rc<Vec<usize>>,
}

type Provider = Rc<dyn Fn() -> Option<StageViewState>>;

struct Inner {
    scheduler: Option<Rc<dyn Scheduler>>,
    grouping_cache: RefCell<Option<Rc<Grouping>>>,
    scene_cache: RefCell<Option<(String, Rc<Scene>)>>,
    registry_cache: RefCell<Option<((u32, u64), Rc<Vec<PageLayout>>)>>,
    /// The behavioural latch: flips when initial placement starts.
    placement_started: Cell<bool>,
    gesture_depth: Cell<u32>,
    gesture_zoomed: Cell<bool>,
    gesture_elastic: Cell<bool>,
    gesture_raw: Cell<Option<Camera>>,
    rest_raf: Cell<u64>,
    raf: Cell<u64>,
    scroll_memo: Cell<Option<ScrollMetrics>>,
    vis: RefCell<(String, Rc<Vec<VisiblePage>>)>,
    base: RefCell<StageSettings>,
    rules: RefCell<Rc<Vec<ResponsiveRule>>>,
    last_effective: RefCell<StageSettings>,
    providers: RefCell<Vec<(f64, Provider)>>,
}

/// `StageCapability`. Cheap to clone; clones are the same lens.
#[derive(Clone)]
pub struct StageCapability {
    ctx: Ctx<StageState, StageAction>,
    inner: Rc<Inner>,
}

/// Where a placement lands and what it clamps against.
#[derive(Clone, Copy)]
struct Placement {
    camera: Camera,
    bounds: Rect,
}

#[derive(Clone, Copy)]
struct RevealPlacement {
    camera: Camera,
    bounds: Rect,
    zoom_changed: bool,
}

/// One axis of the momentum integrator, in camera-space screen px.
#[derive(Clone, Copy)]
struct AxisState {
    p: f64,
    v: f64,
    springing: bool,
    done: bool,
}

/// `createStageCapability(ctx, config)`.
pub fn create_stage_capability(
    ctx: Ctx<StageState, StageAction>,
    config: StageConfig,
) -> StageCapability {
    let base = ctx.state().settings.clone();
    let rules = config.responsive.unwrap_or_else(default_responsive);
    StageCapability {
        inner: Rc::new(Inner {
            scheduler: config.scheduler,
            grouping_cache: RefCell::new(None),
            scene_cache: RefCell::new(None),
            registry_cache: RefCell::new(None),
            placement_started: Cell::new(false),
            gesture_depth: Cell::new(0),
            gesture_zoomed: Cell::new(false),
            gesture_elastic: Cell::new(false),
            gesture_raw: Cell::new(None),
            rest_raf: Cell::new(0),
            raf: Cell::new(0),
            scroll_memo: Cell::new(None),
            vis: RefCell::new((String::new(), Rc::new(Vec::new()))),
            last_effective: RefCell::new(base.clone()),
            base: RefCell::new(base),
            rules: RefCell::new(Rc::new(rules)),
            providers: RefCell::new(Vec::new()),
        }),
        ctx,
    }
}

impl StageCapability {
    /// Whether two handles are the same lens.
    pub fn same(&self, other: &StageCapability) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    // ── host timing seam ──────────────────────────────────────────────────────

    fn can_animate(&self) -> bool {
        self.inner.scheduler.is_some()
    }

    fn raf_call(&self, callback: Box<dyn FnOnce(f64)>) -> u64 {
        match &self.inner.scheduler {
            Some(s) => s.raf(callback),
            None => 0,
        }
    }

    fn caf_call(&self, handle: u64) {
        if let Some(s) = &self.inner.scheduler {
            s.caf(handle);
        }
    }

    /// Whether the slice still exists: a frame scheduled before the
    /// document closed must not read a removed slice.
    fn alive(&self) -> bool {
        self.ctx.try_state().is_some()
    }

    // ── state reads ───────────────────────────────────────────────────────────

    fn st(&self) -> Rc<StageState> {
        self.ctx.state()
    }

    fn cam(&self) -> Camera {
        self.st().camera
    }

    fn vp(&self) -> Size {
        self.st().vp
    }

    fn dpr(&self) -> f64 {
        self.st().dpr
    }

    fn pad(&self) -> f64 {
        self.st().settings.padding
    }

    fn paged(&self) -> bool {
        self.st().settings.flow == FlowMode::Paged
    }

    fn is_fit_all(&self) -> bool {
        matches!(self.st().settings.zoom, ZoomSpec::Mode { mode: ZoomMode::FitAll })
    }

    fn dispatch(&self, action: StageAction) {
        self.ctx.dispatch(action);
    }

    /// `ctx.document()?.pages`, parsed once per registry revision.
    fn registry(&self) -> Rc<Vec<PageLayout>> {
        let Some(doc) = self.ctx.document() else { return Rc::new(Vec::new()) };
        let key = (doc.page_count, doc.revision);
        if let Some((k, pages)) = self.inner.registry_cache.borrow().as_ref()
            && *k == key
        {
            return pages.clone();
        }
        let pages: Vec<PageLayout> = doc
            .pages
            .as_array()
            .map(|pages| {
                pages
                    .iter()
                    .filter_map(|p| serde_json::from_value(without_undefined(p)).ok())
                    .collect()
            })
            .unwrap_or_default();
        let pages = Rc::new(pages);
        *self.inner.registry_cache.borrow_mut() = Some((key, pages.clone()));
        pages
    }

    fn page_count_now(&self) -> usize {
        self.ctx.document().map_or(0, |d| d.page_count as usize)
    }

    // ── the item model (spread grouping) ─────────────────────────────────────

    fn grouping(&self) -> Rc<Grouping> {
        let key = (self.page_count_now(), self.st().settings.spread);
        if let Some(g) = self.inner.grouping_cache.borrow().as_ref()
            && g.key == key
        {
            return g.clone();
        }
        let g = sc::group_pages(key.0, key.1);
        let first_pages = g.iter().map(|item| item[0]).collect();
        let cache =
            Rc::new(Grouping { key, grouping: Rc::new(g), first_pages: Rc::new(first_pages) });
        *self.inner.grouping_cache.borrow_mut() = Some(cache.clone());
        cache
    }

    fn item_index_of_page(&self, page_index: usize) -> usize {
        let g = self.grouping();
        let fp = &g.first_pages;
        let (mut lo, mut hi) = (0, fp.len());
        while lo < hi {
            let mid = (lo + hi) >> 1;
            if fp[mid] <= page_index {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo.saturating_sub(1)
    }

    // ── screen-px settings in world units ─────────────────────────────────────

    fn effective_zoom(&self) -> f64 {
        let st = self.st();
        match st.settings.zoom {
            ZoomSpec::Level { level } => level,
            _ => js::max(st.camera.zoom, 0.0001),
        }
    }

    fn wrap_line_width(&self) -> f64 {
        js::max(1.0, (self.vp().width - 2.0 * self.pad()) / self.effective_zoom())
    }

    fn no_frame(m: &epdf_geometry::PageFrame) -> bool {
        falsy(m.top) && falsy(m.right) && falsy(m.bottom) && falsy(m.left)
    }

    fn world_page_frame(&self) -> epdf_geometry::PageFrame {
        let m = self.st().settings.page_frame;
        if Self::no_frame(&m) {
            return m;
        }
        let ez = self.effective_zoom();
        epdf_geometry::PageFrame {
            top: m.top / ez,
            right: m.right / ez,
            bottom: m.bottom / ez,
            left: m.left / ez,
        }
    }

    fn frame_key(&self) -> String {
        let m = self.st().settings.page_frame;
        if Self::no_frame(&m) {
            return "-".into();
        }
        let w = self.world_page_frame();
        format!(
            "{},{},{},{}",
            num(js::round(w.top)),
            num(js::round(w.right)),
            num(js::round(w.bottom)),
            num(js::round(w.left))
        )
    }

    fn world_gap(&self) -> f64 {
        match self.st().settings.gap {
            Gap::World(g) => g,
            Gap::Px { px } if !falsy(px) => px / self.effective_zoom(),
            Gap::Px { .. } => 0.0,
        }
    }

    fn gap_key(&self) -> String {
        match self.st().settings.gap {
            Gap::World(g) => num(g),
            Gap::Px { .. } => format!("px:{}", num(js::round(self.world_gap()))),
        }
    }

    fn layout_for(&self, groups: &[Vec<usize>]) -> Scene {
        let st = self.st();
        let s = &st.settings;
        // The view-rotation injection point: each page's display rotation
        // is its /Rotate plus this lens's viewRotation.
        let vr = s.view_rotation;
        let pages: Vec<PageGeom> = self
            .registry()
            .iter()
            .map(|p| PageGeom {
                size: p.size,
                rotation: Some(if vr.0 == 0.0 {
                    p.rotation
                } else {
                    add_rotations(p.rotation, vr)
                }),
                user_unit: p.user_unit,
            })
            .collect();
        let page_frame = self.world_page_frame();
        let gap = self.world_gap();
        let vupp = s.view_units_per_point;
        // stage-core fails only on a grouping that names a missing page or
        // a NaN column count; the grouping comes from the same registry.
        let scene = if s.layout == LayoutKind::Grid {
            sc::grid_layout(
                &pages,
                groups,
                sc::GridOptions {
                    gap: Some(gap),
                    columns: match s.columns {
                        GridColumns::Count(n) => Some(n),
                        GridColumns::Policy(_) => None,
                    },
                    line_width: (s.columns == GridColumns::Policy(ColumnsPolicy::Auto))
                        .then(|| self.wrap_line_width()),
                    sizing: Some(s.sizing),
                    direction: Some(s.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(vupp),
                },
            )
        } else {
            sc::linear_layout(
                &pages,
                groups,
                sc::LinearOptions {
                    gap: Some(gap),
                    axis: Some(if s.layout == LayoutKind::Horizontal {
                        sc::LinearAxis::X
                    } else {
                        sc::LinearAxis::Y
                    }),
                    align: None,
                    sizing: Some(s.sizing),
                    direction: Some(s.direction),
                    page_frame: Some(page_frame),
                    view_units_per_point: Some(vupp),
                },
            )
        };
        scene.unwrap_or_else(|e| panic!("[stage] layout: {}: {e}", e.name()))
    }

    fn columns_key(&self) -> String {
        let st = self.st();
        if st.settings.layout != LayoutKind::Grid {
            return "-".into();
        }
        match st.settings.columns {
            GridColumns::Policy(ColumnsPolicy::Auto) => {
                format!("auto:{}", num(js::round(self.wrap_line_width())))
            }
            GridColumns::Policy(ColumnsPolicy::Square) => "square".into(),
            GridColumns::Count(n) => num(n),
        }
    }

    /// The scene's settings signature, over the `'scene'` rows of the
    /// registry in key order. Primitives key by `String(v)`; the three
    /// screen-px settings quantize.
    fn settings_key(&self) -> String {
        let st = self.st();
        let s = &st.settings;
        let mut parts = Vec::new();
        for key in SETTING_KEYS {
            if settings_effect(key) != SettingEffect::Scene {
                continue;
            }
            parts.push(match key {
                SettingKey::Columns => self.columns_key(),
                SettingKey::Gap => self.gap_key(),
                SettingKey::PageFrame => self.frame_key(),
                SettingKey::Layout => serde_str(&s.layout),
                SettingKey::Spread => serde_str(&s.spread),
                SettingKey::Sizing => serde_str(&s.sizing),
                SettingKey::Direction => serde_str(&s.direction),
                SettingKey::ViewRotation => num(s.view_rotation.0),
                SettingKey::ViewUnitsPerPoint => num(s.view_units_per_point),
                other => unreachable!("{other:?} is not a scene setting"),
            });
        }
        parts.join("|")
    }

    fn doc_key(&self) -> String {
        match self.ctx.document() {
            Some(doc) => format!("{}.{}", doc.page_count, doc.revision),
            None => "0.0".into(),
        }
    }

    /// The scene: the whole document (continuous), or a one-item slice at
    /// the cursor (paged). Cached by key; the same `Rc` means the same
    /// scene, as the TypeScript compares scenes by reference.
    fn build_scene(&self) -> Rc<Scene> {
        let g = self.grouping();
        let groups = &g.grouping;
        let (key, slice) = if self.paged() {
            let idx = if groups.is_empty() {
                0
            } else {
                self.item_index_of_page(self.st().cursor).min(groups.len() - 1)
            };
            (format!("paged|{}|{}|{idx}", self.doc_key(), self.settings_key()), Some(idx))
        } else {
            (format!("cont|{}|{}", self.doc_key(), self.settings_key()), None)
        };
        if let Some((k, scene)) = self.inner.scene_cache.borrow().as_ref()
            && *k == key
        {
            return scene.clone();
        }
        let scene = Rc::new(match slice {
            Some(idx) => {
                self.layout_for(if groups.is_empty() { &[] } else { &groups[idx..idx + 1] })
            }
            None => self.layout_for(groups),
        });
        *self.inner.scene_cache.borrow_mut() = Some((key, scene.clone()));
        scene
    }

    fn scene_cache_key(&self) -> String {
        self.inner.scene_cache.borrow().as_ref().map(|(k, _)| k.clone()).unwrap_or_default()
    }

    fn invalidate_scene(&self) {
        *self.inner.scene_cache.borrow_mut() = None;
    }

    // ── geometry helpers ──────────────────────────────────────────────────────

    fn scene_rect(&self) -> Rect {
        let size = self.build_scene().size;
        Rect { x: 0.0, y: 0.0, width: size.width, height: size.height }
    }

    fn item_rect(it: &SceneItem) -> Rect {
        Rect { x: it.x, y: it.y, width: it.width, height: it.height }
    }

    fn page_box_of(it: &SceneItem, page_index: usize) -> &PageBox {
        it.pages.iter().find(|p| p.page_index == page_index).unwrap_or(&it.pages[0])
    }

    fn page_rect_of(it: &SceneItem, page_index: usize) -> Rect {
        let b = Self::page_box_of(it, page_index);
        Rect { x: b.x, y: b.y, width: b.width, height: b.height }
    }

    /// The item shown for the cursor.
    fn cursor_item(&self) -> Option<SceneItem> {
        let scene = self.build_scene();
        if self.paged() {
            scene.items.first().cloned()
        } else {
            scene.items.get(self.item_index_of_page(self.st().cursor)).cloned()
        }
    }

    /// Content-space rect on a page to a world rect, through the same
    /// quarter-turn matrix `pageRectToScreen` uses.
    fn world_rect_for_content(it: &SceneItem, page_index: usize, rect: Rect) -> Rect {
        let b = Self::page_box_of(it, page_index);
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let wr = apply_rect(m, rect);
        Rect { x: b.x + wr.x, y: b.y + wr.y, width: wr.width, height: wr.height }
    }

    /// One axis of a positioned-reveal camera.
    #[allow(clippy::too_many_arguments)]
    fn reveal_axis(
        &self,
        a: Option<RevealAnchorValue>,
        cam_pos: f64,
        rect_pos: f64,
        rect_extent: f64,
        vp_extent: f64,
        zoom: f64,
        zoom_changed: bool,
    ) -> f64 {
        let mut a = a;
        if a == Some(RevealAnchorValue::Word(RevealWord::Keep)) {
            return cam_pos;
        }
        let p = self.pad();
        if a.is_none() {
            if !zoom_changed {
                let lo = cam_pos + p / zoom;
                let hi = cam_pos + (vp_extent - p) / zoom;
                if rect_pos >= lo && rect_pos + rect_extent <= hi {
                    return cam_pos;
                }
                if rect_extent > hi - lo || rect_pos < lo {
                    return rect_pos - p / zoom;
                }
                return rect_pos + rect_extent - (vp_extent - p) / zoom;
            }
            a = Some(RevealAnchorValue::Word(RevealWord::Center));
        }
        match a {
            Some(RevealAnchorValue::Word(RevealWord::Start)) => rect_pos - p / zoom,
            Some(RevealAnchorValue::Word(RevealWord::End)) => {
                rect_pos + rect_extent - (vp_extent - p) / zoom
            }
            other => {
                let f = match other {
                    Some(RevealAnchorValue::Fraction(f)) => js::min(1.0, js::max(0.0, f)),
                    _ => 0.5,
                };
                rect_pos + rect_extent / 2.0 - (vp_extent * f) / zoom
            }
        }
    }

    /// Does this rect fit the padded viewport at this zoom?
    fn fits(&self, rect: Rect, zoom: f64) -> bool {
        let v = self.vp();
        let p = self.pad();
        let eps = 0.5;
        rect.width * zoom <= v.width - 2.0 * p + eps
            && rect.height * zoom <= v.height - 2.0 * p + eps
    }

    fn align_fraction(a: AlignValue) -> f64 {
        match a {
            AlignValue::Named(Align::Start) => 0.0,
            AlignValue::Named(Align::Center) => 0.5,
            AlignValue::Named(Align::End) => 1.0,
            AlignValue::Fraction(f) => js::min(1.0, js::max(0.0, f)),
        }
    }

    /// An alignment policy to a viewport point, interpolating the padded
    /// range; named x stops are logical under RTL.
    fn align_point(&self, al: AlignmentValue, v: Size) -> Point {
        let rtl = self.st().settings.direction == Direction::Rtl;
        let ax = match al.x {
            AlignValue::Named(Align::Start) if rtl => AlignValue::Named(Align::End),
            AlignValue::Named(Align::End) if rtl => AlignValue::Named(Align::Start),
            x => x,
        };
        let p = self.pad();
        Point {
            x: p + (v.width - 2.0 * p) * Self::align_fraction(ax),
            y: p + (v.height - 2.0 * p) * Self::align_fraction(al.y),
        }
    }

    fn anchor_point(&self) -> Point {
        self.align_point(self.st().settings.anchor_align, self.vp())
    }

    /// The fit box the zoom intent resolves against.
    fn fit_box(&self, item: &SceneItem) -> Size {
        if self.is_fit_all() {
            return self.build_scene().size;
        }
        if self.paged() {
            Size { width: item.width, height: item.height }
        } else {
            self.build_scene().max_item_size
        }
    }

    fn bounds_for(&self, it: &SceneItem) -> Rect {
        if self.paged() { Self::item_rect(it) } else { self.scene_rect() }
    }

    /// Clamp rect for a stay write (pan/zoom). In paged flow over a
    /// document with no pages the slice has no item, and the TypeScript
    /// reads `x` of `undefined`.
    fn stay_bounds(&self) -> Result<Rect, Error> {
        if self.paged() {
            let scene = self.build_scene();
            scene.items.first().map(Self::item_rect).ok_or_else(|| read_of_undefined("x"))
        } else {
            Ok(self.scene_rect())
        }
    }

    fn constraint(&self) -> CameraConstraint {
        let st = self.st();
        CameraConstraint {
            bounded: st.settings.bounded,
            padding: st.settings.padding,
            fit_align: Some(st.settings.fit_align),
            direction: Some(st.settings.direction),
        }
    }

    // ── camera-rest detector ──────────────────────────────────────────────────

    fn arm_rest(&self) {
        if !self.can_animate() || !self.inner.placement_started.get() {
            return;
        }
        if self.st().camera_resting {
            self.dispatch(StageAction::CameraRest { resting: false });
        }
        let rest_raf = self.inner.rest_raf.get();
        if rest_raf != 0 {
            self.caf_call(rest_raf);
        }
        let t0 = Rc::new(Cell::new(0.0));
        let id = self.raf_call(self.rest_tick(t0));
        self.inner.rest_raf.set(id);
    }

    fn rest_tick(&self, t0: Rc<Cell<f64>>) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |ts| {
            if !this.alive() {
                return;
            }
            this.inner.rest_raf.set(0);
            if falsy(t0.get()) {
                t0.set(ts);
            }
            if ts - t0.get() >= REST_MS {
                this.dispatch(StageAction::CameraRest { resting: true });
                return;
            }
            let id = this.raf_call(this.rest_tick(t0));
            this.inner.rest_raf.set(id);
        })
    }

    /// The one low-level camera write: clamp to `bounds`, dispatch. It
    /// never touches the cursor.
    fn set_cam(&self, next: Camera, bounds: Rect) {
        let clamped = sc::clamp_camera(next, bounds, self.vp(), self.constraint());
        if clamped.zoom != self.cam().zoom {
            if self.inner.gesture_depth.get() > 0 {
                // Mid-gesture: un-rest now, hold the countdown for endGesture.
                self.inner.gesture_zoomed.set(true);
                if self.st().camera_resting {
                    self.dispatch(StageAction::CameraRest { resting: false });
                }
                let rest_raf = self.inner.rest_raf.get();
                if rest_raf != 0 {
                    self.caf_call(rest_raf);
                    self.inner.rest_raf.set(0);
                }
            } else {
                self.arm_rest();
            }
        }
        self.dispatch(StageAction::Camera { camera: clamped });
    }

    /// `setCam(next)` with its default bounds.
    fn set_cam_stay(&self, next: Camera) -> Result<(), Error> {
        let bounds = self.stay_bounds()?;
        self.set_cam(next, bounds);
        Ok(())
    }

    // ── rubber-band (elastic overscroll) ──────────────────────────────────────

    fn axis_travels(&self, origin: f64, content: f64, view: f64, zoom: f64) -> bool {
        !sc::travel_range(origin, content, view, zoom, self.constraint().padding).fits
    }

    /// Unclamped finger-integrated camera to the displayed camera.
    fn rubberize(&self, raw: Camera) -> Result<Camera, Error> {
        let clamped = sc::clamp_camera(raw, self.stay_bounds()?, self.vp(), self.constraint());
        let v = self.vp();
        let b = self.stay_bounds()?;
        let axis = |raw_a: f64, clamped_a: f64, dim: f64, travels: bool| -> f64 {
            if !travels {
                return clamped_a;
            }
            let d_world = raw_a - clamped_a;
            if d_world == 0.0 {
                return clamped_a;
            }
            let out = rubber_out(d_world.abs() * raw.zoom, js::max(1.0, dim));
            clamped_a + (js::sign(d_world) * out) / raw.zoom
        };
        Ok(Camera {
            zoom: raw.zoom,
            x: axis(raw.x, clamped.x, v.width, self.axis_travels(b.x, b.width, v.width, raw.zoom)),
            y: axis(
                raw.y,
                clamped.y,
                v.height,
                self.axis_travels(b.y, b.height, v.height, raw.zoom),
            ),
        })
    }

    /// Displayed camera to the raw position `rubberize` produced it from.
    fn unrubberize(&self, displayed: Camera) -> Result<Camera, Error> {
        let clamped =
            sc::clamp_camera(displayed, self.stay_bounds()?, self.vp(), self.constraint());
        let v = self.vp();
        let b = self.stay_bounds()?;
        let axis = |disp_a: f64, clamped_a: f64, dim: f64, travels: bool| -> f64 {
            if !travels {
                return clamped_a;
            }
            let d_world = disp_a - clamped_a;
            if d_world == 0.0 {
                return clamped_a;
            }
            let d = js::max(1.0, dim);
            let out = js::min(d_world.abs() * displayed.zoom, d - 1.0);
            clamped_a + (js::sign(d_world) * rubber_in(out, d)) / displayed.zoom
        };
        Ok(Camera {
            zoom: displayed.zoom,
            x: axis(
                displayed.x,
                clamped.x,
                v.width,
                self.axis_travels(b.x, b.width, v.width, displayed.zoom),
            ),
            y: axis(
                displayed.y,
                clamped.y,
                v.height,
                self.axis_travels(b.y, b.height, v.height, displayed.zoom),
            ),
        })
    }

    /// The elastic write path: no clamp.
    fn set_cam_raw(&self, c: Camera) {
        self.dispatch(StageAction::Camera { camera: c });
    }

    /// Manipulation derives the cursor from the camera. Paged never syncs.
    fn sync_cursor_from_camera(&self) {
        if self.paged() {
            return;
        }
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return;
        }
        let page = sc::anchor_from_camera(self.cam(), &scene, self.vp(), None)
            .expect("a scene with items has an anchor")
            .page_index;
        if page != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: page });
        }
    }

    // ── camera tween ──────────────────────────────────────────────────────────

    fn cancel_anim(&self) {
        let raf = self.inner.raf.get();
        if raf != 0 {
            self.caf_call(raf);
            self.inner.raf.set(0);
        }
    }

    /// Tween to `target`, clamping every frame to `bounds`. `then` runs on
    /// natural completion only.
    fn animate_to(&self, target: Camera, bounds: Rect, then: Option<Rc<dyn Fn(&Self)>>) {
        if !self.can_animate() {
            self.set_cam(target, bounds);
            if let Some(then) = then {
                then(self);
            }
            return;
        }
        self.cancel_anim();
        let from = self.cam();
        let t0 = Rc::new(Cell::new(None::<f64>));
        let id = self.raf_call(self.tween_tick(from, target, bounds, t0, then));
        self.inner.raf.set(id);
    }

    fn tween_tick(
        &self,
        from: Camera,
        target: Camera,
        bounds: Rect,
        t0: Rc<Cell<Option<f64>>>,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            if !this.alive() {
                return;
            }
            let start = match t0.get() {
                Some(t) => t,
                None => {
                    t0.set(Some(now));
                    now
                }
            };
            let k = ease_out_cubic(js::min(1.0, (now - start) / TWEEN_MS));
            let lerp = |a: f64, b: f64| a + (b - a) * k;
            this.set_cam(
                Camera {
                    x: lerp(from.x, target.x),
                    y: lerp(from.y, target.y),
                    zoom: lerp(from.zoom, target.zoom),
                },
                bounds,
            );
            if k < 1.0 {
                let id = this.raf_call(this.tween_tick(from, target, bounds, t0, then));
                this.inner.raf.set(id);
            } else {
                this.inner.raf.set(0);
                if let Some(then) = then {
                    then(&this);
                }
            }
        })
    }

    /// Zoom-anchored tween: the zoom interpolates geometrically and each
    /// frame's camera derives from the focal anchor at that zoom.
    fn animate_zoom_anchored(
        &self,
        focal: Anchor,
        pt: Point,
        target_zoom: f64,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) {
        if !self.can_animate() {
            let scene = self.build_scene();
            if scene.item_count > 0 {
                self.set_cam_anchor_at_screen(focal, &scene, pt, target_zoom);
            }
            if let Some(then) = then {
                then(self);
            }
            return;
        }
        self.cancel_anim();
        let z0 = self.cam().zoom;
        let t0 = Rc::new(Cell::new(None::<f64>));
        let id = self.raf_call(self.zoom_tick(focal, pt, z0, target_zoom, t0, then));
        self.inner.raf.set(id);
    }

    /// `setCam(S.cameraForAnchorAtScreen(focal, scene, pt, zoom))`, whose
    /// default bounds exist whenever the scene has items.
    fn set_cam_anchor_at_screen(&self, focal: Anchor, scene: &Scene, pt: Point, zoom: f64) {
        let camera = sc::camera_for_anchor_at_screen(focal, scene, pt, zoom)
            .expect("a scene with items places an anchor");
        let bounds = self.stay_bounds().expect("a scene with items has bounds");
        self.set_cam(camera, bounds);
    }

    fn zoom_tick(
        &self,
        focal: Anchor,
        pt: Point,
        z0: f64,
        target_zoom: f64,
        t0: Rc<Cell<Option<f64>>>,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            if !this.alive() {
                return;
            }
            let start = match t0.get() {
                Some(t) => t,
                None => {
                    t0.set(Some(now));
                    now
                }
            };
            let k = ease_out_cubic(js::min(1.0, (now - start) / TWEEN_MS));
            let z = zoom_lerp(z0, target_zoom, k);
            let scene = this.build_scene();
            if scene.item_count > 0 {
                this.set_cam_anchor_at_screen(focal, &scene, pt, z);
            }
            if k < 1.0 {
                let id = this.raf_call(this.zoom_tick(focal, pt, z0, target_zoom, t0, then));
                this.inner.raf.set(id);
            } else {
                this.inner.raf.set(0);
                if let Some(then) = then {
                    then(&this);
                }
            }
        })
    }

    // ── momentum and edge physics ─────────────────────────────────────────────

    fn start_camera_physics(&self, vx_finger: f64, vy_finger: f64) -> Result<(), Error> {
        if !self.can_animate() {
            return self.set_cam_stay(self.cam()); // no host frames: snap into bounds
        }
        self.cancel_anim();
        let zoom = self.cam().zoom;
        let c0 = self.cam();
        let cl0 = sc::clamp_camera(c0, self.stay_bounds()?, self.vp(), self.constraint());
        let b0 = self.stay_bounds()?;
        let v0 = self.vp();
        let mk = |cam_a: f64, clamped_a: f64, finger_v: f64| AxisState {
            p: cam_a * zoom,
            v: -finger_v / 1000.0,
            springing: cam_a != clamped_a,
            done: false,
        };
        // A fitting axis takes no ballistic motion.
        let ax = mk(
            c0.x,
            cl0.x,
            if self.axis_travels(b0.x, b0.width, v0.width, zoom) { vx_finger } else { 0.0 },
        );
        let ay = mk(
            c0.y,
            cl0.y,
            if self.axis_travels(b0.y, b0.height, v0.height, zoom) { vy_finger } else { 0.0 },
        );
        if !ax.springing && !ay.springing && js::hypot(ax.v, ay.v) < FLING_STOP {
            return Ok(());
        }
        let state = Rc::new(RefCell::new((ax, ay, false, 0.0)));
        let id = self.raf_call(self.physics_tick(zoom, state));
        self.inner.raf.set(id);
        Ok(())
    }

    fn physics_tick(
        &self,
        zoom: f64,
        state: Rc<RefCell<(AxisState, AxisState, bool, f64)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            if !this.alive() {
                return;
            }
            this.inner.raf.set(0);
            let (mut ax, mut ay, started, last) = *state.borrow();
            // The first frame assumes one 60 Hz step.
            let dt = if started { js::min(64.0, js::max(1.0, now - last)) } else { 16.0 };
            // The stay bounds exist: the motion started against them.
            let Ok(bounds) = this.stay_bounds() else { return };
            let cl = sc::clamp_camera(
                Camera { x: ax.p / zoom, y: ay.p / zoom, zoom },
                bounds,
                this.vp(),
                this.constraint(),
            );
            let step = |a: &mut AxisState, edge_world: f64| {
                if a.done {
                    return;
                }
                let r = if a.springing {
                    spring_step(a.p, a.v, edge_world * zoom, dt)
                } else {
                    glide_step(a.p, a.v, dt)
                };
                a.p = r.p;
                a.v = r.v;
                a.done = r.done;
            };
            step(&mut ax, cl.x);
            step(&mut ay, cl.y);
            let bounds = this.stay_bounds().unwrap_or(bounds);
            let ncl = sc::clamp_camera(
                Camera { x: ax.p / zoom, y: ay.p / zoom, zoom },
                bounds,
                this.vp(),
                this.constraint(),
            );
            let convert = |a: &mut AxisState, clamped_a: f64| {
                if !a.springing && (clamped_a * zoom - a.p).abs() > 1e-6 {
                    a.p = clamped_a * zoom;
                    a.springing = true;
                    a.done = false;
                }
            };
            convert(&mut ax, ncl.x);
            convert(&mut ay, ncl.y);
            *state.borrow_mut() = (ax, ay, true, now);
            this.set_cam_raw(Camera { x: ax.p / zoom, y: ay.p / zoom, zoom });
            this.sync_cursor_from_camera();
            if ax.done && ay.done {
                let _ = this.set_cam_stay(this.cam()); // exact landing
                return;
            }
            let id = this.raf_call(this.physics_tick(zoom, state));
            this.inner.raf.set(id);
        })
    }

    // ── anchors ───────────────────────────────────────────────────────────────

    fn anchor_at(&self, at: Point) -> Anchor {
        let scene = self.build_scene();
        if scene.item_count > 0 {
            sc::anchor_from_camera(self.cam(), &scene, self.vp(), Some(at))
                .expect("a scene with items has an anchor")
        } else {
            Anchor { page_index: self.st().cursor, fx: 0.5, fy: 0.0 }
        }
    }

    fn current_anchor(&self) -> Anchor {
        self.anchor_at(self.anchor_point())
    }

    /// Re-run a placement until the scene it placed against is the scene
    /// that results (at most three passes).
    fn stabilized(&self, place: impl Fn()) {
        for _ in 0..3 {
            let scene = self.build_scene();
            place();
            if Rc::ptr_eq(&self.build_scene(), &scene) {
                return;
            }
        }
    }

    fn apply_anchor(&self, anchor: Anchor, align: Option<AlignmentValue>) {
        self.stabilized(|| {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return;
            }
            let item_index = scene.item_of_page(anchor.page_index).expect("a scene with items");
            let item = &scene.items[item_index];
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(item),
                self.vp(),
                self.pad(),
            );
            let at = self.align_point(align.unwrap_or(self.st().settings.anchor_align), self.vp());
            let camera = sc::camera_from_anchor(anchor, &scene, self.vp(), zoom, Some(at))
                .expect("a scene with items places an anchor");
            self.set_cam(camera, self.bounds_for(item));
        });
    }

    fn reapply(&self, anchor: Anchor, align: Option<AlignmentValue>) {
        if self.is_fit_all() {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else {
            self.apply_anchor(anchor, align);
        }
    }

    // ── navigation ────────────────────────────────────────────────────────────

    fn mark_cause(&self, cause: MotionCause) {
        if self.st().motion_cause != cause {
            self.dispatch(StageAction::MotionCause { cause });
        }
    }

    fn go_to_target(&self, page_index: isize, opts: Option<GoToOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        self.cancel_anim();
        let page_count = self.page_count_now();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        if target != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: target });
        }

        // Restore path: exact viewpoint instead of a fresh placement.
        if let Some(viewpoint) = opts.and_then(|o| o.viewpoint) {
            self.dispatch(StageAction::Patch { patch: PartialSettings::zoom(viewpoint.zoom) });
            self.apply_anchor(viewpoint.anchor, None);
            return;
        }

        let placement = || -> Option<Placement> {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &scene.items[0]
            } else {
                &scene.items[self.item_index_of_page(target)]
            };
            let zoom = sc::resolve_zoom(
                self.st().settings.zoom,
                self.fit_box(item),
                self.vp(),
                self.pad(),
            );
            let subject = if self.is_fit_all() {
                self.scene_rect()
            } else if self.fits(Self::item_rect(item), zoom) {
                Self::item_rect(item)
            } else {
                Self::page_rect_of(item, target)
            };
            let setting = self.st().settings.arrival_align;
            let over = opts.and_then(|o| o.arrival_align).unwrap_or_default();
            let want_x = over.x.unwrap_or(setting.x);
            let want_y = over.y.unwrap_or(setting.y);
            let align_of = |w: ArrivalAlignValue| match w {
                ArrivalAlignValue::Keep(_) => AlignValue::Named(Align::Start),
                ArrivalAlignValue::Align(a) => a,
            };
            let placed = sc::place_camera(
                subject,
                self.vp(),
                zoom,
                self.pad(),
                AlignmentValue { x: align_of(want_x), y: align_of(want_y) },
                self.st().settings.direction,
            );
            // Arrivals clamp within the true bounds even unbounded.
            let bounds = self.bounds_for(item);
            let settled = sc::clamp_camera(
                placed,
                bounds,
                self.vp(),
                CameraConstraint { bounded: true, ..self.constraint() },
            );
            let keep = |w: ArrivalAlignValue| matches!(w, ArrivalAlignValue::Keep(_));
            Some(Placement {
                camera: Camera {
                    zoom,
                    x: if keep(want_x) { self.cam().x } else { settled.x },
                    y: if keep(want_y) { self.cam().y } else { settled.y },
                },
                bounds,
            })
        };

        let behavior = opts.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior);
        if behavior == ScrollBehaviorKind::Smooth {
            if let Some(p) = placement() {
                self.animate_to(p.camera, p.bounds, None);
            }
        } else {
            self.stabilized(|| {
                if let Some(p) = placement() {
                    self.set_cam(p.camera, p.bounds);
                }
            });
        }
    }

    /// Step by the navigation unit: the item when it fits, else the page.
    fn step(&self, direction: isize, opts: Option<GoToOptions>) {
        let cursor = self.st().cursor;
        let Some(item) = self.cursor_item() else { return };
        let g = self.grouping();
        if self.is_fit_all() || self.fits(Self::item_rect(&item), self.cam().zoom) {
            let idx = (self.item_index_of_page(cursor) as isize + direction)
                .min(g.grouping.len() as isize - 1)
                .max(0) as usize;
            self.go_to_target(g.grouping[idx][0] as isize, opts);
        } else {
            self.go_to_target(cursor as isize + direction, opts);
        }
    }

    fn pon_for_index(&self, index: usize) -> PageObjectNumber {
        self.registry().get(index).map_or(index as PageObjectNumber + 1, |p| p.page_object_number)
    }

    /// Fallback un-rotated point size from a laid-out box.
    fn unrotated_points(b: &PageBox) -> Size {
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        Size { width: content.width / b.content_scale, height: content.height / b.content_scale }
    }

    fn with_transform(&self, b: &PageBox) -> VisiblePage {
        let registry = self.registry();
        let reg = registry.get(b.page_index);
        let page_size = reg.map_or_else(|| Self::unrotated_points(b), |r| r.size);
        let c = self.cam();
        let ratio = self.dpr();
        let raw_x = (b.x - c.x) * c.zoom;
        let raw_y = (b.y - c.y) * c.zoom;
        let resting = self.st().camera_resting;
        let screen_x = if resting { snap_to_device(raw_x, ratio) } else { raw_x };
        let screen_y = if resting { snap_to_device(raw_y, ratio) } else { raw_y };
        let transform = page_transform(PageTransformInput {
            page_size,
            rotation: b.rotation,
            scale: b.content_scale * c.zoom,
            base_scale: Some(
                self.st().settings.view_units_per_point
                    * reg.and_then(|r| r.user_unit).unwrap_or(1.0),
            ),
            dpr: ratio,
        })
        .expect("a page transform at a positive zoom is invertible");
        let v = self.vp();
        let on_screen = intersect_rects(
            Rect { x: -screen_x, y: -screen_y, width: v.width, height: v.height },
            Rect { x: 0.0, y: 0.0, width: transform.view_width, height: transform.view_height },
        );
        let visible_rect = if on_screen.width > 0.0 && on_screen.height > 0.0 {
            transform.view_to_content_rect(on_screen)
        } else {
            Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
        };
        VisiblePage {
            page: *b,
            pon: self.pon_for_index(b.page_index),
            screen_x,
            screen_y,
            transform,
            visible_rect,
        }
    }

    // ── scroller projection ───────────────────────────────────────────────────

    fn scroll_metrics_now(&self) -> ScrollMetrics {
        if !self.st().placed {
            return EMPTY_SCROLL_METRICS;
        }
        let scene = self.build_scene();
        let bounds = if scene.item_count > 0 {
            self.stay_bounds().expect("a scene with items has bounds")
        } else {
            Rect { x: 0.0, y: 0.0, width: 0.0, height: 0.0 }
        };
        let m = sc::scroll_metrics(self.cam(), bounds, self.vp(), self.pad());
        if let Some(p) = self.inner.scroll_memo.get()
            && p.scroll_left == m.scroll_left
            && p.scroll_top == m.scroll_top
            && p.scroll_width == m.scroll_width
            && p.scroll_height == m.scroll_height
            && p.client_width == m.client_width
            && p.client_height == m.client_height
        {
            return p;
        }
        self.inner.scroll_memo.set(Some(m));
        m
    }

    fn visible_pages_now(&self) -> Rc<Vec<VisiblePage>> {
        if !self.st().placed {
            return Rc::new(Vec::new());
        }
        let c = self.cam();
        let v = self.vp();
        let scene = self.build_scene();
        let items: Vec<&SceneItem> = if self.paged() {
            scene.items.iter().take(1).collect()
        } else {
            scene.query(sc::camera_world_rect(c, v))
        };
        let st = self.st();
        let sig = format!(
            "{}|{}|{},{},{}|{}x{}|{}|r{}",
            self.scene_cache_key(),
            serde_str(&st.settings.flow),
            num(c.x),
            num(c.y),
            num(c.zoom),
            num(v.width),
            num(v.height),
            num(self.dpr()),
            if st.camera_resting { 1 } else { 0 }
        );
        if self.inner.vis.borrow().0 == sig {
            return self.inner.vis.borrow().1.clone();
        }
        let vis: Vec<VisiblePage> =
            items.iter().flat_map(|it| it.pages.iter()).map(|b| self.with_transform(b)).collect();
        let vis = Rc::new(vis);
        *self.inner.vis.borrow_mut() = (sig, vis.clone());
        vis
    }

    fn snapshot_settings(&self) -> StageSettings {
        self.st().settings.clone()
    }

    /// A settings change, reacted per the registry: the strongest effect
    /// among the touched settings wins.
    fn apply_settings(&self, patch: PartialSettings) -> Result<(), Error> {
        self.cancel_anim();
        let touched = |effect: SettingEffect| {
            SETTING_KEYS.iter().any(|&k| patch_has(&patch, k) && settings_effect(k) == effect)
        };
        let align = if touched(SettingEffect::Refit)
            && !touched(SettingEffect::Scene)
            && !touched(SettingEffect::Reflow)
        {
            self.st().settings.zoom_align
        } else {
            self.st().settings.anchor_align
        };
        let anchor = self.anchor_at(self.align_point(align, self.vp()));
        let scene = touched(SettingEffect::Scene);
        let reflow = touched(SettingEffect::Reflow);
        let refit = touched(SettingEffect::Refit);
        let reclamp = touched(SettingEffect::Reclamp);
        self.dispatch(StageAction::Patch { patch });
        if scene {
            self.invalidate_scene();
        }
        if reflow {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else if scene || refit {
            self.reapply(anchor, Some(align));
        } else if reclamp {
            self.set_cam_stay(self.cam())?;
        }
        Ok(())
    }

    // ── responsive settings ───────────────────────────────────────────────────

    fn publish_active(&self, active: Vec<String>) {
        let prev = &self.st().active_rules;
        if prev.len() != active.len() || active.iter().zip(prev.iter()).any(|(n, p)| n != p) {
            self.dispatch(StageAction::Responsive { active });
        }
    }

    /// Re-resolve and apply what changed since the last resolution.
    fn sync_responsive(&self, react: bool) -> Result<SettingEffect, Error> {
        let rules = self.inner.rules.borrow().clone();
        let resolved = resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.vp()));
        let patch = diff_settings(&resolved.effective, &self.inner.last_effective.borrow());
        *self.inner.last_effective.borrow_mut() = resolved.effective;
        self.publish_active(resolved.active);
        let keys = patch_keys(&patch);
        if keys.is_empty() {
            return Ok(SettingEffect::None);
        }
        if react {
            self.apply_settings(patch)?;
            return Ok(SettingEffect::None);
        }
        self.dispatch(StageAction::Patch { patch });
        let strongest =
            keys.iter().map(|&k| settings_effect(k)).max().unwrap_or(SettingEffect::None);
        if strongest == SettingEffect::Scene || strongest == SettingEffect::Reflow {
            self.invalidate_scene();
        }
        Ok(strongest)
    }
}

/// The keys whose effective value changed, carrying the new value.
fn diff_settings(effective: &StageSettings, last: &StageSettings) -> PartialSettings {
    let mut patch = PartialSettings::default();
    macro_rules! arm {
        ($key:ident, $field:ident, $effect:ident) => {
            if !eq_setting(&effective.$field, &last.$field) {
                patch.$field = Some(effective.$field.clone());
            }
        };
    }
    crate::for_each_setting!(arm);
    patch
}

/// A unit enum's JSON string (`'vertical'`, `'odd'`, ...).
fn serde_str<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

const EMPTY_SCROLL_METRICS: ScrollMetrics = ScrollMetrics {
    scroll_left: 0.0,
    scroll_top: 0.0,
    scroll_width: 0.0,
    scroll_height: 0.0,
    client_width: 0.0,
    client_height: 0.0,
    scrollable_x: false,
    scrollable_y: false,
};

// ── the public surface ────────────────────────────────────────────────────────

impl StageCapability {
    // ── selectors ──

    pub fn camera(&self) -> Camera {
        self.cam()
    }

    pub fn viewport(&self) -> Size {
        self.vp()
    }

    /// The camera as a native scroller, over the pan clamp's travel range.
    /// Equal to the previous answer until a field moves.
    pub fn scroll_metrics(&self) -> ScrollMetrics {
        self.scroll_metrics_now()
    }

    pub fn page_count(&self) -> usize {
        self.page_count_now()
    }

    /// The laid-out pages on screen (paged: the slice's). The same `Rc`
    /// until the camera, scene, viewport, ratio or rest state changes.
    pub fn visible_pages(&self) -> Rc<Vec<VisiblePage>> {
        self.visible_pages_now()
    }

    /// The current page (the cursor), valid in both flows.
    pub fn current_page(&self) -> usize {
        self.st().cursor
    }

    /// The display indices of the current item's pages.
    pub fn current_item_pages(&self) -> Vec<usize> {
        self.cursor_item().map(|item| item.page_indexes).unwrap_or_default()
    }

    /// The page list with PDF labels.
    pub fn pages(&self) -> Vec<PageEntry> {
        self.registry()
            .iter()
            .map(|p| PageEntry {
                index: p.index,
                pon: p.page_object_number,
                label: p.label.clone(),
            })
            .collect()
    }

    /// The laid-out box for a page by its durable pon.
    pub fn page_rect(&self, pon: PageObjectNumber) -> Option<VisiblePage> {
        if !self.st().placed {
            return None;
        }
        let index = self.registry().iter().position(|p| p.page_object_number == pon)?;
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return None;
        }
        let item = &scene.items[scene.item_of_page(index)?];
        let b = item.pages.iter().find(|p| p.page_index == index)?;
        Some(self.with_transform(b))
    }

    /// Screen point to the page under it and its content point; `None` over
    /// a gap.
    pub fn page_at(&self, screen: Point) -> Option<PageHit> {
        for p in self.visible_pages_now().iter() {
            let lx = screen.x - p.screen_x;
            let ly = screen.y - p.screen_y;
            if lx >= 0.0
                && ly >= 0.0
                && lx <= p.transform.view_width
                && ly <= p.transform.view_height
            {
                return Some(PageHit {
                    pon: p.pon,
                    point: p.transform.view_to_content(Point { x: lx, y: ly }),
                    scale: p.transform.view_scale,
                    rotation: p.page.rotation,
                    zoom: p.transform.zoom,
                });
            }
        }
        None
    }

    /// Screen point to `pon`'s content space, unclamped; `None` when the
    /// page is not laid out on screen.
    pub fn point_on_page(&self, pon: PageObjectNumber, screen: Point) -> Option<Point> {
        let vis = self.visible_pages_now();
        let p = vis.iter().find(|v| v.pon == pon)?;
        Some(
            p.transform
                .view_to_content(Point { x: screen.x - p.screen_x, y: screen.y - p.screen_y }),
        )
    }

    /// Page space (intrinsic PDF points) to world space.
    pub fn page_to_world(&self, pon: PageObjectNumber, pt: Point) -> Option<Point> {
        let pr = self.page_rect(pon)?;
        let b = pr.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let offset = apply_point(m, pt);
        Some(Point { x: b.x + offset.x, y: b.y + offset.y })
    }

    /// Content rect on a page to this viewport's screen-space AABB.
    pub fn page_rect_to_screen(&self, pon: PageObjectNumber, rect: Rect) -> Option<Rect> {
        let pr = self.page_rect(pon)?;
        let b = pr.page;
        let content = display_size(Size { width: b.width, height: b.height }, b.rotation);
        let m = rotate_scale_matrix(b.content_scale, content.width, content.height, b.rotation);
        let wr = apply_rect(m, rect);
        let c = self.cam();
        let tl = sc::to_screen(c, Point { x: b.x + wr.x, y: b.y + wr.y });
        Some(Rect { x: tl.x, y: tl.y, width: wr.width * c.zoom, height: wr.height * c.zoom })
    }

    pub fn to_screen(&self, world: Point) -> Point {
        sc::to_screen(self.cam(), world)
    }

    pub fn to_world(&self, screen: Point) -> Point {
        sc::to_world(self.cam(), screen)
    }

    pub fn flow(&self) -> FlowMode {
        self.st().settings.flow
    }
    pub fn layout(&self) -> LayoutKind {
        self.st().settings.layout
    }
    pub fn spread(&self) -> sc::SpreadMode {
        self.st().settings.spread
    }
    pub fn sizing(&self) -> sc::SizingMode {
        self.st().settings.sizing
    }
    pub fn columns(&self) -> GridColumns {
        self.st().settings.columns
    }
    pub fn bounded(&self) -> bool {
        self.st().settings.bounded
    }
    pub fn padding(&self) -> f64 {
        self.st().settings.padding
    }
    pub fn gap(&self) -> Gap {
        self.st().settings.gap
    }
    pub fn page_frame(&self) -> epdf_geometry::PageFrame {
        self.st().settings.page_frame
    }
    pub fn fit_align(&self) -> sc::Alignment {
        self.st().settings.fit_align
    }
    pub fn arrival_align(&self) -> ArrivalAlignment {
        self.st().settings.arrival_align
    }
    pub fn zoom_align(&self) -> AlignmentValue {
        self.st().settings.zoom_align
    }
    pub fn anchor_align(&self) -> AlignmentValue {
        self.st().settings.anchor_align
    }
    pub fn direction(&self) -> Direction {
        self.st().settings.direction
    }
    pub fn scroll_behavior(&self) -> ScrollBehaviorKind {
        self.st().settings.scroll_behavior
    }
    /// The lens's non-persistent view rotation.
    pub fn view_rotation(&self) -> PageRotation {
        self.st().settings.view_rotation
    }
    pub fn zoom_level(&self) -> f64 {
        self.cam().zoom
    }

    /// The active zoom intent's mode, `None` for `'custom'` (a level or a
    /// page-width or page-height spec).
    pub fn zoom_mode(&self) -> Option<ZoomMode> {
        match self.st().settings.zoom {
            ZoomSpec::Mode { mode } => Some(mode),
            _ => None,
        }
    }

    /// What I'm looking at plus the zoom intent.
    pub fn viewpoint(&self) -> Viewpoint {
        Viewpoint { anchor: self.current_anchor(), zoom: self.st().settings.zoom }
    }

    pub fn settings(&self) -> StageSettings {
        self.snapshot_settings()
    }

    pub fn view_state(&self) -> StageViewState {
        StageViewState {
            settings: self.snapshot_settings(),
            cursor: Some(self.st().cursor),
            anchor: self.current_anchor(),
        }
    }

    // ── intents ──

    /// Report the viewport. The first report with both axes positive and a
    /// document with pages runs initial placement; later ones keep the
    /// anchor and re-resolve fit modes.
    pub fn set_viewport(&self, v: Size) {
        if !self.inner.placement_started.get() {
            self.dispatch(StageAction::Vp { vp: v });
            // Rules see the box before placement resolves. With react false
            // nothing clamps, so this cannot fail.
            let _ = self.sync_responsive(false);
            if v.width > 0.0 && v.height > 0.0 && self.page_count_now() > 0 {
                self.place_initial();
            }
            return;
        }
        self.cancel_anim();
        let anchor = self.current_anchor(); // against the old viewport
        self.dispatch(StageAction::Vp { vp: v });
        let fx = self.sync_responsive(false).unwrap_or(SettingEffect::None);
        if fx == SettingEffect::Reflow {
            self.go_to_target(
                self.st().cursor as isize,
                Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)),
            );
        } else {
            self.reapply(anchor, None);
        }
    }

    /// Report the device pixel ratio. Only a positive, changed ratio lands.
    pub fn set_device_pixel_ratio(&self, ratio: f64) {
        if ratio > 0.0 && ratio != self.dpr() {
            self.dispatch(StageAction::Dpr { dpr: ratio });
        }
    }

    pub fn set_camera(&self, c: Camera) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        self.set_cam_stay(c)?;
        self.sync_cursor_from_camera();
        Ok(())
    }

    pub fn pan_by(&self, dx: f64, dy: f64) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        if self.inner.gesture_depth.get() > 0 && self.inner.gesture_elastic.get() {
            // Elastic: integrate the finger on the unclamped camera.
            let raw = match self.inner.gesture_raw.get() {
                Some(raw) => raw,
                None => self.unrubberize(self.cam())?,
            };
            let raw = sc::pan_by_screen(raw, dx, dy);
            self.inner.gesture_raw.set(Some(raw));
            self.set_cam_raw(self.rubberize(raw)?);
        } else {
            self.set_cam_stay(sc::pan_by_screen(self.cam(), dx, dy))?;
        }
        self.sync_cursor_from_camera();
        Ok(())
    }

    /// `Element.scrollTo` for the camera: absolute, clamped, cursor-synced.
    pub fn scroll_to(&self, opts: StageScrollToOptions) {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return;
        }
        let bounds = self.stay_bounds().expect("a scene with items has bounds");
        let next = sc::camera_from_scroll(
            self.cam(),
            bounds,
            self.vp(),
            self.pad(),
            sc::ScrollTarget { left: opts.left, top: opts.top },
        );
        if opts.behavior == Some(ScrollBehaviorKind::Smooth) {
            let bounds = self.stay_bounds().expect("a scene with items has bounds");
            self.animate_to(next, bounds, Some(Rc::new(|s: &Self| s.sync_cursor_from_camera())));
        } else {
            self.set_cam(next, bounds);
            self.sync_cursor_from_camera();
        }
    }

    /// `Element.scrollBy`: relative offsets over `scroll_to`.
    pub fn scroll_by(&self, opts: StageScrollToOptions) {
        let m = self.scroll_metrics_now();
        self.scroll_to(StageScrollToOptions {
            left: opts.left.map(|left| m.scroll_left + left),
            top: opts.top.map(|top| m.scroll_top + top),
            behavior: opts.behavior,
        });
    }

    pub fn zoom_around(&self, pt: Point, factor: f64) -> Result<(), Error> {
        self.cancel_anim();
        let before = self.build_scene();
        let focal = if before.item_count > 0 {
            Some(
                sc::anchor_at_point(&before, sc::to_world(self.cam(), pt))
                    .expect("a scene with items has an anchor"),
            )
        } else {
            None
        };
        self.set_cam_stay(sc::zoom_around(self.cam(), pt, factor))?;
        if self.inner.gesture_depth.get() == 0 {
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: self.cam().zoom }),
            });
        }
        // A wrapped grid re-wrapped under the camera: re-pin the focal point.
        let after = self.build_scene();
        if let Some(focal) = focal
            && !Rc::ptr_eq(&after, &before)
            && after.item_count > 0
        {
            self.set_cam_anchor_at_screen(focal, &after, pt, self.cam().zoom);
        }
        if self.inner.gesture_depth.get() > 0 && self.inner.gesture_elastic.get() {
            self.inner.gesture_raw.set(Some(self.unrubberize(self.cam())?));
        }
        self.sync_cursor_from_camera();
        Ok(())
    }

    /// Bracket a continuous gesture; re-entrant. Opening one catches any
    /// running tween or fling.
    pub fn begin_gesture(&self, options: Option<GestureOptions>) {
        self.inner.gesture_depth.set(self.inner.gesture_depth.get() + 1);
        if self.inner.gesture_depth.get() == 1 {
            self.cancel_anim();
            self.inner.gesture_zoomed.set(false);
            self.inner.gesture_elastic.set(options.and_then(|o| o.elastic) == Some(true));
            self.inner.gesture_raw.set(None);
        }
    }

    pub fn end_gesture(&self) -> Result<(), Error> {
        if self.inner.gesture_depth.get() == 0 {
            return Ok(());
        }
        self.inner.gesture_depth.set(self.inner.gesture_depth.get() - 1);
        if self.inner.gesture_depth.get() > 0 {
            return Ok(());
        }
        self.inner.gesture_raw.set(None);
        let was_elastic = self.inner.gesture_elastic.get();
        self.inner.gesture_elastic.set(false);
        if self.inner.gesture_zoomed.get() {
            self.inner.gesture_zoomed.set(false);
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: self.cam().zoom }),
            });
            self.arm_rest();
        }
        self.sync_cursor_from_camera();
        if was_elastic {
            let c = self.cam();
            let cl = sc::clamp_camera(c, self.stay_bounds()?, self.vp(), self.constraint());
            if cl.x != c.x || cl.y != c.y {
                self.start_camera_physics(0.0, 0.0)?;
            }
        }
        Ok(())
    }

    /// Momentum scroll from a release velocity (screen px/s, `pan_by`'s
    /// sign convention). Without frames it only snaps into bounds.
    pub fn fling(&self, vx: f64, vy: f64) -> Result<(), Error> {
        self.start_camera_physics(vx, vy)
    }

    /// True while a tween or fling is running.
    pub fn camera_in_motion(&self) -> bool {
        self.inner.raf.get() != 0
    }

    /// The double-tap ladder: automatic, fit-width, 2.5 times automatic,
    /// wrapping; off the ladder it resets to the base rung.
    pub fn double_tap_zoom(&self, pt: Point) {
        self.cancel_anim();
        let sc0 = self.build_scene();
        if sc0.item_count == 0 {
            return;
        }
        let item = if self.paged() {
            &sc0.items[0]
        } else {
            &sc0.items[self.item_index_of_page(self.st().cursor)]
        };
        let fit = self.fit_box(item);
        let auto = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::Automatic },
            fit,
            self.vp(),
            self.pad(),
        );
        let fit_w = sc::resolve_zoom(
            ZoomSpec::Mode { mode: ZoomMode::FitWidth },
            fit,
            self.vp(),
            self.pad(),
        );
        let mut sorted = [auto, fit_w, js::min(auto * 2.5, sc::ZOOM_MAX)];
        // `Array#sort` with `(a, b) => a - b`: stable, and a NaN comparison
        // reads as equal.
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut stops = Vec::new();
        for (i, &z) in sorted.iter().enumerate() {
            if i == 0 || z > sorted[i - 1] * 1.1 {
                stops.push(z);
            }
        }
        let near = |z: f64, stop: f64| z > stop / 1.1 && z < stop * 1.1;
        let zoom = self.cam().zoom;
        let target = match stops.iter().position(|&s| near(zoom, s)) {
            Some(on_rung) => stops[(on_rung + 1) % stops.len()],
            None => stops[0],
        };
        // The intent commits up front.
        self.dispatch(StageAction::Patch {
            patch: PartialSettings::zoom(ZoomSpec::Level { level: target }),
        });
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return;
        }
        let focal = sc::anchor_at_point(&scene, sc::to_world(self.cam(), pt))
            .expect("a scene with items has an anchor");
        self.animate_zoom_anchored(
            focal,
            pt,
            target,
            Some(Rc::new(|s: &Self| s.sync_cursor_from_camera())),
        );
    }

    /// Magnify by 1.2 around the zoomAlign point.
    pub fn zoom_in(&self) -> Result<(), Error> {
        self.zoom_around(self.align_point(self.st().settings.zoom_align, self.vp()), 1.2)
    }

    pub fn zoom_out(&self) -> Result<(), Error> {
        self.zoom_around(self.align_point(self.st().settings.zoom_align, self.vp()), 1.0 / 1.2)
    }

    pub fn zoom_to(&self, spec: ZoomSpec) -> Result<(), Error> {
        self.update(PartialSettings::zoom(spec))
    }

    pub fn fit_width(&self) -> Result<(), Error> {
        self.zoom_to(ZoomSpec::Mode { mode: ZoomMode::FitWidth })
    }

    pub fn fit_page(&self) -> Result<(), Error> {
        self.zoom_to(ZoomSpec::Mode { mode: ZoomMode::FitPage })
    }

    /// Fit the whole scene.
    pub fn fit_all(&self) -> Result<(), Error> {
        self.zoom_to(ZoomSpec::Mode { mode: ZoomMode::FitAll })
    }

    /// Fit width but never past 100%.
    pub fn automatic(&self) -> Result<(), Error> {
        self.zoom_to(ZoomSpec::Mode { mode: ZoomMode::Automatic })
    }

    /// Re-resolve the zoom intent against the current scene, keeping the
    /// anchor. A no-op before the first placement. The effects call it on
    /// every page-registry revision.
    pub fn refit(&self) {
        if !self.inner.placement_started.get() {
            return;
        }
        self.cancel_anim();
        self.reapply(self.current_anchor(), None);
    }

    /// Go to a page (clamped into range). A viewpoint restores; otherwise a
    /// fresh arrival at arrivalAlign.
    pub fn go_to_page(&self, page_index: isize, opts: Option<GoToOptions>) {
        self.go_to_target(page_index, opts);
    }

    /// The follower-UI arrival verb. Bare: minimal movement, cursor
    /// untouched. Positioned (rect, zoom or anchor set): place the target,
    /// optionally re-zooming, the cursor following.
    pub fn reveal(&self, page_index: isize, opts: Option<RevealOptions>) {
        self.mark_cause(MotionCause::Programmatic);
        let page_count = self.page_count_now();
        if page_count == 0 {
            return;
        }
        let target = page_index.clamp(0, page_count as isize - 1) as usize;
        let positioned = opts.is_some_and(|o| {
            o.rect.is_some()
                || o.anchor.is_some()
                || o.zoom.is_some_and(|z| z != RevealZoom::Word(RevealZoomWord::Keep))
        });
        let behavior_of = |o: Option<RevealOptions>| {
            o.and_then(|o| o.behavior).unwrap_or(self.st().settings.scroll_behavior)
        };

        if !positioned {
            if self.paged() {
                self.go_to_target(
                    target as isize,
                    opts.map(|o| GoToOptions { behavior: o.behavior, ..Default::default() }),
                );
                return;
            }
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return;
            }
            let page = Self::page_rect_of(&scene.items[self.item_index_of_page(target)], target);
            // Reveal the outer box: the pageFrame bands belong to the page.
            let m = self.world_page_frame();
            let bx = Rect {
                x: page.x - m.left,
                y: page.y - m.top,
                width: page.width + m.left + m.right,
                height: page.height + m.top + m.bottom,
            };
            let camera = sc::reveal_camera(self.cam(), bx, self.vp(), self.pad());
            let current = self.cam();
            if camera.x == current.x && camera.y == current.y {
                return;
            }
            self.cancel_anim();
            if behavior_of(opts) == ScrollBehaviorKind::Smooth {
                self.animate_to(camera, self.scene_rect(), None);
            } else {
                self.set_cam(camera, self.scene_rect());
            }
            return;
        }

        let opts = opts.unwrap_or_default();
        self.cancel_anim();
        if target != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: target });
        }

        let place = || -> Option<RevealPlacement> {
            let scene = self.build_scene();
            if scene.item_count == 0 {
                return None;
            }
            let item = if self.paged() {
                &scene.items[0]
            } else {
                &scene.items[self.item_index_of_page(target)]
            };
            let world = match opts.rect {
                Some(rect) => Self::world_rect_for_content(item, target, rect),
                None => Self::page_rect_of(item, target),
            };
            let zd = opts.zoom.unwrap_or(RevealZoom::Word(RevealZoomWord::Keep));
            let avail_w = js::max(1.0, self.vp().width - 2.0 * self.pad());
            let avail_h = js::max(1.0, self.vp().height - 2.0 * self.pad());
            let mut zoom = match zd {
                RevealZoom::Level { level } => level,
                RevealZoom::Word(RevealZoomWord::Fit) => {
                    js::min(avail_w / world.width, avail_h / world.height)
                }
                RevealZoom::Word(RevealZoomWord::FitWidth) => avail_w / world.width,
                RevealZoom::Word(RevealZoomWord::FitHeight) => avail_h / world.height,
                RevealZoom::Word(RevealZoomWord::Keep) => self.cam().zoom,
            };
            // A degenerate target with a fit directive pans only.
            if !zoom.is_finite() || zoom <= 0.0 {
                zoom = self.cam().zoom;
            }
            let zoom_changed = zd != RevealZoom::Word(RevealZoomWord::Keep);
            let a = opts.anchor.unwrap_or_default();
            let c = self.cam();
            let v = self.vp();
            Some(RevealPlacement {
                camera: Camera {
                    x: self.reveal_axis(
                        a.x,
                        c.x,
                        world.x,
                        world.width,
                        v.width,
                        zoom,
                        zoom_changed,
                    ),
                    y: self.reveal_axis(
                        a.y,
                        c.y,
                        world.y,
                        world.height,
                        v.height,
                        zoom,
                        zoom_changed,
                    ),
                    zoom,
                },
                bounds: self.bounds_for(item),
                zoom_changed,
            })
        };

        let Some(first) = place() else { return };
        if first.zoom_changed {
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: first.camera.zoom }),
            });
        }
        if behavior_of(Some(opts)) == ScrollBehaviorKind::Smooth {
            let p = place().unwrap_or(first);
            self.animate_to(p.camera, p.bounds, None);
        } else {
            self.stabilized(|| {
                if let Some(p) = place() {
                    self.set_cam(p.camera, p.bounds);
                }
            });
        }
    }

    /// Step forward by the navigation unit.
    pub fn next(&self, opts: Option<GoToOptions>) {
        self.step(1, opts);
    }

    /// Step backward by the navigation unit.
    pub fn prev(&self, opts: Option<GoToOptions>) {
        self.step(-1, opts);
    }

    /// The stage plugin id this lens registered under.
    pub fn lens_id(&self) -> String {
        self.ctx.id().to_string()
    }

    /// Set any subset of settings at once: one anchor-preserving update
    /// that writes the responsive base.
    pub fn update(&self, patch: PartialSettings) -> Result<(), Error> {
        let merged = merge_settings(&self.inner.base.borrow(), &patch);
        *self.inner.base.borrow_mut() = merged;
        self.sync_responsive(true).map(|_| ())
    }

    /// Replace the responsive rules and re-resolve.
    pub fn set_responsive(&self, rules: Vec<ResponsiveRule>) -> Result<(), Error> {
        *self.inner.rules.borrow_mut() = Rc::new(rules);
        self.sync_responsive(true).map(|_| ())
    }

    /// Is the named rule matching?
    pub fn matches(&self, name: &str) -> bool {
        self.st().active_rules.iter().any(|n| n == name)
    }

    /// Names of the matching rules, in source order.
    pub fn active_rules(&self) -> Vec<String> {
        self.st().active_rules.clone()
    }

    pub fn set_flow(&self, flow: FlowMode) -> Result<(), Error> {
        self.update(PartialSettings { flow: Some(flow), ..Default::default() })
    }
    pub fn set_layout(&self, layout: LayoutKind) -> Result<(), Error> {
        self.update(PartialSettings { layout: Some(layout), ..Default::default() })
    }
    pub fn set_spread(&self, spread: sc::SpreadMode) -> Result<(), Error> {
        self.update(PartialSettings { spread: Some(spread), ..Default::default() })
    }
    pub fn set_sizing(&self, sizing: sc::SizingMode) -> Result<(), Error> {
        self.update(PartialSettings { sizing: Some(sizing), ..Default::default() })
    }
    pub fn set_columns(&self, columns: GridColumns) -> Result<(), Error> {
        self.update(PartialSettings { columns: Some(columns), ..Default::default() })
    }
    pub fn set_bounded(&self, bounded: bool) -> Result<(), Error> {
        self.update(PartialSettings { bounded: Some(bounded), ..Default::default() })
    }
    pub fn set_padding(&self, padding: f64) -> Result<(), Error> {
        self.update(PartialSettings { padding: Some(padding), ..Default::default() })
    }
    pub fn set_gap(&self, gap: Gap) -> Result<(), Error> {
        self.update(PartialSettings { gap: Some(gap), ..Default::default() })
    }
    pub fn set_page_frame(&self, page_frame: epdf_geometry::PageFrame) -> Result<(), Error> {
        self.update(PartialSettings { page_frame: Some(page_frame), ..Default::default() })
    }
    pub fn set_fit_align(&self, fit_align: sc::Alignment) -> Result<(), Error> {
        self.update(PartialSettings { fit_align: Some(fit_align), ..Default::default() })
    }
    pub fn set_arrival_align(&self, arrival_align: ArrivalAlignment) -> Result<(), Error> {
        self.update(PartialSettings { arrival_align: Some(arrival_align), ..Default::default() })
    }
    pub fn set_zoom_align(&self, zoom_align: AlignmentValue) -> Result<(), Error> {
        self.update(PartialSettings { zoom_align: Some(zoom_align), ..Default::default() })
    }
    pub fn set_anchor_align(&self, anchor_align: AlignmentValue) -> Result<(), Error> {
        self.update(PartialSettings { anchor_align: Some(anchor_align), ..Default::default() })
    }
    pub fn set_direction(&self, direction: Direction) -> Result<(), Error> {
        self.update(PartialSettings { direction: Some(direction), ..Default::default() })
    }
    /// Set the view rotation to an absolute quarter-turn.
    pub fn set_view_rotation(&self, view_rotation: PageRotation) -> Result<(), Error> {
        self.update(PartialSettings { view_rotation: Some(view_rotation), ..Default::default() })
    }
    /// Rotate the view a quarter-turn: 90 clockwise, anything else counter.
    pub fn rotate_view(&self, delta: f64) -> Result<(), Error> {
        let turn = if delta == 90.0 { PageRotation::DEG_90 } else { PageRotation::DEG_270 };
        self.set_view_rotation(add_rotations(self.st().settings.view_rotation, turn))
    }
    pub fn set_scroll_behavior(&self, behavior: ScrollBehaviorKind) -> Result<(), Error> {
        self.update(PartialSettings { scroll_behavior: Some(behavior), ..Default::default() })
    }

    /// Restore a saved view: it writes the base and the rules re-assert on
    /// top, in one PATCH, then the cursor and the anchor.
    pub fn apply_view_state(&self, view: StageViewState) {
        self.cancel_anim();
        let merged = merge_settings(
            &self.inner.base.borrow(),
            &PartialSettings::from(view.settings.clone()),
        );
        *self.inner.base.borrow_mut() = merged;
        let rules = self.inner.rules.borrow().clone();
        let resolved = resolve_responsive(&self.inner.base.borrow(), &rules, &box_of(self.vp()));
        *self.inner.last_effective.borrow_mut() = resolved.effective.clone();
        self.publish_active(resolved.active);
        self.dispatch(StageAction::Patch { patch: PartialSettings::from(resolved.effective) });
        self.dispatch(StageAction::Cursor { cursor: view.cursor.unwrap_or(0) });
        self.invalidate_scene();
        self.apply_anchor(view.anchor, None);
    }

    /// Offer a candidate initial view; the highest priority non-null one
    /// wins at placement.
    pub fn provide_initial_view(
        &self,
        priority: f64,
        provider: impl Fn() -> Option<StageViewState> + 'static,
    ) {
        self.inner.providers.borrow_mut().push((priority, Rc::new(provider)));
    }

    /// Resolve the providers once (else reset), then publish PLACED.
    pub fn place_initial(&self) {
        self.inner.placement_started.set(true);
        let mut sorted = self.inner.providers.borrow().clone();
        // `Array#sort` with `(a, b) => b.priority - a.priority`: stable.
        sorted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, provider) in sorted {
            if let Some(view) = provider() {
                self.apply_view_state(view);
                self.dispatch(StageAction::Placed);
                return;
            }
        }
        self.reset_view();
        self.dispatch(StageAction::Placed);
    }

    /// Back to page 0, placed fresh.
    pub fn reset_view(&self) {
        self.go_to_target(0, Some(GoToOptions::behavior(ScrollBehaviorKind::Instant)));
    }
}
