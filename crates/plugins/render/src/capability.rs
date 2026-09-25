//! `capability.ts`: the one place STRATEGY (the plugin options, what the
//! viewer spends) composes with POLICY (the engine fact, what the deployment
//! serves). Conforming demand to the resolved render points, collapsing
//! same-key asks in the raster store and exposing stable source keys all
//! happen here.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::types::DocumentMeta;
use epdf_kernel::{Ctx, Error};
use indexmap::IndexSet;
use serde_json::{Value, json};

use crate::effects::all_pons_of;
use crate::js::{self, number_to_string as num};
use crate::paint_plan::{
    PageViewDemand, RenderPluginOptions, ResolvedRenderOptions, TilePaintPlan,
    resolve_render_options,
};
use crate::policy::{CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, PageRenderViewport};
use crate::raster_store::{PageImageHandle, RasterStore};
use crate::strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
use crate::tile_manager::{TileManager, TileManagerDeps};
use crate::tiles::PageSizePt;
use crate::types::{
    InvalidateOptions, InvalidateScope, PaintSettings, RenderAction, RenderHost, RenderPageOptions,
    RenderState,
};

const RENDER_CAPABILITY: &str = "doc.render";

/// What both the capability and the tile manager's inputs read. Kept apart
/// from the manager so neither owns the other (the TypeScript's closures
/// share these variables).
struct Shared {
    ctx: Ctx<RenderState, RenderAction>,
    resolved: Rc<ResolvedRenderOptions>,
    host: RenderHost,
    /// The kernel's policy JSON and its parse.
    policy_memo: RefCell<Option<(Value, EngineRenderPolicy)>>,
    strategy_memo: RefCell<Option<(EngineRenderPolicy, Rc<ResolvedStrategy>)>>,
    warned_format: Cell<bool>,
    warned_budget: Cell<bool>,
    pending_advance: RefCell<IndexSet<u32>>,
    advance_scheduled: Cell<bool>,
}

impl Shared {
    /// Read the bound document's meta without cloning its page list when it
    /// is in the registry; `ctx.document()` otherwise (bring-up, or after
    /// close, when it is the staged meta).
    fn with_meta<R>(&self, f: impl FnOnce(Option<&DocumentMeta>) -> R) -> R {
        let core = self.ctx.core();
        if let Some(id) = self.ctx.document_id()
            && let Some(meta) = core.documents.get(id)
        {
            return f(Some(meta));
        }
        let meta = self.ctx.document();
        f(meta.as_ref())
    }

    /// The twin (permissions.md).
    fn can_render(&self) -> bool {
        self.ctx.doc().is_some_and(|doc| doc.security().allows(RENDER_CAPABILITY))
    }

    fn policy(&self) -> EngineRenderPolicy {
        self.with_meta(|meta| {
            let Some(meta) = meta else { return CONTINUOUS_RENDER_POLICY };
            let mut memo = self.policy_memo.borrow_mut();
            if let Some((raw, parsed)) = &*memo
                && *raw == meta.render_policy
            {
                return parsed.clone();
            }
            let parsed = EngineRenderPolicy::from_json(&meta.render_policy);
            *memo = Some((meta.render_policy.clone(), parsed.clone()));
            parsed
        })
    }

    fn strategy(&self) -> Rc<ResolvedStrategy> {
        let policy = self.policy();
        let memoized = match &*self.strategy_memo.borrow() {
            Some((p, strategy)) if *p == policy => Some(strategy.clone()),
            _ => None,
        };
        if let Some(strategy) = memoized {
            return strategy;
        }
        let strategy = Rc::new(resolve_strategy(&policy, &self.resolved));
        *self.strategy_memo.borrow_mut() = Some((policy, strategy.clone()));
        self.warn_dropped_format(&strategy);
        strategy
    }

    /// One-shot developer hint, not an error.
    fn warn_dropped_format(&self, strategy: &ResolvedStrategy) {
        let Some(requested) = self.resolved.format else { return };
        if self.warned_format.get() || strategy.format == Some(requested) {
            return;
        }
        self.warned_format.set(true);
        (self.host.warn)(&format!(
            "[render] format '{}' is not in the deployment's formats — using '{}'.",
            requested.as_str(),
            strategy.format.map_or("undefined", |f| f.as_str())
        ));
    }

    fn warn_past_budget_no_tiles(&self, demand_width: f64, supplied: f64) {
        if self.warned_budget.get() || self.resolved.tiles.enabled || demand_width <= supplied * 2.0
        {
            return;
        }
        self.warned_budget.set(true);
        (self.host.warn)(&format!(
            "[render] demand is {}× over the base budget and the tile plane is disabled (tiles: false) — the page rests blurry. Raise fullPage.maxWidth or re-enable tiles.",
            js::to_fixed(demand_width / supplied, 1)
        ));
    }

    fn page_size(&self, pon: u32) -> Option<PageSizePt> {
        self.with_meta(|meta| {
            let pages = meta?.pages.as_array()?;
            let page = pages.iter().find(|p| {
                p.get("pageObjectNumber").and_then(Value::as_f64) == Some(f64::from(pon))
            })?;
            let size = page.get("size")?;
            Some(PageSizePt {
                width: size.get("width").and_then(Value::as_f64).unwrap_or(f64::NAN),
                height: size.get("height").and_then(Value::as_f64).unwrap_or(f64::NAN),
            })
        })
    }

    fn page_width_of(&self, pon: u32) -> Result<f64, Error> {
        self.page_size(pon)
            .map(|size| size.width)
            .ok_or_else(|| Error::new(format!("render: unknown page object number {pon}")))
    }

    /// The one base-sizing path: renderPage, renderSourceKey,
    /// conformViewport and (through the tile manager) engagement.
    fn conform_viewport(&self, pon: u32, scale: f64) -> Result<PageRenderViewport, Error> {
        let strategy = self.strategy();
        let demand_width = scale * self.page_width_of(pon)?;
        let width = base_ask_width(&strategy, demand_width);
        self.warn_past_budget_no_tiles(demand_width, width);
        Ok(PageRenderViewport::Width { width })
    }

    /// `Err` only once the document's slice is gone, where the TypeScript
    /// reads a member of `undefined`.
    fn epoch_of(&self, pon: u32, include_annotations: bool) -> Result<f64, Error> {
        let Some(state) = self.ctx.try_state() else {
            return Err(Error::named(
                "TypeError",
                "Cannot read properties of undefined (reading 'contentEpochs')",
            ));
        };
        let content = state.content_epochs.get(&pon).copied().unwrap_or(0);
        if !include_annotations {
            return Ok(content as f64);
        }
        Ok((content + state.annotated_epochs.get(&pon).copied().unwrap_or(0)) as f64)
    }

    /// A raster's identity: conformed width, annotations flag, epoch, and the
    /// encode format when one is set.
    fn base_key(
        &self,
        pon: u32,
        viewport: PageRenderViewport,
        annotations: bool,
    ) -> Result<String, Error> {
        let format = self.strategy().format;
        let width = match viewport {
            PageRenderViewport::Width { width } => width,
            PageRenderViewport::Scale { .. } => 0.0,
        };
        Ok(format!(
            "{pon}|w{}|a{}|e{}{}",
            num(width),
            u8::from(annotations),
            num(self.epoch_of(pon, annotations)?),
            format.map_or(String::new(), |f| format!("|f{}", f.as_str()))
        ))
    }

    /// Tile resolutions arrive in bursts: coalesce the wake-ups per frame.
    fn wake(self: &Rc<Self>, pon: u32) {
        self.pending_advance.borrow_mut().insert(pon);
        if self.advance_scheduled.get() {
            return;
        }
        self.advance_scheduled.set(true);
        let this = self.clone();
        (self.host.request_frame)(Box::new(move || {
            this.advance_scheduled.set(false);
            let pons: Vec<u32> =
                std::mem::take(&mut *this.pending_advance.borrow_mut()).into_iter().collect();
            for pon in pons {
                this.ctx.dispatch(RenderAction::PaintAdvanced { pon });
            }
        }));
    }

    /// The encode options both planes add to a render call.
    fn encode_options(&self, options: &mut serde_json::Map<String, Value>) {
        let strategy = self.strategy();
        if let Some(format) = strategy.format {
            options.insert("format".into(), json!(format.as_str()));
        }
        if let Some(quality) = strategy.quality {
            options.insert("quality".into(), json!(quality));
        }
    }
}

/// `doc.page(pon).render.image(options)`, aborted with the store's signal.
fn render_image(
    doc: &Rc<dyn DocumentHandle>,
    pon: u32,
    options: Value,
    signal: &AbortSignal,
) -> Result<Promise<Value>, Error> {
    doc.page_index(pon)?;
    let task = doc.call("page.render.image", Some(pon), vec![options]);
    match signal.reason() {
        Some(reason) => task.abort(Some(reason.to_canonical())),
        None => {
            let task = task.clone();
            signal.on_abort(Rc::new(move |reason: &Error| task.abort(Some(reason.to_canonical()))));
        }
    }
    Ok(task)
}

struct CapInner {
    shared: Rc<Shared>,
    /// Document-lifetime cache; staleness is a new key, never a flush.
    store: RasterStore,
    tiles: TileManager,
    view_tiles: RefCell<HashMap<String, ViewTiles>>,
}

/// `RenderCapability`. Cheap to clone; clones are the same capability.
#[derive(Clone)]
pub struct RenderCapability(Rc<CapInner>);

struct ViewTilesInner {
    view: String,
    tiles: TileManager,
}

/// `ViewTiles`: one view's tile surface, identity bound at creation. The
/// same view gets the same handle (`ViewTiles::same`).
#[derive(Clone)]
pub struct ViewTiles(Rc<ViewTilesInner>);

impl ViewTiles {
    /// The tile paint plan for a page under this view's demand; also
    /// schedules the want set's fetches. `include_annotations: None` is true.
    pub fn plan(
        &self,
        pon: u32,
        demand: &PageViewDemand,
        include_annotations: Option<bool>,
    ) -> Result<Rc<TilePaintPlan>, Error> {
        self.0.tiles.plan(&self.0.view, pon, demand, include_annotations.unwrap_or(true))
    }

    /// The painter's report: this plan key had a presentation opportunity.
    pub fn painted(&self, pon: u32, key: &str) {
        self.0.tiles.source_painted(&self.0.view, pon, key);
    }

    /// This plan key's image left the screen.
    pub fn unpainted(&self, pon: u32, key: &str) {
        self.0.tiles.source_unpainted(&self.0.view, pon, key);
    }

    /// This view unmounted its tile plane for the page.
    pub fn release(&self, pon: u32) {
        self.0.tiles.release_page(&self.0.view, pon);
    }

    pub fn view(&self) -> &str {
        &self.0.view
    }

    /// `a === b`.
    pub fn same(&self, other: &ViewTiles) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// `createRenderCapability`.
pub fn create_render_capability(
    ctx: Ctx<RenderState, RenderAction>,
    options: &RenderPluginOptions,
    host: RenderHost,
) -> RenderCapability {
    let resolved = Rc::new(resolve_render_options(options));
    let shared = Rc::new(Shared {
        ctx,
        resolved: resolved.clone(),
        host: host.clone(),
        policy_memo: RefCell::new(None),
        strategy_memo: RefCell::new(None),
        warned_format: Cell::new(false),
        warned_budget: Cell::new(false),
        pending_advance: RefCell::new(IndexSet::new()),
        advance_scheduled: Cell::new(false),
    });
    let store = RasterStore::default();

    let fetch_shared = shared.clone();
    let debug_sink = host.debug.clone();
    let tiles = TileManager::new(TileManagerDeps {
        store: store.clone(),
        options: resolved.clone(),
        get_policy: {
            let shared = shared.clone();
            Rc::new(move || shared.policy())
        },
        get_page_size: {
            let shared = shared.clone();
            Rc::new(move |pon| shared.page_size(pon))
        },
        get_epoch: {
            let shared = shared.clone();
            Rc::new(move |pon, annotations| shared.epoch_of(pon, annotations))
        },
        fetch_tile: Rc::new(move |pon, rect, scale, include_annotations, signal| {
            let shared = &fetch_shared;
            let Some(doc) = shared.ctx.doc() else {
                return Ok(Promise::rejected(Error::new("render: no document bound")));
            };
            // The refusal the engine would send, without the round trip.
            if !shared.can_render() {
                return Ok(Promise::rejected(Error::permission_denied(
                    RENDER_CAPABILITY,
                    Some("render.tile"),
                )));
            }
            let mut options = serde_json::Map::new();
            options.insert("target".into(), json!({ "kind": "rect", "rect": rect }));
            options.insert("viewport".into(), json!({ "kind": "scale", "scale": scale }));
            options.insert("includeAnnotations".into(), json!(include_annotations));
            shared.encode_options(&mut options);
            render_image(&doc, pon, Value::Object(options), signal)
        }),
        on_advance: {
            let shared = shared.clone();
            Rc::new(move |pon| shared.wake(pon))
        },
        debug: resolved.debug.then(|| {
            Rc::new(move |message: &str| debug_sink(&format!("[render] {message}")))
                as Rc<dyn Fn(&str)>
        }),
        warn: host.warn.clone(),
    });

    RenderCapability(Rc::new(CapInner {
        shared,
        store,
        tiles,
        view_tiles: RefCell::new(HashMap::new()),
    }))
}

impl RenderCapability {
    fn shared(&self) -> &Rc<Shared> {
        &self.0.shared
    }

    /// The twin: would page rasters be served to this session? Mirrors
    /// `doc.render`.
    pub fn can_render(&self) -> bool {
        self.shared().can_render()
    }

    /// Render a page to an encoded image through the raster store. `Err` is
    /// a synchronous throw (an unknown page, a page the engine refuses); a
    /// refusal or engine failure rejects the promise.
    pub fn render_page(
        &self,
        pon: u32,
        options: RenderPageOptions,
    ) -> Result<Promise<PageImageHandle>, Error> {
        let shared = self.shared();
        let Some(doc) = shared.ctx.doc() else {
            return Ok(Promise::rejected(Error::new("render: no document bound")));
        };
        if !shared.can_render() {
            return Ok(Promise::rejected(Error::permission_denied(
                RENDER_CAPABILITY,
                Some("render.renderPage"),
            )));
        }
        let annotations = options.include_annotations.unwrap_or(true);
        let viewport = shared.conform_viewport(pon, options.scale)?;
        let key = shared.base_key(pon, viewport, annotations)?;
        self.0.store.acquire(
            &key,
            |store_signal| {
                let mut request = serde_json::Map::new();
                request
                    .insert("viewport".into(), serde_json::to_value(viewport).unwrap_or_default());
                request.insert("includeAnnotations".into(), json!(annotations));
                shared.encode_options(&mut request);
                render_image(&doc, pon, Value::Object(request), store_signal)
            },
            options.signal.as_ref(),
        )
    }

    /// The identity of the raster `render_page` would produce.
    pub fn render_source_key(
        &self,
        pon: u32,
        scale: f64,
        include_annotations: Option<bool>,
    ) -> Result<String, Error> {
        let annotations = include_annotations.unwrap_or(true);
        let viewport = self.shared().conform_viewport(pon, scale)?;
        self.shared().base_key(pon, viewport, annotations)
    }

    /// The canonical (width-kind) viewport a desired scale conforms to.
    pub fn conform_viewport(&self, pon: u32, scale: f64) -> Result<PageRenderViewport, Error> {
        self.shared().conform_viewport(pon, scale)
    }

    pub fn paint_settings(&self) -> PaintSettings {
        let resolved = &self.shared().resolved;
        PaintSettings { fade_ms: resolved.tiles.fade_ms, tiles: resolved.tiles.enabled }
    }

    /// The document's advertised policy.
    pub fn render_policy(&self) -> EngineRenderPolicy {
        self.shared().policy()
    }

    /// This view's tile surface, one handle per view.
    pub fn tiles_for(&self, view: &str) -> ViewTiles {
        self.0
            .view_tiles
            .borrow_mut()
            .entry(view.to_string())
            .or_insert_with(|| {
                ViewTiles(Rc::new(ViewTilesInner {
                    view: view.to_string(),
                    tiles: self.0.tiles.clone(),
                }))
            })
            .clone()
    }

    /// The version of the raster these options would produce. `Err` only
    /// after the document closed.
    pub fn render_epoch(&self, pon: u32, include_annotations: bool) -> Result<f64, Error> {
        self.shared().epoch_of(pon, include_annotations)
    }

    /// Declare that page pixels changed. `pons: None` is every page, `scope:
    /// None` is `content`.
    pub fn invalidate(&self, options: InvalidateOptions) {
        let shared = self.shared();
        let target = options.pons.unwrap_or_else(|| all_pons_of(&shared.ctx));
        if !target.is_empty() {
            shared.ctx.dispatch(RenderAction::Invalidate {
                scope: options.scope.unwrap_or(InvalidateScope::Content),
                pons: target,
            });
        }
    }

    /// The raster store both planes share.
    pub fn raster_store(&self) -> &RasterStore {
        &self.0.store
    }

    /// The tile manager behind `tiles_for`.
    pub fn tile_manager(&self) -> &TileManager {
        &self.0.tiles
    }
}
