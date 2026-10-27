//! `tile-manager.ts`: the tile retention state machine. One per document,
//! all levels, pages and views in one place, over the same [`RasterStore`]
//! the base renders use.
//!
//! The invariant: every screen region paints the sharpest painted pixels
//! available, and quality per region only goes up until the want set
//! resolves.
//!
//! - Want vs paint: `plan` queues the want set (visible tiles first,
//!   center-out, then the prefetch ring once every visible tile resolved)
//!   and returns a paint list drawn only from resolved entries, the current
//!   level and retained older generations.
//! - One line for every view and page: a transit slot goes to an on-screen
//!   tile of any page before a tile of any prefetch ring.
//! - Release on occlusion: a retained generation leaves the paint list once
//!   the want tiles covering its visible footprint report painted. Its bytes
//!   stay in the store, so zooming back re-promotes from cache.
//! - Epoch exception: an invalidation bump means retained pixels are wrong,
//!   so everything of the old epoch drops at once.
//!
//! `plan` never wakes readers itself: its callers do, once, when the
//! returned plan changed. Resolution handlers call `on_advance` and hand
//! the freed slot on.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use epdf_geometry::Rect;
use epdf_kernel::Error;
use epdf_kernel::clock::Cancel;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use indexmap::IndexMap;
use serde_json::Value;

use crate::js::{self, number_to_string as num};
use crate::model::{PageRenderLayers, layers_key_of};
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

/// `fetchTile(view, page, rect, scale, layers, signal)`: render a page-space
/// region (y-down page points) for `view`, whose pixels it is. The promise
/// answers the engine's `PageImageHandle` JSON.
///
/// The TypeScript's `fetchTile` is `async`, so it never throws. A Rust
/// embedder's may answer `Err` at once; the manager takes that as the
/// fetch's rejection and frees the slot (M25), where a synchronous throw in
/// the TypeScript would leave the entry and its slot behind.
pub type FetchTile = Rc<
    dyn Fn(&str, u32, Rect, f64, PageRenderLayers, &AbortSignal) -> Result<Promise<Value>, Error>,
>;

/// What the manager needs from its owner. The capability supplies these; a
/// host embedding the manager on its own supplies its own.
#[derive(Clone)]
pub struct TileManagerDeps {
    pub store: RasterStore,
    /// The render settings as they are now: read at every use, so a change
    /// applies at once.
    pub get_options: Rc<dyn Fn() -> Rc<ResolvedRenderOptions>>,
    /// The document fact off the kernel registry.
    pub get_policy: Rc<dyn Fn() -> EngineRenderPolicy>,
    pub get_page_size: Rc<dyn Fn(u32) -> Option<PageSizePt>>,
    pub get_epoch: Rc<dyn Fn(u32, PageRenderLayers) -> f64>,
    /// Run `run` once, `ms` from now; answers its cancel (`ctx.clock.after`).
    pub after: Rc<dyn Fn(f64, Box<dyn FnOnce()>) -> Cancel>,
    pub fetch_tile: FetchTile,
    /// A page's plans changed outside `plan`: re-plan it and wake layers.
    pub on_advance: Rc<dyn Fn(u32)>,
    /// Diagnostic sink: scheduling and fetch outcomes. The manager composes
    /// the more expensive diagnostics only while the options say `debug`.
    pub debug: Option<Rc<dyn Fn(&str)>>,
    /// `console.warn`, which the TypeScript calls directly for the
    /// stage-less clamp.
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

/// The tiles one view of a page wants started, in order.
#[derive(Clone)]
struct TileQueue {
    /// Visible tiles center-out, then the prefetch ring once every visible
    /// tile resolved.
    coords: Rc<Vec<TileCoord>>,
    /// Where the ring starts in `coords`.
    ring_from: usize,
    /// Where the pump looks next: every coord before it is started or failed.
    next: usize,
    page: PageSizePt,
    grid: TileGrid,
    want_width: f64,
    layers: PageRenderLayers,
    epoch: f64,
}

struct PageTileState {
    view: String,
    pon: u32,
    /// What the pump starts for this view of the page.
    queue: Option<TileQueue>,
    epoch: f64,
    want_scale: Option<f64>,
    /// The want level's identity: integer device px across the page.
    want_width: Option<f64>,
    entries: IndexMap<String, Rc<TileEntry>>,
    /// Keys that failed (not aborted) at the current level.
    failed_keys: HashSet<String>,
    version: u64,
    plan_cache: Option<PlanCache>,
    /// Cancels the level-change settle wait while it runs.
    cancel_settle: Option<Cancel>,
    pending_level: Option<f64>,
    last_visible: Option<Rect>,
}

type State = Rc<RefCell<PageTileState>>;

struct Inner {
    /// Per view-of-page, in the order they were first planned.
    pages: RefCell<IndexMap<String, State>>,
    strategy_memo:
        RefCell<Option<(EngineRenderPolicy, Rc<ResolvedRenderOptions>, Rc<ResolvedStrategy>)>>,
    in_flight: Cell<i64>,
    warned_stageless: Cell<bool>,
    deps: TileManagerDeps,
}

/// `TileManager`. Cheap to clone; clones share the manager.
#[derive(Clone)]
pub struct TileManager(Rc<Inner>);

/// `stats(view, page)`: bookkeeping size, for diagnostics.
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

/// `layersOfKey(key)`: what a tile key's picture draws, as
/// [`layers_key_of`] wrote it into the key.
fn layers_of_key(key: &str) -> PageRenderLayers {
    let drawn = |a: char, f: char| key.contains(&format!("|a{a}f{f}|"));
    for (a, f) in [('0', '0'), ('0', '1'), ('1', '0'), ('1', '1')] {
        if drawn(a, f) {
            return PageRenderLayers {
                include_annotations: a == '1',
                include_form_fields: f == '1',
            };
        }
    }
    PageRenderLayers::default()
}

impl TileManager {
    pub fn new(deps: TileManagerDeps) -> Self {
        TileManager(Rc::new(Inner {
            pages: RefCell::new(IndexMap::new()),
            strategy_memo: RefCell::new(None),
            in_flight: Cell::new(0),
            warned_stageless: Cell::new(false),
            deps,
        }))
    }

    fn deps(&self) -> &TileManagerDeps {
        &self.0.deps
    }

    fn options(&self) -> Rc<ResolvedRenderOptions> {
        (self.deps().get_options)()
    }

    fn debug(&self, message: impl FnOnce() -> String) {
        if let Some(debug) = &self.deps().debug {
            debug(&message());
        }
    }

    /// The resolved strategy, recomputed when the policy or the settings
    /// change.
    fn strategy(&self) -> Rc<ResolvedStrategy> {
        let policy = (self.deps().get_policy)();
        let options = self.options();
        let mut memo = self.0.strategy_memo.borrow_mut();
        match &*memo {
            Some((p, o, strategy)) if *p == policy && Rc::ptr_eq(o, &options) => strategy.clone(),
            _ => {
                let strategy = Rc::new(resolve_strategy(&policy, &options));
                *memo = Some((policy, options, strategy.clone()));
                strategy
            }
        }
    }

    /// The tile paint plan for one view of a page.
    pub fn plan(
        &self,
        view: &str,
        pon: u32,
        demand: &PageViewDemand,
        layers: PageRenderLayers,
    ) -> Rc<TilePaintPlan> {
        let options = self.options();
        if !options.tiles.enabled {
            return empty_tile_plan();
        }
        let strategy = self.strategy();
        let Some(page) = (self.deps().get_page_size)(pon) else { return empty_tile_plan() };

        let epoch = (self.deps().get_epoch)(pon, layers);
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

        // Engagement: the deficit of what the base actually supplies. An
        // empty ladder supplies `undefined`, and the deficit is NaN (M26).
        let supplied = base_ask_width(&strategy, demand.desired_device_width).unwrap_or(f64::NAN);
        let deficit = demand.desired_device_width / supplied;
        if deficit <= strategy.engage_at {
            let engaged = {
                let s = state.borrow();
                !s.entries.is_empty() || s.want_scale.is_some()
            };
            if engaged {
                self.debug(|| {
                    format!("disengage page={pon} (deficit {})", js::to_fixed(deficit, 2))
                });
                self.abort_all(&state);
                {
                    let mut s = state.borrow_mut();
                    s.entries.clear();
                    s.failed_keys.clear();
                    s.want_scale = None;
                    s.want_width = None;
                    s.plan_cache = None;
                }
                self.pump();
            }
            return empty_tile_plan();
        }

        // Level selection: pyramid mode snaps up the ladder; exact mode
        // renders the demand itself, clamped by the safety cap.
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
                    // An empty pyramid answers NaN, the TypeScript's `undefined` (M26).
                    want_scale = fitting
                        .last()
                        .copied()
                        .unwrap_or_else(|| pyramid.first().copied().unwrap_or(f64::NAN));
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
        let visible_coords = tiles_in_rect(&grid, page, visible);
        let ring = inflate_rect(
            visible,
            options.tiles.prefetch_margin,
            if options.tiles.velocity_bias { demand.velocity } else { None },
        );
        let visible_keys: HashSet<String> = visible_coords.iter().map(|c| coord_key(*c)).collect();
        let prefetch_coords: Vec<TileCoord> = tiles_in_rect(&grid, page, ring)
            .into_iter()
            .filter(|c| !visible_keys.contains(&coord_key(*c)))
            .collect();

        self.schedule(
            &state,
            page,
            grid,
            want_scale,
            want_width,
            layers,
            epoch,
            visible_coords,
            prefetch_coords,
            visible,
        );

        // Release retained generations covered by the current painted set:
        // evaluated here, not only on painted reports, so report order can
        // never strand a release.
        if self.release_covered(pon, &state, page, layers) {
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
                return cache.plan.clone();
            }
        }

        let bleed_px = self.options().tiles.bleed_px;
        let entries: Vec<Rc<TileEntry>> = state.borrow().entries.values().cloned().collect();
        let mut paint: Vec<TilePaintSource> = Vec::new();
        let mut fetching: Vec<String> = Vec::new();
        let mut stale: Vec<String> = Vec::new();
        for entry in entries {
            let visible_part = intersect_rects(entry.rect, visible);
            if entry.resolved.get() {
                if visible_part.width > 0.0 && visible_part.height > 0.0 {
                    // The store owns the bytes: an entry whose bytes were
                    // evicted is no longer resolved.
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
        for source in &paint {
            if !scales.iter().any(|s| same_value_zero(*s, source.scale)) {
                scales.push(source.scale);
            }
        }
        let scales = crate::strategy::ascending(&scales);
        for source in &mut paint {
            let rank = scales.iter().position(|s| same_value_zero(*s, source.scale)).unwrap_or(0);
            source.z = rank as f64;
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
        plan
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
            self.release_covered(pon, &state, page, layers_of_key(key));
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

    /// A view stopped painting tiles for the page: stop fetching, drop the
    /// bookkeeping. Resolved bytes stay in the store.
    pub fn release_page(&self, view: &str, pon: u32) {
        let Some(state) = self.0.pages.borrow().get(&state_key(view, pon)).cloned() else {
            return;
        };
        self.abort_all(&state);
        self.0.pages.borrow_mut().shift_remove(&state_key(view, pon));
        self.pump();
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
                    view: view.to_string(),
                    pon,
                    queue: None,
                    epoch: -1.0,
                    want_scale: None,
                    want_width: None,
                    entries: IndexMap::new(),
                    failed_keys: HashSet::new(),
                    version: 0,
                    plan_cache: None,
                    cancel_settle: None,
                    pending_level: None,
                    last_visible: None,
                }))
            })
            .clone()
    }

    #[allow(clippy::too_many_arguments)]
    fn schedule(
        &self,
        state: &State,
        page: PageSizePt,
        grid: TileGrid,
        want_scale: f64,
        want_width: f64,
        layers: PageRenderLayers,
        epoch: f64,
        visible_coords: Vec<TileCoord>,
        prefetch_coords: Vec<TileCoord>,
        visible: Rect,
    ) {
        let pon = state.borrow().pon;
        let suffix = self.key_suffix();
        let options = self.options();
        let (first_engage, level_changed) = {
            let mut s = state.borrow_mut();
            let first_engage = s.want_width.is_none();
            let level_changed = s.want_width.is_some_and(|w| w != want_width);
            if level_changed {
                // A new level gets fresh chances.
                s.failed_keys.clear();
            }
            s.want_scale = Some(want_scale);
            s.want_width = Some(want_width);
            (first_engage, level_changed)
        };

        // Entries follow the want set: in-flight ones that left it abort,
        // resolved same-level ones drop (their bytes stay in the store), and
        // only cross-level retained entries stay.
        let key_of = |c: TileCoord| tile_key(&suffix, pon, want_width, c, layers, epoch);
        let wanted: HashSet<String> =
            visible_coords.iter().chain(prefetch_coords.iter()).map(|c| key_of(*c)).collect();
        let snapshot: Vec<(String, Rc<TileEntry>)> =
            state.borrow().entries.iter().map(|(k, e)| (k.clone(), e.clone())).collect();
        for (key, entry) in snapshot {
            if wanted.contains(&key) {
                continue;
            }
            if !entry.resolved.get() {
                entry.abort.abort(js::dom_abort_error());
                // The transit slot frees now, so fetches started in this
                // plan see the capacity.
                self.release_slot(&entry);
                state.borrow_mut().entries.shift_remove(&key);
            } else if entry.scale == want_scale {
                state.borrow_mut().entries.shift_remove(&key);
            }
        }

        // Center-out: the region under the gesture sharpens first.
        let center_x = visible.x + visible.width / 2.0;
        let center_y = visible.y + visible.height / 2.0;
        let span = grid.tile_size / grid.scale;
        let distance_to_center = |c: &TileCoord| {
            let dx = (c.ix + 0.5) * span - center_x;
            let dy = (c.iy + 0.5) * span - center_y;
            dx * dx + dy * dy
        };
        let mut ordered_visible = visible_coords;
        ordered_visible.sort_by(|a, b| {
            let d = distance_to_center(a) - distance_to_center(b);
            d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
        });

        let kickoff = {
            let this = self.clone();
            let state = state.clone();
            let suffix = suffix.clone();
            move || {
                // The prefetch ring strictly after the visible tiles. A tile
                // that failed at this level counts as done.
                let all_visible_ready = {
                    let s = state.borrow();
                    ordered_visible.iter().all(|c| {
                        let key = tile_key(&suffix, pon, want_width, *c, layers, epoch);
                        s.failed_keys.contains(&key)
                            || s.entries.get(&key).is_some_and(|e| e.resolved.get())
                    })
                };
                let ring_from = ordered_visible.len();
                let coords = if all_visible_ready {
                    ordered_visible.iter().chain(prefetch_coords.iter()).copied().collect()
                } else {
                    ordered_visible.clone()
                };
                state.borrow_mut().queue = Some(TileQueue {
                    coords: Rc::new(coords),
                    ring_from,
                    next: 0,
                    page,
                    grid,
                    want_width,
                    layers,
                    epoch,
                });
                this.pump();
            }
        };

        // Level-change settle: a zoom in motion does not fetch each
        // intermediate level. First engagement fires at once.
        if level_changed && options.tiles.settle_ms > 0.0 {
            self.debug(|| format!("arm settle page={pon} level={}", num(want_width)));
            let previous = {
                let mut s = state.borrow_mut();
                // Nothing of the old level starts meanwhile.
                s.queue = None;
                s.pending_level = Some(want_width);
                s.cancel_settle.take()
            };
            if let Some(cancel) = previous {
                cancel.call();
            }
            let this = self.clone();
            let timer_state = state.clone();
            let cancel = (self.deps().after)(
                options.tiles.settle_ms,
                Box::new(move || {
                    let current = {
                        let mut s = timer_state.borrow_mut();
                        s.cancel_settle = None;
                        s.pending_level == Some(want_width)
                    };
                    this.debug(|| {
                        format!(
                            "settle fired page={pon} level={} current={current}",
                            num(want_width)
                        )
                    });
                    if current {
                        kickoff();
                    }
                }),
            );
            state.borrow_mut().cancel_settle = Some(cancel);
            return;
        }
        let cancel = {
            let mut s = state.borrow_mut();
            if s.cancel_settle.is_some() && !level_changed && !first_engage {
                // The zoom came back to the current level: cancel the
                // pending level's fetch.
                s.pending_level = None;
                s.cancel_settle.take()
            } else {
                None
            }
        };
        if let Some(cancel) = cancel {
            cancel.call();
        }
        kickoff();
    }

    /// Starts queued tiles while transit slots are free: the first on-screen
    /// tile of any view and page, in the order their queues were set, before
    /// any tile of a prefetch ring. Within one view of a page, its queue's
    /// order holds.
    fn pump(&self) {
        let mut started: IndexMap<String, (State, u32)> = IndexMap::new();
        while self.0.in_flight.get() < MAX_IN_FLIGHT {
            let states: Vec<(String, State)> =
                self.0.pages.borrow().iter().map(|(k, s)| (k.clone(), s.clone())).collect();
            let mut next: Option<(String, State, TileCoord)> = None;
            for (key, state) in states {
                let Some((coord, prefetch)) = self.next_to_start(&state) else { continue };
                if next.is_some() && prefetch {
                    continue;
                }
                next = Some((key, state, coord));
                if !prefetch {
                    break;
                }
            }
            let Some((key, state, coord)) = next else { break };
            self.start(&state, coord);
            started.entry(key).or_insert_with(|| (state.clone(), 0)).1 += 1;
        }
        for (_, (state, count)) in started {
            let s = state.borrow();
            let level = s.queue.as_ref().map_or_else(|| "undefined".into(), |q| num(q.want_width));
            self.debug(|| format!("fetch page={} level={level} +{count} tiles", s.pon));
        }
    }

    /// The first tile of the state's queue not started yet, nor failed at
    /// this level, and whether it belongs to the ring.
    fn next_to_start(&self, state: &State) -> Option<(TileCoord, bool)> {
        let suffix = self.key_suffix();
        let mut s = state.borrow_mut();
        let pon = s.pon;
        let PageTileState { queue, failed_keys, entries, .. } = &mut *s;
        let queue = queue.as_mut()?;
        while queue.next < queue.coords.len() {
            let coord = queue.coords[queue.next];
            let key = tile_key(&suffix, pon, queue.want_width, coord, queue.layers, queue.epoch);
            // A key that failed at this level is not retried until the level
            // or epoch changes.
            if failed_keys.contains(&key) || entries.contains_key(&key) {
                queue.next += 1;
                continue;
            }
            return Some((coord, queue.next >= queue.ring_from));
        }
        None
    }

    /// Starts one queued tile's fetch, in a transit slot.
    fn start(&self, state: &State, coord: TileCoord) {
        let (view, pon, queue) = {
            let s = state.borrow();
            (s.view.clone(), s.pon, s.queue.clone().expect("a started tile has a queue"))
        };
        let TileQueue { page, grid, want_width, layers, epoch, .. } = queue;
        let key = tile_key(&self.key_suffix(), pon, want_width, coord, layers, epoch);
        let bleed_pt = self.options().tiles.bleed_px / grid.scale;
        self.0.in_flight.set(self.0.in_flight.get() + 1);
        let logical = tile_paint_rect(&grid, page, coord);
        let entry = Rc::new(TileEntry {
            key: key.clone(),
            scale: grid.scale,
            rect: logical,
            resolved: Cell::new(false),
            painted: Cell::new(false),
            charged: Cell::new(true),
            abort: AbortSignal::new(),
        });
        {
            let mut s = state.borrow_mut();
            s.entries.insert(key.clone(), entry.clone());
            // The cached plan's `fetching` misses this tile now.
            s.plan_cache = None;
        }
        let fetch_tile = self.deps().fetch_tile.clone();
        let acquired = self.deps().store.acquire(
            &key,
            |signal| {
                // The rendered region is the bled rect, matching the
                // placement rect the paint list emits.
                let rendered =
                    if bleed_pt > 0.0 { bleed_rect(logical, bleed_pt, page) } else { logical };
                fetch_tile(&view, pon, rendered, grid.scale, layers, signal)
            },
            Some(&entry.abort),
        );
        let promise = match acquired {
            Ok(promise) => promise,
            Err(error) => {
                // M25: a synchronous refusal settles as a rejection does.
                // The pump that called this goes on to the next tile.
                self.release_slot(&entry);
                self.fail(state, &entry, &error);
                return;
            }
        };
        let this = self.clone();
        let state = state.clone();
        rt::spawn(async move {
            let result = promise.await;
            this.release_slot(&entry);
            match result {
                Ok(_) => {
                    // Unless aborted or superseded meanwhile. The handle
                    // stays in the store; the entry records success.
                    if this.is_current(&state, &entry) {
                        entry.resolved.set(true);
                        {
                            let mut s = state.borrow_mut();
                            s.version += 1;
                            s.plan_cache = None;
                        }
                        (this.deps().on_advance)(pon);
                    }
                }
                Err(error) => this.fail(&state, &entry, &error),
            }
            this.pump();
        });
    }

    fn is_current(&self, state: &State, entry: &Rc<TileEntry>) -> bool {
        state.borrow().entries.get(&entry.key).is_some_and(|e| Rc::ptr_eq(e, entry))
    }

    /// A fetch that did not resolve. Our own abort (pan-away, level change)
    /// is expected silence; a real failure marks the key and wakes the
    /// layers, so the rest of the want set keeps making progress.
    fn fail(&self, state: &State, entry: &Rc<TileEntry>, error: &Error) {
        if !self.is_current(state, entry) {
            return;
        }
        state.borrow_mut().entries.shift_remove(&entry.key);
        if entry.abort.aborted() {
            return;
        }
        let pon = {
            let mut s = state.borrow_mut();
            s.failed_keys.insert(entry.key.clone());
            s.pon
        };
        self.debug(|| format!("tile failed {}: {}", entry.key, js::error_string(error)));
        {
            let mut s = state.borrow_mut();
            s.version += 1;
            s.plan_cache = None;
        }
        (self.deps().on_advance)(pon);
    }

    /// The release rule, as a pure function of the current painted set:
    /// retained entries whose visible footprint is covered by painted want
    /// tiles leave. Runs on every plan and every painted report.
    fn release_covered(
        &self,
        pon: u32,
        state: &State,
        page: PageSizePt,
        layers: PageRenderLayers,
    ) -> bool {
        let suffix = self.key_suffix();
        let (want_scale, want_width, epoch, last_visible) = {
            let s = state.borrow();
            (s.want_scale, s.want_width, s.epoch, s.last_visible)
        };
        let (Some(want_scale), Some(want_width)) = (want_scale, want_width) else { return false };
        let grid = tile_grid(page, want_scale, self.strategy().tile_size);
        let want_key_of = |c: TileCoord| tile_key(&suffix, pon, want_width, c, layers, epoch);
        let painted_at = |c: TileCoord| {
            state.borrow().entries.get(&want_key_of(c)).is_some_and(|e| e.painted.get())
        };
        let diagnose = self.deps().debug.is_some() && self.options().debug;
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
            if region_covered(&grid, page, region, painted_at) {
                state.borrow_mut().entries.shift_remove(&key);
                released = true;
            } else if diagnose {
                let missing: Vec<TileCoord> = tiles_in_rect(&grid, page, region)
                    .into_iter()
                    .filter(|c| !painted_at(*c))
                    .collect();
                let detail: Vec<String> = missing
                    .iter()
                    .take(3)
                    .map(|c| {
                        let status = match state.borrow().entries.get(&want_key_of(*c)) {
                            None => "NO-ENTRY",
                            Some(e) if !e.resolved.get() => "PENDING",
                            Some(_) => "RESOLVED-unpainted",
                        };
                        format!("{}={status}", coord_key(*c))
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
        let (cancel, pending) = {
            let mut s = state.borrow_mut();
            s.queue = None;
            let pending: Vec<Rc<TileEntry>> =
                s.entries.values().filter(|e| !e.resolved.get()).cloned().collect();
            (s.cancel_settle.take(), pending)
        };
        if let Some(cancel) = cancel {
            cancel.call();
        }
        for entry in pending {
            entry.abort.abort(js::dom_abort_error());
            self.release_slot(&entry);
        }
    }

    /// The part of every tile key that depends only on the options and the
    /// strategy's format: what determines the fetched bitmap for a coord.
    fn key_suffix(&self) -> String {
        let options = self.options();
        let format = self.strategy().format;
        format!(
            "g{}.{}{}",
            num(options.tiles.size),
            num(options.tiles.bleed_px),
            format.map_or(String::new(), |f| format!(".{}", f.as_str()))
        )
    }
}

/// `tileKey`: the level identity is the integer device width across the
/// page; the key also carries what determines the fetched bitmap (tile
/// size, bleed and format, in `suffix`).
fn tile_key(
    suffix: &str,
    pon: u32,
    level_width: f64,
    c: TileCoord,
    layers: PageRenderLayers,
    epoch: f64,
) -> String {
    format!(
        "t:{pon}|w{}|{}|{}|e{}|{suffix}",
        num(level_width),
        coord_key(c),
        layers_key_of(layers),
        num(epoch)
    )
}

/// `SameValueZero`, the equality a `Set` of numbers and `Map.get` use.
fn same_value_zero(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}
