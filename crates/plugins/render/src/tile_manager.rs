//! `tile-manager.ts`: the tile retention state machine. One per document,
//! all levels, pages and views in one place, over the same [`RasterStore`]
//! the base renders use.
//!
//! The invariant: every screen region paints the sharpest PAINTED pixels
//! available, and quality per region only goes up until the want set
//! resolves. `plan` schedules the want set's fetches (visible tiles first,
//! center-out, then the prefetch ring once they are all resolved) and
//! returns a paint list drawn only from resolved entries, current level and
//! retained older generations. A retained generation leaves the list when the
//! want tiles covering its visible footprint report painted; an epoch bump
//! drops everything at once.
//!
//! `plan` must not dispatch: it is called from selectors. Resolutions wake
//! the layers through `on_advance`.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise, TimerId};
use epdf_kernel::signal::AbortSignal;
use indexmap::IndexMap;
use serde_json::Value;

use crate::js::{self, number_to_string as num};
use crate::paint_plan::{
    PageViewDemand, ResolvedRenderOptions, TilePaintPlan, TilePaintSource, empty_tile_plan,
};
use crate::policy::EngineRenderPolicy;
use crate::raster_store::RasterStore;
use crate::strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
use crate::tiles::{
    PageSizePt, TileCoord, TileGrid, bleed_rect, inflate_rect, intersect_rects, region_covered,
    snap_to_pyramid, tile_grid, tile_paint_rect, tiles_in_rect,
};

/// Backpressure: raw rasters in flight at once.
const MAX_IN_FLIGHT: i64 = 8;
/// Stage-less (no visibleRect) demand is capped to about this many tiles.
const STAGELESS_TILE_CAP: f64 = 64.0;

/// `fetchTile(pon, rect, scale, includeAnnotations, signal)`: render a
/// page-space region (y-down page points); the owner converts it to PDF
/// space. `Err` is a synchronous throw. The promise answers the engine's
/// `PageImageHandle` JSON.
pub type FetchTile =
    Rc<dyn Fn(u32, Rect, f64, bool, &AbortSignal) -> Result<Promise<Value>, Error>>;

/// What the manager needs from its owner. The capability supplies these; a
/// host embedding the manager on its own supplies its own.
#[derive(Clone)]
pub struct TileManagerDeps {
    pub store: RasterStore,
    pub options: Rc<ResolvedRenderOptions>,
    /// The document fact off the kernel registry.
    pub get_policy: Rc<dyn Fn() -> EngineRenderPolicy>,
    pub get_page_size: Rc<dyn Fn(u32) -> Option<PageSizePt>>,
    pub get_epoch: Rc<dyn Fn(u32, bool) -> f64>,
    pub fetch_tile: FetchTile,
    /// Wake subscribed layers (the owner re-plans and dispatches
    /// `paintAdvanced` outside `plan`).
    pub on_advance: Rc<dyn Fn(u32)>,
    /// Diagnostic sink (`options.debug`).
    pub debug: Option<Rc<dyn Fn(&str)>>,
    /// `console.warn`, which the TypeScript calls directly for the stage-less
    /// clamp.
    pub warn: Rc<dyn Fn(&str)>,
}

struct TileEntry {
    key: String,
    scale: f64,
    /// y-down page points, unbled.
    rect: Rect,
    resolved: Cell<bool>,
    painted: Cell<bool>,
    /// Holds an in-flight transit slot.
    charged: Cell<bool>,
    abort: AbortSignal,
}

struct PlanCache {
    demand_key: String,
    version: u64,
    plan: Rc<TilePaintPlan>,
}

struct PageTileState {
    epoch: f64,
    want_scale: Option<f64>,
    /// The want level's identity: integer device px across the page.
    want_width: Option<f64>,
    entries: IndexMap<String, Rc<TileEntry>>,
    /// Keys that failed (not aborted) at the current level.
    failed_keys: HashSet<String>,
    version: u64,
    plan_cache: Option<PlanCache>,
    settle_timer: Option<TimerId>,
    pending_level: Option<f64>,
    last_visible: Option<Rect>,
}

type State = Rc<RefCell<PageTileState>>;

struct Inner {
    /// Per view-of-page, keyed `view\0pon`.
    pages: RefCell<HashMap<String, State>>,
    strategy_memo: RefCell<Option<(EngineRenderPolicy, Rc<ResolvedStrategy>)>>,
    in_flight: Cell<i64>,
    warned_stageless: Cell<bool>,
    deps: TileManagerDeps,
}

/// `TileManager`. Cheap to clone; clones share the manager.
#[derive(Clone)]
pub struct TileManager(Rc<Inner>);

/// `stats(view, pon)`: bookkeeping size, for diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileStats {
    pub entries: usize,
    pub in_flight: i64,
}

fn state_key(view: &str, pon: u32) -> String {
    format!("{view}\u{0}{pon}")
}

fn coord_key(c: TileCoord) -> String {
    format!("{},{}", num(c.ix), num(c.iy))
}

fn rect_key(r: Rect) -> String {
    format!(
        "{},{},{},{}",
        num(js::round(r.x)),
        num(js::round(r.y)),
        num(js::round(r.width)),
        num(js::round(r.height))
    )
}

fn annotations_of(key: &str) -> bool {
    key.contains("|a1|")
}

impl TileManager {
    pub fn new(deps: TileManagerDeps) -> Self {
        TileManager(Rc::new(Inner {
            pages: RefCell::new(HashMap::new()),
            strategy_memo: RefCell::new(None),
            in_flight: Cell::new(0),
            warned_stageless: Cell::new(false),
            deps,
        }))
    }

    fn deps(&self) -> &TileManagerDeps {
        &self.0.deps
    }

    fn debug(&self, message: impl FnOnce() -> String) {
        if let Some(debug) = &self.deps().debug {
            debug(&message());
        }
    }

    fn strategy(&self) -> Rc<ResolvedStrategy> {
        let policy = (self.deps().get_policy)();
        let mut memo = self.0.strategy_memo.borrow_mut();
        match &*memo {
            Some((p, strategy)) if *p == policy => strategy.clone(),
            _ => {
                let strategy = Rc::new(resolve_strategy(&policy, &self.deps().options));
                *memo = Some((policy, strategy.clone()));
                strategy
            }
        }
    }

    /// The tile paint plan for one view of a page. `Err` where the TypeScript
    /// throws out of `plan`: the engine refusing a page the registry lists.
    pub fn plan(
        &self,
        view: &str,
        pon: u32,
        demand: &PageViewDemand,
        include_annotations: bool,
    ) -> Result<Rc<TilePaintPlan>, Error> {
        let options = self.deps().options.clone();
        if !options.tiles.enabled {
            return Ok(empty_tile_plan());
        }
        let strategy = self.strategy();
        let Some(page) = (self.deps().get_page_size)(pon) else { return Ok(empty_tile_plan()) };

        let epoch = (self.deps().get_epoch)(pon, include_annotations);
        let state = self.page_state(pon, view);

        // Epoch exception: old-epoch pixels are wrong, not blurry.
        if state.borrow().epoch != epoch {
            self.abort_all(&state);
            let mut s = state.borrow_mut();
            s.entries.clear();
            s.failed_keys.clear();
            s.epoch = epoch;
            s.want_scale = None;
            s.want_width = None;
            s.plan_cache = None;
        }

        // Engagement: the deficit of what the base actually supplies.
        let supplied = base_ask_width(&strategy, demand.desired_device_width);
        let deficit = demand.desired_device_width / supplied;
        if deficit <= strategy.engage_at {
            let engaged = {
                let s = state.borrow();
                !s.entries.is_empty() || s.want_scale.is_some()
            };
            if engaged {
                self.debug(|| {
                    format!("disengage pon={pon} (deficit {})", js::to_fixed(deficit, 2))
                });
                self.abort_all(&state);
                let mut s = state.borrow_mut();
                s.entries.clear();
                s.failed_keys.clear();
                s.want_scale = None;
                s.want_width = None;
                s.plan_cache = None;
            }
            return Ok(empty_tile_plan());
        }

        // Level selection.
        let (mut want_width, mut want_scale);
        if let Some(pyramid) = &strategy.pyramid {
            want_scale = snap_to_pyramid(pyramid, demand.desired_device_width / page.width);
            want_width = js::round(want_scale * page.width);
        } else {
            want_width = js::min(
                js::max(1.0, js::round(demand.desired_device_width)),
                js::round(strategy.tile_max_scale * page.width),
            );
            want_scale = want_width / page.width;
        }

        // Stage-less demand tiles the whole page: clamp the level so the
        // whole-page tile count stays bounded.
        if demand.visible_rect.is_none() {
            let max_stageless_width = js::max(
                strategy.tile_size,
                (strategy.tile_size * (STAGELESS_TILE_CAP * page.width / page.height).sqrt())
                    .floor(),
            );
            if want_width > max_stageless_width {
                if !self.0.warned_stageless.get() {
                    self.0.warned_stageless.set(true);
                    (self.deps().warn)(&format!(
                        "[render] stage-less tile demand of {} device px would tile the whole page — clamped to {}. Supply a visibleRect (host the page in a Stage) for sharp deep zoom.",
                        num(js::round(demand.desired_device_width)),
                        num(max_stageless_width)
                    ));
                }
                if let Some(pyramid) = &strategy.pyramid {
                    let fitting: Vec<f64> = pyramid
                        .iter()
                        .copied()
                        .filter(|s| js::round(s * page.width) <= max_stageless_width)
                        .collect();
                    want_scale = fitting.last().copied().unwrap_or(pyramid[0]);
                    want_width = js::round(want_scale * page.width);
                } else {
                    want_width = max_stageless_width;
                    want_scale = want_width / page.width;
                }
            }
        }
        let grid = tile_grid(page, want_scale, strategy.tile_size);

        let visible = demand.visible_rect.unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: page.width,
            height: page.height,
        });
        state.borrow_mut().last_visible = Some(visible);
        let p0 = tiles_in_rect(&grid, page, visible);
        let ring = inflate_rect(
            visible,
            options.tiles.prefetch_margin,
            if options.tiles.velocity_bias { demand.velocity } else { None },
        );
        let p0_keys: HashSet<String> = p0.iter().map(|c| coord_key(*c)).collect();
        let p1: Vec<TileCoord> = tiles_in_rect(&grid, page, ring)
            .into_iter()
            .filter(|c| !p0_keys.contains(&coord_key(*c)))
            .collect();

        self.schedule(
            pon,
            &state,
            page,
            grid,
            want_scale,
            want_width,
            include_annotations,
            epoch,
            p0,
            p1,
            visible,
        )?;

        // Release retained generations covered by the current painted set.
        if self.release_covered(pon, &state, page, include_annotations) {
            let mut s = state.borrow_mut();
            s.version += 1;
            s.plan_cache = None;
        }

        let demand_key = format!("w{}|{}|e{}", num(want_width), rect_key(visible), num(epoch));
        {
            let s = state.borrow();
            if let Some(cache) = &s.plan_cache
                && cache.demand_key == demand_key
                && cache.version == s.version
            {
                return Ok(cache.plan.clone());
            }
        }

        let bleed_px = options.tiles.bleed_px;
        let entries: Vec<Rc<TileEntry>> = state.borrow().entries.values().cloned().collect();
        let mut paint: Vec<TilePaintSource> = Vec::new();
        let mut fetching: Vec<String> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        for entry in entries {
            let vis = intersect_rects(entry.rect, visible);
            if entry.resolved.get() {
                if vis.width > 0.0 && vis.height > 0.0 {
                    // The store owns the bytes; an evicted entry is simply no
                    // longer resolved.
                    let Some(handle) = self.deps().store.peek(&entry.key) else {
                        stale.push(entry.key.clone());
                        continue;
                    };
                    paint.push(TilePaintSource {
                        key: entry.key.clone(),
                        scale: entry.scale,
                        rect: if bleed_px > 0.0 {
                            bleed_rect(entry.rect, bleed_px / entry.scale, page)
                        } else {
                            entry.rect
                        },
                        z: 0.0,
                        handle,
                    });
                }
            } else if entry.scale == want_scale {
                fetching.push(entry.key.clone());
            }
        }
        {
            let mut s = state.borrow_mut();
            for key in &stale {
                s.entries.shift_remove(key);
            }
        }
        // Stacking: rank the scales present, coarse under fine.
        let mut scales: Vec<f64> = Vec::new();
        for p in &paint {
            if !scales.iter().any(|s| same_value_zero(*s, p.scale)) {
                scales.push(p.scale);
            }
        }
        let scales = crate::strategy::ascending(&scales);
        for p in &mut paint {
            let rank = scales.iter().position(|s| same_value_zero(*s, p.scale)).unwrap_or(0);
            p.z = rank as f64;
        }
        paint.sort_by(|a, b| {
            a.z.partial_cmp(&b.z)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| js::locale_compare(&a.key, &b.key))
        });
        fetching.sort();
        let mut s = state.borrow_mut();
        let plan = Rc::new(TilePaintPlan {
            engaged: true,
            paint,
            fetching,
            stamp: format!("{demand_key}|v{}", s.version),
        });
        s.plan_cache = Some(PlanCache { demand_key, version: s.version, plan: plan.clone() });
        Ok(plan)
    }

    /// The layer's painted report: this key's pixels had a presentation
    /// opportunity.
    pub fn source_painted(&self, view: &str, pon: u32, key: &str) {
        let Some(state) = self.0.pages.borrow().get(&state_key(view, pon)).cloned() else {
            return;
        };
        let Some(entry) = state.borrow().entries.get(key).cloned() else { return };
        if entry.painted.get() {
            return;
        }
        entry.painted.set(true);
        if let Some(page) = (self.deps().get_page_size)(pon) {
            self.release_covered(pon, &state, page, annotations_of(key));
        }
        {
            let mut s = state.borrow_mut();
            s.version += 1;
            s.plan_cache = None;
        }
        (self.deps().on_advance)(pon);
    }

    /// The inverse report: this key's image left the DOM, so its pixels are
    /// not compositable until it reports painted again.
    pub fn source_unpainted(&self, view: &str, pon: u32, key: &str) {
        let Some(state) = self.0.pages.borrow().get(&state_key(view, pon)).cloned() else {
            return;
        };
        if let Some(entry) = state.borrow().entries.get(key) {
            entry.painted.set(false);
        }
    }

    /// A view unmounted its tile plane for the page: stop fetching, drop the
    /// bookkeeping. Resolved bytes stay in the store.
    pub fn release_page(&self, view: &str, pon: u32) {
        let Some(state) = self.0.pages.borrow().get(&state_key(view, pon)).cloned() else {
            return;
        };
        self.abort_all(&state);
        self.0.pages.borrow_mut().remove(&state_key(view, pon));
    }

    /// Bookkeeping size for one view of a page, and the live fetch count.
    pub fn stats(&self, view: &str, pon: u32) -> TileStats {
        TileStats {
            entries: self
                .0
                .pages
                .borrow()
                .get(&state_key(view, pon))
                .map_or(0, |s| s.borrow().entries.len()),
            in_flight: self.0.in_flight.get(),
        }
    }

    fn page_state(&self, pon: u32, view: &str) -> State {
        self.0
            .pages
            .borrow_mut()
            .entry(state_key(view, pon))
            .or_insert_with(|| {
                Rc::new(RefCell::new(PageTileState {
                    epoch: -1.0,
                    want_scale: None,
                    want_width: None,
                    entries: IndexMap::new(),
                    failed_keys: HashSet::new(),
                    version: 0,
                    plan_cache: None,
                    settle_timer: None,
                    pending_level: None,
                    last_visible: None,
                }))
            })
            .clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn schedule(
        &self,
        pon: u32,
        state: &State,
        page: PageSizePt,
        grid: TileGrid,
        want_scale: f64,
        want_width: f64,
        include_annotations: bool,
        epoch: f64,
        p0: Vec<TileCoord>,
        p1: Vec<TileCoord>,
        visible: Rect,
    ) -> Result<(), Error> {
        let suffix = self.key_suffix();
        let options = self.deps().options.clone();
        let (first_engage, level_changed) = {
            let mut s = state.borrow_mut();
            let first_engage = s.want_width.is_none();
            let level_changed = s.want_width.is_some_and(|w| w != want_width);
            if level_changed {
                s.failed_keys.clear();
            }
            s.want_scale = Some(want_scale);
            s.want_width = Some(want_width);
            (first_engage, level_changed)
        };

        // Entries follow the want set: in-flight ones that left it abort,
        // resolved same-level ones drop (their bytes stay in the store), and
        // only cross-level retained entries stay.
        let wanted: HashSet<String> = p0
            .iter()
            .chain(p1.iter())
            .map(|c| self.tile_key(&suffix, pon, want_width, *c, include_annotations, epoch))
            .collect();
        let snapshot: Vec<(String, Rc<TileEntry>)> =
            state.borrow().entries.iter().map(|(k, e)| (k.clone(), e.clone())).collect();
        for (key, entry) in snapshot {
            if wanted.contains(&key) {
                continue;
            }
            if !entry.resolved.get() {
                entry.abort.abort(js::dom_abort_error());
                // The transit slot frees now, so fetches started in this plan
                // see the capacity.
                self.release_slot(&entry);
                state.borrow_mut().entries.shift_remove(&key);
            } else if entry.scale == want_scale {
                state.borrow_mut().entries.shift_remove(&key);
            }
        }

        // Center-out: the region under the gesture sharpens first.
        let cx = visible.x + visible.width / 2.0;
        let cy = visible.y + visible.height / 2.0;
        let span = grid.tile_size / grid.scale;
        let mut ordered_p0 = p0;
        ordered_p0.sort_by(|a, b| {
            let da = (a.ix + 0.5) * span - cx;
            let db = (b.ix + 0.5) * span - cx;
            let ea = (a.iy + 0.5) * span - cy;
            let eb = (b.iy + 0.5) * span - cy;
            let d = da * da + ea * ea - (db * db + eb * eb);
            d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
        });

        let kickoff = {
            let this = self.clone();
            let state = state.clone();
            move || -> Result<(), Error> {
                let all_p0_ready = this.ensure_fetches(
                    pon,
                    &state,
                    page,
                    grid,
                    want_width,
                    include_annotations,
                    epoch,
                    &ordered_p0,
                )?;
                // P1 strictly after P0: prefetch never competes with visible tiles.
                if all_p0_ready {
                    this.ensure_fetches(
                        pon,
                        &state,
                        page,
                        grid,
                        want_width,
                        include_annotations,
                        epoch,
                        &p1,
                    )?;
                }
                Ok(())
            }
        };

        // Level-change settle: a zoom in motion does not fetch each
        // intermediate level. First engagement fires at once.
        if level_changed && options.tiles.settle_ms > 0.0 {
            self.debug(|| format!("arm settle pon={pon} level={}", num(want_width)));
            let previous = {
                let mut s = state.borrow_mut();
                s.pending_level = Some(want_width);
                s.settle_timer.take()
            };
            if let Some(timer) = previous {
                rt::clear_timer(timer);
            }
            let this = self.clone();
            let timer_state = state.clone();
            let timer = rt::set_timeout(options.tiles.settle_ms, move || {
                let current = {
                    let mut s = timer_state.borrow_mut();
                    s.settle_timer = None;
                    s.pending_level == Some(want_width)
                };
                this.debug(|| {
                    format!("settle fired pon={pon} level={} current={current}", num(want_width))
                });
                if current {
                    // A throw here would be an uncaught error in a timer.
                    let _ = kickoff();
                }
            });
            state.borrow_mut().settle_timer = Some(timer);
            return Ok(());
        }
        let cancel = {
            let mut s = state.borrow_mut();
            if s.settle_timer.is_some() && !level_changed && !first_engage {
                // The zoom came back to the pending level: fetch it now.
                s.pending_level = None;
                s.settle_timer.take()
            } else {
                None
            }
        };
        if let Some(timer) = cancel {
            rt::clear_timer(timer);
        }
        kickoff()
    }

    /// Start missing fetches; `true` when every coord is already resolved.
    #[allow(clippy::too_many_arguments)]
    fn ensure_fetches(
        &self,
        pon: u32,
        state: &State,
        page: PageSizePt,
        grid: TileGrid,
        want_width: f64,
        include_annotations: bool,
        epoch: f64,
        coords: &[TileCoord],
    ) -> Result<bool, Error> {
        let suffix = self.key_suffix();
        let mut all_ready = true;
        let mut started = 0;
        let bleed_pt = self.deps().options.tiles.bleed_px / grid.scale;
        for coord in coords {
            let key = self.tile_key(&suffix, pon, want_width, *coord, include_annotations, epoch);
            // A key that failed at this level is not retried until the level
            // or epoch changes.
            if state.borrow().failed_keys.contains(&key) {
                continue;
            }
            if let Some(existing) = state.borrow().entries.get(&key) {
                if !existing.resolved.get() {
                    all_ready = false;
                }
                continue;
            }
            all_ready = false;
            // Backpressure: the wake → plan → ensure loop is the pump.
            if self.0.in_flight.get() >= MAX_IN_FLIGHT {
                continue;
            }
            self.0.in_flight.set(self.0.in_flight.get() + 1);
            started += 1;
            let logical = tile_paint_rect(&grid, page, *coord);
            let entry = Rc::new(TileEntry {
                key: key.clone(),
                scale: grid.scale,
                rect: logical,
                resolved: Cell::new(false),
                painted: Cell::new(false),
                charged: Cell::new(true),
                abort: AbortSignal::new(),
            });
            state.borrow_mut().entries.insert(key.clone(), entry.clone());
            let fetch_tile = self.deps().fetch_tile.clone();
            let promise = self.deps().store.acquire(
                &key,
                |signal| {
                    // The rendered region is the bled rect, matching the
                    // placement rect the paint list emits.
                    let rendered =
                        if bleed_pt > 0.0 { bleed_rect(logical, bleed_pt, page) } else { logical };
                    fetch_tile(pon, rendered, grid.scale, include_annotations, signal)
                },
                Some(&entry.abort),
            )?;
            let this = self.clone();
            let state = state.clone();
            rt::spawn(async move {
                let result = promise.await;
                this.release_slot(&entry);
                let current =
                    state.borrow().entries.get(&key).is_some_and(|e| Rc::ptr_eq(e, &entry));
                if !current {
                    return; // aborted or superseded
                }
                match result {
                    Ok(_) => {
                        // The handle stays in the store; this records success.
                        entry.resolved.set(true);
                        let mut s = state.borrow_mut();
                        s.version += 1;
                        s.plan_cache = None;
                    }
                    Err(error) => {
                        state.borrow_mut().entries.shift_remove(&key);
                        // Our own abort is expected silence; a real failure
                        // marks the key and wakes the layers.
                        if entry.abort.aborted() {
                            return;
                        }
                        state.borrow_mut().failed_keys.insert(key.clone());
                        this.debug(|| format!("tile failed {key}: {}", js::error_string(&error)));
                        let mut s = state.borrow_mut();
                        s.version += 1;
                        s.plan_cache = None;
                    }
                }
                (this.deps().on_advance)(pon);
            });
        }
        if started > 0 {
            self.debug(|| format!("fetch pon={pon} level={} +{started} tiles", num(want_width)));
        }
        Ok(all_ready)
    }

    /// The release rule, as a pure function of the current painted set:
    /// retained entries whose visible footprint is covered by painted want
    /// tiles leave. Runs on every plan and every painted report.
    fn release_covered(
        &self,
        pon: u32,
        state: &State,
        page: PageSizePt,
        include_annotations: bool,
    ) -> bool {
        let suffix = self.key_suffix();
        let (want_scale, want_width, epoch, last_visible) = {
            let s = state.borrow();
            (s.want_scale, s.want_width, s.epoch, s.last_visible)
        };
        let (Some(want_scale), Some(want_width)) = (want_scale, want_width) else { return false };
        let grid = tile_grid(page, want_scale, self.strategy().tile_size);
        let painted_at = |c: TileCoord| {
            let key = self.tile_key(&suffix, pon, want_width, c, include_annotations, epoch);
            state.borrow().entries.get(&key).is_some_and(|e| e.painted.get())
        };
        let mut released = false;
        let snapshot: Vec<(String, Rc<TileEntry>)> =
            state.borrow().entries.iter().map(|(k, e)| (k.clone(), e.clone())).collect();
        for (key, entry) in snapshot {
            if entry.scale == want_scale || !entry.resolved.get() {
                continue;
            }
            // Visible footprint only.
            let region = match last_visible {
                Some(visible) => intersect_rects(entry.rect, visible),
                None => entry.rect,
            };
            if region_covered(&grid, page, region, &painted_at) {
                state.borrow_mut().entries.shift_remove(&key);
                released = true;
            } else if self.deps().debug.is_some() {
                let missing: Vec<TileCoord> = tiles_in_rect(&grid, page, region)
                    .into_iter()
                    .filter(|c| !painted_at(*c))
                    .collect();
                let detail: Vec<String> = missing
                    .iter()
                    .take(3)
                    .map(|c| {
                        let k =
                            self.tile_key(&suffix, pon, want_width, *c, include_annotations, epoch);
                        let st = match state.borrow().entries.get(&k) {
                            None => "NO-ENTRY",
                            Some(e) if !e.resolved.get() => "PENDING",
                            Some(_) => "RESOLVED-unpainted",
                        };
                        format!("{}={st}", coord_key(*c))
                    })
                    .collect();
                self.debug(|| {
                    format!(
                        "retained {key} blocked by: {} ({} missing)",
                        detail.join(" "),
                        missing.len()
                    )
                });
            }
        }
        released
    }

    /// Idempotent transit-slot release: the one place the count decrements.
    fn release_slot(&self, entry: &TileEntry) {
        if !entry.charged.get() {
            return;
        }
        entry.charged.set(false);
        self.0.in_flight.set(self.0.in_flight.get() - 1);
    }

    fn abort_all(&self, state: &State) {
        let (timer, pending) = {
            let mut s = state.borrow_mut();
            let pending: Vec<Rc<TileEntry>> =
                s.entries.values().filter(|e| !e.resolved.get()).cloned().collect();
            (s.settle_timer.take(), pending)
        };
        if let Some(timer) = timer {
            rt::clear_timer(timer);
        }
        for entry in pending {
            entry.abort.abort(js::dom_abort_error());
            self.release_slot(&entry);
        }
    }

    /// Level identity is the integer device width across the page; the key
    /// also carries what determines the fetched bitmap (tile size and bleed
    /// from the options, the conformed format).
    fn tile_key(
        &self,
        suffix: &str,
        pon: u32,
        level_width: f64,
        c: TileCoord,
        include_annotations: bool,
        epoch: f64,
    ) -> String {
        format!(
            "t:{pon}|w{}|{}|a{}|e{}|{suffix}",
            num(level_width),
            coord_key(c),
            u8::from(include_annotations),
            num(epoch)
        )
    }

    /// The part of every [`tile_key`](Self::tile_key) that depends only on
    /// the options and the strategy's format. Built once per pass over a
    /// page's tiles: resolving the strategy reads and compares the policy,
    /// and doing that per key was most of the key's cost.
    fn key_suffix(&self) -> String {
        let tiles = &self.deps().options.tiles;
        let format = self.strategy().format;
        format!(
            "g{}.{}{}",
            num(tiles.size),
            num(tiles.bleed_px),
            format.map_or(String::new(), |f| format!(".{}", f.as_str()))
        )
    }
}

/// `SameValueZero`, the equality a `Set` of numbers and `Map.get` use.
fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}
