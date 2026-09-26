//! `controller.ts`: the render controller, the one place STRATEGY (the
//! config, what the viewer spends) composes with POLICY (the engine fact,
//! what the deployment serves). Conforming demand to the resolved render
//! points, collapsing same-key asks in the raster store, exposing stable
//! source keys and turning a view's demand into a retention-safe tile plan
//! all happen here.
//!
//! Two doors for rasters: `render_page` (public, exact size) and
//! `render_source` (host, conformed). Both share the raster store. Two doors
//! for staleness: confirmed document events (see `connect`) and the
//! `invalidate` verb. Both go through `publish_invalidation`, the one place
//! the ledger bumps and `onInvalidated` fires.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::{Rc, Weak};

use epdf_geometry::Rect;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::{self as kgeo, PageRef};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::types::{DocumentMeta, PluginErrorInfo, origin_of};
use epdf_kernel::{Controller, Error};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value, json};

use crate::contract::{
    InvalidateOptions, InvalidateScope, PageImage, PageRender, PageRenderFailure,
    RenderBatchResult, RenderConfig, RenderInvalidatedEvent, RenderPageOptions, RenderPagesOptions,
    RenderThumbnailOptions,
};
use crate::host_contract::{
    PaintSettings, RenderCompletedEvent, RenderFailedEvent, RenderHost, RenderSourceOptions,
};
use crate::invalidation::annotated_pons;
use crate::js::{self, number_to_string as num};
use crate::model::{RenderAction, RenderState};
use crate::paint_plan::{
    PageViewDemand, RenderFormat, ResolvedRenderOptions, TilePaintPlan, empty_tile_plan,
    resolve_render_options,
};
use crate::policy::{CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, PageRenderViewport};
use crate::raster_store::{PageImageHandle, RasterStore};
use crate::strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
use crate::tile_manager::{TileManager, TileManagerDeps};
use crate::tiles::PageSizePt;

const RENDER_SCOPE: &str = "doc.render";
const DEFAULT_BATCH_CONCURRENCY: f64 = 4.0;

/// `SYSTEM_ORIGIN`: engine events without an origin (the stream's own
/// housekeeping).
fn system_origin() -> Value {
    json!({ "locality": "local", "trigger": "system", "sessionId": null, "actorId": null })
}

/// `API_ORIGIN`: an `invalidate` call.
fn api_origin() -> Value {
    json!({ "locality": "local", "trigger": "api", "sessionId": null, "actorId": null })
}

/// `'origin' in event && event.origin ? originOf(event) : SYSTEM_ORIGIN`.
fn origin_of_event(event: &Value) -> Value {
    match event.get("origin") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => system_origin(),
        Some(Value::String(s)) if s.is_empty() => system_origin(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => system_origin(),
        Some(_) => origin_of(event),
    }
}

fn error_info(error: &Error) -> PluginErrorInfo {
    let text =
        |key: &str| error.member(key).and_then(Value::as_str).unwrap_or_default().to_string();
    PluginErrorInfo {
        code: text("code"),
        message: error.message.clone(),
        capability: text("capability"),
    }
}

/// What a raster request sends the engine besides its target.
#[derive(Clone, Copy)]
struct RasterRequest {
    viewport: PageRenderViewport,
    include_annotations: bool,
    format: Option<RenderFormat>,
    quality: Option<f64>,
}

/// A view's live demand for one page, as last set through its handle.
struct PageDemand {
    demand: PageViewDemand,
    include_annotations: bool,
    plan: Rc<TilePaintPlan>,
}

type Pages = Rc<RefCell<IndexMap<u32, PageDemand>>>;

/// One view id's entry: its handle (held weakly, so the handle's reference
/// to the controller does not make a cycle), its reference count and its
/// per-page demands.
struct ViewRecord {
    handle: Weak<ViewInner>,
    refs: Rc<Cell<u32>>,
    pages: Pages,
}

struct Core {
    cx: Controller<RenderState, RenderAction>,
    resolved: Rc<ResolvedRenderOptions>,
    host: RenderHost,
    /// Document-lifetime cache; staleness is a new key, never a flush.
    store: RasterStore,
    tiles: TileManager,
    invalidated: EventHookSource<RenderInvalidatedEvent>,
    render_completed: EventHookSource<RenderCompletedEvent>,
    render_failed: EventHookSource<RenderFailedEvent>,
    /// The kernel's policy JSON and its parse.
    policy_memo: RefCell<Option<(Value, EngineRenderPolicy)>>,
    strategy_memo: RefCell<Option<(EngineRenderPolicy, Rc<ResolvedStrategy>)>>,
    warned_format: Cell<bool>,
    warned_budget: Cell<bool>,
    pending_advance: RefCell<IndexSet<u32>>,
    advance_scheduled: Cell<bool>,
    views: RefCell<IndexMap<String, ViewRecord>>,
    paint_settings: PaintSettings,
}

impl Core {
    /// Read the bound document's meta without cloning its page list when it
    /// is in the registry; `ctx.document()` otherwise (bring-up, or after
    /// close, when it is the staged meta).
    fn with_meta<R>(&self, f: impl FnOnce(Option<&DocumentMeta>) -> R) -> R {
        let core = self.cx.core();
        if let Some(id) = self.cx.document_id()
            && let Some(meta) = core.documents.get(id)
        {
            return f(Some(meta));
        }
        let meta = self.cx.document();
        f(meta.as_ref())
    }

    /// The twin (permissions.md).
    fn can_render(&self) -> bool {
        self.cx.doc().is_ok_and(|doc| doc.security().allows(RENDER_SCOPE))
    }

    fn assert_can_render(&self, operation: &str) -> Result<(), Error> {
        if self.can_render() {
            return Ok(());
        }
        Err(plugin_error(
            code::PERMISSION_DENIED,
            "render",
            format!("{operation} requires {RENDER_SCOPE}"),
            None,
            Some(json!({ "required": RENDER_SCOPE })),
        ))
    }

    /// The policy is a document fact on the kernel's registry.
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

    /// `ctx.getPage(ref)?.size`.
    fn page_size(&self, page: PageRef) -> Option<PageSizePt> {
        self.with_meta(|meta| {
            let pages = meta?.pages.as_array()?;
            let size = pages.iter().find(|p| page.names(p))?.get("size")?;
            Some(PageSizePt {
                width: size.get("width").and_then(Value::as_f64).unwrap_or(f64::NAN),
                height: size.get("height").and_then(Value::as_f64).unwrap_or(f64::NAN),
            })
        })
    }

    /// `not-found` for a page this document does not have.
    fn page_width_of(&self, page: PageRef) -> Result<f64, Error> {
        match self.page_size(page) {
            Some(size) => Ok(size.width),
            None => {
                self.cx.assert_page_ref(page)?;
                Err(Error::new(format!("render: no size for page {}", page.page_object_number)))
            }
        }
    }

    fn all_pons(&self) -> Vec<u32> {
        self.with_meta(|meta| {
            meta.and_then(|m| m.pages.as_array())
                .into_iter()
                .flatten()
                .filter_map(|p| p.get("ref").and_then(PageRef::from_json))
                .map(|r| r.page_object_number)
                .collect()
        })
    }

    /// The one base-sizing path: `render_source`, `get_source_key`,
    /// `conform_viewport` and (through the tile manager) engagement.
    fn conform_viewport(&self, page: PageRef, scale: f64) -> Result<PageRenderViewport, Error> {
        let demand_width = scale * self.page_width_of(page)?;
        let width = base_ask_width(&self.strategy(), demand_width);
        self.warn_past_budget_no_tiles(demand_width, width);
        Ok(PageRenderViewport::Width { width })
    }

    fn epoch_of(&self, pon: u32, include_annotations: bool) -> f64 {
        let state = self.cx.state();
        let content = state.content_epochs.get(&pon).copied().unwrap_or(0);
        if !include_annotations {
            return content as f64;
        }
        (content + state.annotated_epochs.get(&pon).copied().unwrap_or(0)) as f64
    }

    /// A raster's identity includes its encode format (the resolved format
    /// can change under a live store) and an explicit quality.
    fn raster_key(
        &self,
        pon: u32,
        width: f64,
        annotations: bool,
        format: Option<RenderFormat>,
        quality: Option<f64>,
    ) -> String {
        format!(
            "{pon}|w{}|a{}|e{}{}{}",
            num(width),
            u8::from(annotations),
            num(self.epoch_of(pon, annotations)),
            format.map_or(String::new(), |f| format!("|f{}", f.as_str())),
            quality.map_or(String::new(), |q| format!("|q{}", num(q)))
        )
    }

    /// The raster store door, shared by both render doors. `Err` is a
    /// synchronous throw (the engine refusing a page the registry lists).
    fn acquire_raster(
        self: &Rc<Self>,
        page: PageRef,
        key: String,
        request: RasterRequest,
        signal: Option<&AbortSignal>,
    ) -> Result<Promise<PageImageHandle>, Error> {
        let result = self.store.acquire(
            &key,
            |store_signal| {
                let mut options = Map::new();
                options.insert(
                    "viewport".into(),
                    serde_json::to_value(request.viewport).unwrap_or_default(),
                );
                options.insert("includeAnnotations".into(), json!(request.include_annotations));
                if let Some(format) = request.format {
                    options.insert("format".into(), json!(format.as_str()));
                }
                if let Some(quality) = request.quality {
                    options.insert("quality".into(), json!(quality));
                }
                render_image(&self.cx.doc()?, page, Value::Object(options), store_signal)
            },
            signal,
        )?;
        let this = self.clone();
        Ok(Promise::spawn(async move {
            match result.await {
                Ok(image) => {
                    this.render_completed.emit(&RenderCompletedEvent { page, key });
                    Ok(image)
                }
                Err(error) => {
                    let mapped = to_plugin_error("render", &error);
                    if !mapped.is_plugin_error(Some(code::OPERATION_CANCELLED))
                        && !mapped.is_plugin_error(Some(code::INSTANCE_CLOSED))
                    {
                        this.render_failed.emit(&RenderFailedEvent {
                            page,
                            key,
                            error: error_info(&mapped),
                        });
                    }
                    Err(mapped)
                }
            }
        }))
    }

    fn render_source(
        self: &Rc<Self>,
        page: PageRef,
        options: RenderSourceOptions,
    ) -> Promise<PageImage> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_can_render("render.renderSource")?;
            let annotations = options.include_annotations.unwrap_or(true);
            let viewport = this.conform_viewport(page, options.scale)?;
            let strategy = this.strategy();
            let key = this.raster_key(
                page.page_object_number,
                viewport_width(viewport),
                annotations,
                strategy.format,
                None,
            );
            let request = RasterRequest {
                viewport,
                include_annotations: annotations,
                format: strategy.format,
                quality: strategy.quality,
            };
            let raster = this.acquire_raster(page, key, request, options.signal.as_ref())?;
            adopt(raster).await
        })
    }

    fn render_page(
        self: &Rc<Self>,
        page: PageRef,
        options: RenderPageOptions,
    ) -> Promise<PageImage> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_can_render("render.renderPage")?;
            let page_width = this.page_width_of(page)?;
            let width = js::max(
                1.0,
                js::round(options.width.unwrap_or(options.scale.unwrap_or(1.0) * page_width)),
            );
            let annotations = options.include_annotations.unwrap_or(true);
            let strategy = this.strategy();
            let format = options.format.or(strategy.format);
            let quality = options.quality.or(strategy.quality);
            // A width that matches a conformed raster (same format, default
            // quality) is the same key, so the cached image serves it.
            let key = this.raster_key(
                page.page_object_number,
                width,
                annotations,
                format,
                options.quality,
            );
            let request = RasterRequest {
                viewport: PageRenderViewport::Width { width },
                include_annotations: annotations,
                format,
                quality,
            };
            let raster = this.acquire_raster(page, key, request, options.signal.as_ref())?;
            adopt(raster).await
        })
    }

    fn render_pages(
        self: &Rc<Self>,
        pages: Vec<PageRef>,
        options: RenderPagesOptions,
    ) -> Promise<RenderBatchResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.assert_can_render("render.renderPages")?;
            let concurrency =
                js::max(1.0, options.concurrency.unwrap_or(DEFAULT_BATCH_CONCURRENCY));
            let applied: Rc<RefCell<Vec<PageRender>>> = Rc::default();
            let failed: Rc<RefCell<Vec<PageRenderFailure>>> = Rc::default();
            let queue: Rc<RefCell<VecDeque<PageRef>>> =
                Rc::new(RefCell::new(pages.iter().copied().collect()));
            // `Array.from({length: Math.min(concurrency, queue.length)})`.
            let workers = js::min(concurrency, queue.borrow().len() as f64);
            let workers = if workers.is_nan() { 0 } else { workers as usize };
            let mut running = Vec::with_capacity(workers);
            for _ in 0..workers {
                let (this, queue) = (this.clone(), queue.clone());
                let (applied, failed) = (applied.clone(), failed.clone());
                let page_options = options.page.clone();
                running.push(Promise::spawn(async move {
                    loop {
                        let Some(page) = queue.borrow_mut().pop_front() else { break };
                        match this.render_page(page, page_options.clone()).await {
                            Ok(image) => applied.borrow_mut().push(PageRender { page, image }),
                            Err(error) => failed.borrow_mut().push(PageRenderFailure {
                                page,
                                error: error_info(&to_plugin_error("render", &error)),
                            }),
                        }
                    }
                    Ok(())
                }));
            }
            all(running).await?;
            // Report in input order, whatever the completion order was. A
            // repeated page sorts at its last index, as a `Map` keeps it.
            let index: HashMap<u32, usize> =
                pages.iter().enumerate().map(|(i, p)| (p.page_object_number, i)).collect();
            let at = |p: &PageRef| index.get(&p.page_object_number).copied().unwrap_or(0);
            let mut applied = std::mem::take(&mut *applied.borrow_mut());
            let mut failed = std::mem::take(&mut *failed.borrow_mut());
            applied.sort_by_key(|a| at(&a.page));
            failed.sort_by_key(|f| at(&f.page));
            Ok(RenderBatchResult { applied, skipped: Vec::new(), failed })
        })
    }

    /// The one place the ledger bumps and `onInvalidated` fires.
    fn publish_invalidation(
        &self,
        pons: Vec<u32>,
        scope: InvalidateScope,
        origin: Value,
    ) -> Result<(), Error> {
        if pons.is_empty() {
            return Ok(());
        }
        self.cx.dispatch(RenderAction::Invalidate { scope, pages: pons.clone() });
        // Every view's plan for these pages points at old-epoch pixels:
        // re-plan now, so `get_plan` reads fresh.
        for &pon in &pons {
            self.replan_page(pon)?;
        }
        self.invalidated.emit(&RenderInvalidatedEvent {
            pages: pons.into_iter().map(PageRef::new).collect(),
            scope,
            origin,
        });
        Ok(())
    }

    /// Tile resolutions arrive in bursts: coalesce the re-plans per frame.
    fn wake(self: &Rc<Self>, pon: u32) {
        self.pending_advance.borrow_mut().insert(pon);
        if self.advance_scheduled.get() {
            return;
        }
        self.advance_scheduled.set(true);
        let this = Rc::downgrade(self);
        (self.host.request_frame)(Box::new(move || {
            let Some(this) = this.upgrade() else { return };
            this.advance_scheduled.set(false);
            let pons: Vec<u32> =
                std::mem::take(&mut *this.pending_advance.borrow_mut()).into_iter().collect();
            for pon in pons {
                // A throw here would be an uncaught error in a frame callback.
                let _ = this.replan_page(pon);
            }
        }));
    }

    /// Re-plan a page for every view that wants it (schedules the next want
    /// set), and wake subscribers once when any plan changed.
    fn replan_page(&self, pon: u32) -> Result<(), Error> {
        let views: Vec<(String, Pages)> =
            self.views.borrow().iter().map(|(id, v)| (id.clone(), v.pages.clone())).collect();
        let mut changed = false;
        for (view_id, pages) in views {
            let Some((demand, include_annotations)) =
                pages.borrow().get(&pon).map(|e| (e.demand, e.include_annotations))
            else {
                continue;
            };
            let plan = self.tiles.plan(&view_id, pon, &demand, include_annotations)?;
            if let Some(entry) = pages.borrow_mut().get_mut(&pon)
                && !Rc::ptr_eq(&plan, &entry.plan)
            {
                entry.plan = plan;
                changed = true;
            }
        }
        if changed {
            self.cx.dispatch(RenderAction::PaintAdvanced { page: pon });
        }
        Ok(())
    }

    fn on_document_event(&self, event: &Value) {
        // A throw from a listener has nowhere to go.
        let _ = if event.get("type").and_then(Value::as_str) == Some("redaction.applied") {
            let pons: Vec<u32> = event
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|r| r.get("status").and_then(Value::as_str) == Some("applied"))
                .filter_map(|r| r.get("page").and_then(PageRef::from_json))
                .map(|r| r.page_object_number)
                .collect();
            self.publish_invalidation(pons, InvalidateScope::Content, origin_of_event(event))
        } else {
            let pons = annotated_pons(event, || self.all_pons());
            self.publish_invalidation(pons, InvalidateScope::Annotations, origin_of_event(event))
        };
    }
}

fn viewport_width(viewport: PageRenderViewport) -> f64 {
    match viewport {
        PageRenderViewport::Width { width } => width,
        PageRenderViewport::Scale { .. } => 0.0,
    }
}

/// `return promise` from an `async` function: the outer promise adopts it
/// as a thenable, one turn for the adoption job and one for its reaction.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.await
}

/// `Promise.all` over promises that never reject: settles one turn after the
/// last one does, at once for none.
fn all(promises: Vec<Promise<()>>) -> Promise<()> {
    if promises.is_empty() {
        return Promise::resolved(());
    }
    let (all, resolver) = Promise::pending();
    let remaining = Rc::new(Cell::new(promises.len()));
    for promise in promises {
        let (remaining, resolver) = (remaining.clone(), resolver.clone());
        promise.on_settle(move |_| {
            rt::spawn(async move {
                rt::tick().await;
                remaining.set(remaining.get() - 1);
                if remaining.get() == 0 {
                    resolver.resolve(());
                }
            });
        });
    }
    all
}

/// `ctx.doc.page(page).render.image(options)`, aborted with `signal`. The
/// TypeScript hands the engine the signal's reason only when it is a string;
/// a Rust abort reason is always an error, so the engine gets none.
fn render_image(
    doc: &Rc<dyn DocumentHandle>,
    page: PageRef,
    options: Value,
    signal: &AbortSignal,
) -> Result<Promise<Value>, Error> {
    let pon = page.page_object_number;
    doc.page_index(pon)?;
    let task = doc.call("page.render.image", Some(pon), vec![options]);
    if signal.aborted() {
        task.abort(None);
    } else {
        let task = task.clone();
        signal.on_abort(Rc::new(move |_| task.abort(None)));
    }
    Ok(task)
}

// ── the capability ────────────────────────────────────────────────────────────

/// `RenderHostCapability`, the public lens (`RenderCapability` in the
/// TypeScript) and the host lens in one type. Cheap to clone; clones are the
/// same capability.
#[derive(Clone)]
pub struct RenderCapability(Rc<Core>);

impl RenderCapability {
    // ── public lens ──

    /// Would page rasters be served to this session (`doc.render`)?
    pub fn can_render(&self) -> bool {
        self.0.can_render()
    }

    /// Render one page at the requested size, exactly. Rejects
    /// `permission-denied` without `doc.render`, `not-found` for a page not
    /// in this document, `operation-cancelled` on abort.
    pub fn render_page(&self, page: PageRef, options: RenderPageOptions) -> Promise<PageImage> {
        self.0.render_page(page, options)
    }

    /// A small raster at `max_width` device pixels.
    pub fn render_thumbnail(
        &self,
        page: PageRef,
        options: RenderThumbnailOptions,
    ) -> Promise<PageImage> {
        self.0.render_page(
            page,
            RenderPageOptions {
                width: Some(options.max_width),
                include_annotations: options.include_annotations,
                signal: options.signal,
                ..Default::default()
            },
        )
    }

    /// Render several pages with bounded concurrency; best-effort per page.
    pub fn render_pages(
        &self,
        pages: Vec<PageRef>,
        options: RenderPagesOptions,
    ) -> Promise<RenderBatchResult> {
        self.0.render_pages(pages, options)
    }

    /// The deployment's advertised render policy (a document fact).
    pub fn get_render_policy(&self) -> EngineRenderPolicy {
        self.0.policy()
    }

    /// The version of the raster these options would produce.
    pub fn get_render_epoch(&self, page: PageRef, include_annotations: bool) -> f64 {
        self.0.epoch_of(page.page_object_number, include_annotations)
    }

    /// Declare that page pixels changed. `Err` where re-planning a view's
    /// tiles throws (the engine refusing a page the registry lists).
    pub fn invalidate(&self, options: InvalidateOptions) -> Result<(), Error> {
        let pons = match options.pages {
            Some(pages) => pages.into_iter().map(|p| p.page_object_number).collect(),
            None => self.0.all_pons(),
        };
        self.0.publish_invalidation(
            pons,
            options.scope.unwrap_or(InvalidateScope::Content),
            api_origin(),
        )
    }

    /// `onInvalidated`.
    pub fn on_invalidated(
        &self,
        listener: impl Fn(&RenderInvalidatedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.invalidated.on(listener)
    }

    // ── host lens ──

    /// The viewer door: render at the scale a view shows the page,
    /// conformed through the resolved render points.
    pub fn render_source(&self, page: PageRef, options: RenderSourceOptions) -> Promise<PageImage> {
        self.0.render_source(page, options)
    }

    /// The identity of the raster `render_source` would produce. `Err` is
    /// `not-found` for a page not in this document.
    pub fn get_source_key(
        &self,
        page: PageRef,
        scale: f64,
        include_annotations: Option<bool>,
    ) -> Result<String, Error> {
        let viewport = self.0.conform_viewport(page, scale)?;
        Ok(self.0.raster_key(
            page.page_object_number,
            viewport_width(viewport),
            include_annotations.unwrap_or(true),
            self.0.strategy().format,
            None,
        ))
    }

    /// The canonical (width-kind) viewport a desired scale conforms to.
    pub fn conform_viewport(&self, page: PageRef, scale: f64) -> Result<PageRenderViewport, Error> {
        self.0.conform_viewport(page, scale)
    }

    pub fn get_paint_settings(&self) -> PaintSettings {
        self.0.paint_settings
    }

    /// This view's tile surface. Stable per view id; pair with `dispose`.
    pub fn create_view_demand(&self, view_id: &str) -> ViewDemand {
        let mut views = self.0.views.borrow_mut();
        if let Some(record) = views.get_mut(view_id) {
            record.refs.set(record.refs.get() + 1);
            if let Some(handle) = record.handle.upgrade() {
                return ViewDemand(handle);
            }
            let handle = Rc::new(ViewInner {
                view_id: view_id.to_string(),
                core: self.0.clone(),
                refs: record.refs.clone(),
                pages: record.pages.clone(),
            });
            record.handle = Rc::downgrade(&handle);
            return ViewDemand(handle);
        }
        let handle = Rc::new(ViewInner {
            view_id: view_id.to_string(),
            core: self.0.clone(),
            refs: Rc::new(Cell::new(1)),
            pages: Rc::default(),
        });
        views.insert(
            view_id.to_string(),
            ViewRecord {
                handle: Rc::downgrade(&handle),
                refs: handle.refs.clone(),
                pages: handle.pages.clone(),
            },
        );
        ViewDemand(handle)
    }

    /// `onRenderCompleted`: a base raster resolved (either door).
    pub fn on_render_completed(
        &self,
        listener: impl Fn(&RenderCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.render_completed.on(listener)
    }

    /// `onRenderFailed`: a base raster failed (not an abort or a close).
    pub fn on_render_failed(
        &self,
        listener: impl Fn(&RenderFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.render_failed.on(listener)
    }

    /// The raster store both planes share.
    pub fn raster_store(&self) -> &RasterStore {
        &self.0.store
    }

    /// The tile manager behind the view demands.
    pub fn tile_manager(&self) -> &TileManager {
        &self.0.tiles
    }
}

// ── view demands ──────────────────────────────────────────────────────────────

struct ViewInner {
    view_id: String,
    core: Rc<Core>,
    refs: Rc<Cell<u32>>,
    pages: Pages,
}

/// `ViewDemand`: one view's tile surface, identity bound at creation. Two
/// views showing the same page plan independently. `set_demand` is the one
/// call that schedules fetches and re-plans; `get_plan` is pure and returns
/// the same `Rc` until the demand, an epoch or a tile arrival changes it.
#[derive(Clone)]
pub struct ViewDemand(Rc<ViewInner>);

impl ViewDemand {
    /// What this view wants for a page right now. `include_annotations:
    /// None` is true. `Err` where planning throws (the engine refusing a
    /// page the registry lists).
    pub fn set_demand(
        &self,
        page: PageRef,
        demand: PageViewDemand,
        include_annotations: Option<bool>,
    ) -> Result<(), Error> {
        let inner = &self.0;
        let pon = page.page_object_number;
        let include_annotations = include_annotations.unwrap_or(true);
        let previous = inner.pages.borrow().get(&pon).map(|e| e.plan.clone());
        let plan = inner.core.tiles.plan(&inner.view_id, pon, &demand, include_annotations)?;
        let changed = previous.is_none_or(|p| !Rc::ptr_eq(&p, &plan));
        inner.pages.borrow_mut().insert(pon, PageDemand { demand, include_annotations, plan });
        if changed {
            inner.core.cx.dispatch(RenderAction::PaintAdvanced { page: pon });
        }
        Ok(())
    }

    /// The current paint plan for a page under this view's demand.
    pub fn get_plan(&self, page: PageRef) -> Rc<TilePaintPlan> {
        self.0
            .pages
            .borrow()
            .get(&page.page_object_number)
            .map_or_else(empty_tile_plan, |e| e.plan.clone())
    }

    /// The image for this plan key had a presentation opportunity.
    pub fn mark_painted(&self, page: PageRef, key: &str) {
        self.0.core.tiles.source_painted(&self.0.view_id, page.page_object_number, key);
    }

    /// This plan key's element left the DOM.
    pub fn mark_unpainted(&self, page: PageRef, key: &str) {
        self.0.core.tiles.source_unpainted(&self.0.view_id, page.page_object_number, key);
    }

    /// This view unmounted its tile plane for the page: abort its in-flight
    /// tile fetches and drop its bookkeeping (resolved bytes stay cached).
    pub fn release(&self, page: PageRef) {
        let pon = page.page_object_number;
        self.0.pages.borrow_mut().shift_remove(&pon);
        self.0.core.tiles.release_page(&self.0.view_id, pon);
    }

    /// Drop one reference; the last releases every page of this view. The
    /// view's entry stays, so a handle reused after a dispose keeps working.
    pub fn dispose(&self) {
        let refs = &self.0.refs;
        if refs.get() == 0 {
            return;
        }
        refs.set(refs.get() - 1);
        if refs.get() > 0 {
            return;
        }
        let pons: Vec<u32> = self.0.pages.borrow().keys().copied().collect();
        for pon in pons {
            self.0.core.tiles.release_page(&self.0.view_id, pon);
        }
        self.0.pages.borrow_mut().clear();
    }

    pub fn view_id(&self) -> &str {
        &self.0.view_id
    }

    /// `a === b`.
    pub fn same(&self, other: &ViewDemand) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

// ── createRenderController ────────────────────────────────────────────────────

/// `createRenderController(ctx, config)`: the capability and its `connect`.
pub fn create_render_controller(
    cx: Controller<RenderState, RenderAction>,
    config: &RenderConfig,
    host: RenderHost,
) -> Result<(RenderCapability, Option<ConnectFn>), Error> {
    let resolved = Rc::new(resolve_render_options(config));
    let store = RasterStore::default();
    let invalidated = cx.event_source::<RenderInvalidatedEvent>();
    let render_completed = cx.event_source::<RenderCompletedEvent>();
    let render_failed = cx.event_source::<RenderFailedEvent>();
    let paint_settings =
        PaintSettings { fade_ms: resolved.tiles.fade_ms, tiles: resolved.tiles.enabled };

    let core = Rc::new_cyclic(|weak: &Weak<Core>| {
        let debug_sink = host.debug.clone();
        let tiles = TileManager::new(TileManagerDeps {
            store: store.clone(),
            options: resolved.clone(),
            get_policy: {
                let weak = weak.clone();
                Rc::new(move || weak.upgrade().map_or(CONTINUOUS_RENDER_POLICY, |c| c.policy()))
            },
            get_page_size: {
                let weak = weak.clone();
                Rc::new(move |pon| weak.upgrade()?.page_size(PageRef::new(pon)))
            },
            get_epoch: {
                let weak = weak.clone();
                Rc::new(move |pon, annotations| {
                    weak.upgrade().map_or(0.0, |c| c.epoch_of(pon, annotations))
                })
            },
            fetch_tile: {
                let weak = weak.clone();
                Rc::new(move |pon, rect: Rect, scale, include_annotations, signal| {
                    let Some(core) = weak.upgrade() else {
                        return Err(Error::new("render: the controller is gone"));
                    };
                    fetch_tile(&core, pon, rect, scale, include_annotations, signal)
                })
            },
            on_advance: {
                let weak = weak.clone();
                Rc::new(move |pon| {
                    if let Some(core) = weak.upgrade() {
                        core.wake(pon);
                    }
                })
            },
            debug: resolved.debug.then(|| {
                Rc::new(move |message: &str| debug_sink(&format!("[render] {message}")))
                    as Rc<dyn Fn(&str)>
            }),
            warn: host.warn.clone(),
        });
        Core {
            cx: cx.clone(),
            resolved: resolved.clone(),
            host: host.clone(),
            store: store.clone(),
            tiles,
            invalidated,
            render_completed,
            render_failed,
            policy_memo: RefCell::new(None),
            strategy_memo: RefCell::new(None),
            warned_format: Cell::new(false),
            warned_budget: Cell::new(false),
            pending_advance: RefCell::new(IndexSet::new()),
            advance_scheduled: Cell::new(false),
            views: RefCell::new(IndexMap::new()),
            paint_settings,
        }
    });

    let connecting = core.clone();
    let connect: ConnectFn = Rc::new(move || {
        // Confirmed pixel-changing facts, own or remote.
        let doc = connecting.cx.doc()?;
        let listener = connecting.clone();
        connecting
            .cx
            .listen(doc.subscribe(Rc::new(move |event: &Value| listener.on_document_event(event))));
        Ok(())
    });
    Ok((RenderCapability(core), Some(connect)))
}

/// The tile manager's `fetchTile`: the same refusal the engine would send,
/// without the round trip, then a page-space region converted to PDF space
/// by the kernel's page space (crop offsets included).
fn fetch_tile(
    core: &Core,
    pon: u32,
    rect: Rect,
    scale: f64,
    include_annotations: bool,
    signal: &AbortSignal,
) -> Result<Promise<Value>, Error> {
    if !core.can_render() {
        return Ok(Promise::rejected(plugin_error(
            code::PERMISSION_DENIED,
            "render",
            format!("render.tile requires {RENDER_SCOPE}"),
            None,
            Some(json!({ "required": RENDER_SCOPE })),
        )));
    }
    let page = PageRef::new(pon);
    let strategy = core.strategy();
    let doc = core.cx.doc()?;
    doc.page_index(pon)?;
    let target = core.cx.for_page(page)?.page_rect_to_pdf(kgeo::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    });
    let mut options = Map::new();
    options.insert("target".into(), json!({ "kind": "rect", "rect": target }));
    options.insert("viewport".into(), json!({ "kind": "scale", "scale": scale }));
    options.insert("includeAnnotations".into(), json!(include_annotations));
    if let Some(format) = strategy.format {
        options.insert("format".into(), json!(format.as_str()));
    }
    if let Some(quality) = strategy.quality {
        options.insert("quality".into(), json!(quality));
    }
    render_image(&doc, page, Value::Object(options), signal)
}
