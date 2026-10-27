//! `controller.ts`: the render controller, the one place strategy (the
//! settings, what the viewer spends) composes with policy (the engine fact,
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
//!
//! Every render it asks for is the picture of a page itself, so it asks the
//! engine with `priority: 'high'`, naming the view it's for when it knows it
//! (`ctx.doc.with({priority, view})`, here `DocumentHandle::call_with`).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::{Rc, Weak};

use epdf_geometry::Rect;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::{CallFacts, CallPriority, DocumentHandle};
use epdf_kernel::error::{code, plugin_error, to_plugin_error, to_plugin_error_info};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::settings::{InstanceSettings, SettingsChanged};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::store::Unsubscribe;
use epdf_kernel::types::DocumentMeta;
use epdf_kernel::{Controller, Error};
use indexmap::{IndexMap, IndexSet};
use serde_json::{Map, Value, json};

use crate::appearance_store::{AppearanceStore, RecentPictures};
use crate::contract::{
    InvalidateOptions, InvalidateScope, PageArg, PageImage, PageLayerOptions, PageRender,
    PageRenderFailure, RenderBatchResult, RenderConfig, RenderInvalidatedEvent, RenderPageOptions,
    RenderPagesOptions, RenderSettings, RenderThumbnailOptions, config_of, render_defaults,
};
use crate::host_contract::{
    AppearanceFamily, AppearancePicture, AppearancesOptions, PaintSettings, RenderCompletedEvent,
    RenderFailedEvent, RenderHost, RenderSourceOptions,
};
use crate::invalidation::pixel_change_of;
use crate::js::{self, number_to_string as num};
use crate::model::{
    PageLayerRights, PageRenderLayers, RenderState, invalidate_pages, layers_key_of,
    render_epoch_of, resolve_page_layers,
};
use crate::paint_plan::{
    PageViewDemand, RenderFormat, ResolvedRenderOptions, TilePaintPlan, empty_tile_plan,
    resolve_render_options,
};
use crate::policy::{CONTINUOUS_RENDER_POLICY, EngineRenderPolicy, PageRenderViewport};
use crate::raster_store::{DEFAULT_MAX_COST, PageImageHandle, RasterStore};
use crate::strategy::{ResolvedStrategy, base_ask_width, resolve_strategy};
use crate::tile_manager::{TileManager, TileManagerDeps};
use crate::tiles::PageSizePt;

const RENDER_SCOPE: &str = "doc.render";
const FORMS_SCOPE: &str = "doc.forms.read";
const ANNOTATIONS_SCOPE: &str = "doc.annotate.read";
const DEFAULT_BATCH_CONCURRENCY: f64 = 4.0;
const CAPABILITY: &str = "render";

/// `annotationKey(ref)`: `obj:<n>` or `base:<page>:<index>`, and
/// `undefined` (as a template literal prints it) for a ref of another kind.
pub(crate) fn annotation_key(reference: &Value) -> String {
    let text = |value: Option<&Value>| match value {
        None => "undefined".to_string(),
        Some(Value::Number(n)) => num(n.as_f64().unwrap_or(f64::NAN)),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    match reference.get("kind").and_then(Value::as_str) {
        Some("baseIndex") => format!(
            "base:{}:{}",
            text(reference.get("page").and_then(|p| p.get("objectNumber"))),
            text(reference.get("baseIndex"))
        ),
        Some("objectNumber") => format!("obj:{}", text(reference.get("objectNumber"))),
        _ => "undefined".to_string(),
    }
}

/// What a raster request sends the engine besides its target.
#[derive(Clone)]
struct RasterRequest {
    viewport: PageRenderViewport,
    layers: PageRenderLayers,
    format: Option<RenderFormat>,
    quality: Option<f64>,
    /// The view whose picture it is, so the engine ranks it by where the
    /// page is there.
    view: Option<String>,
}

/// A view's live demand for one page, as last set through its handle.
struct PageDemand {
    demand: PageViewDemand,
    layers: PageRenderLayers,
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

/// `RenderSettingsChanged`: the settings after a change, and the top-level
/// keys that changed.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RenderSettingsChanged {
    pub settings: RenderSettings,
    pub changed: Vec<String>,
}

struct Core {
    cx: Controller<RenderState, Value>,
    settings: InstanceSettings,
    resolved_memo: RefCell<Option<(Rc<Value>, Rc<ResolvedRenderOptions>)>>,
    host: RenderHost,
    /// Document-lifetime cache; staleness is a new key, never a flush.
    store: RasterStore,
    appearances: AppearanceStore<AppearancePicture>,
    recent_appearances: RecentPictures<AppearancePicture>,
    tiles: TileManager,
    invalidated: EventHookSource<RenderInvalidatedEvent>,
    render_completed: EventHookSource<RenderCompletedEvent>,
    render_failed: EventHookSource<RenderFailedEvent>,
    /// The kernel's policy JSON and its parse.
    policy_memo: RefCell<Option<(Value, EngineRenderPolicy)>>,
    strategy_memo:
        RefCell<Option<(EngineRenderPolicy, Rc<ResolvedRenderOptions>, Rc<ResolvedStrategy>)>>,
    paint_settings_memo: RefCell<Option<(bool, Rc<PaintSettings>)>>,
    warned_format: Cell<bool>,
    warned_budget: Cell<bool>,
    warned_settings: Cell<bool>,
    pending_advance: RefCell<IndexSet<u32>>,
    advance_scheduled: Cell<bool>,
    views: RefCell<IndexMap<String, ViewRecord>>,
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

    // ── settings ──

    /// A settings value as [`RenderSettings`]. One this port cannot read (a
    /// member of the wrong type, written past the typed API through
    /// `Kernel::settings_of`) reads as [`render_defaults`], and the first
    /// such read warns once. The TypeScript resolves whatever is there.
    fn decode_settings(&self, value: &Value) -> RenderSettings {
        config_of(value).unwrap_or_else(|error| {
            if !self.warned_settings.replace(true) {
                (self.host.warn)(&format!(
                    "[render] malformed settings ({error}); using the defaults."
                ));
            }
            render_defaults()
        })
    }

    /// The settings as the strategy reads them, resolved again when they
    /// change.
    fn resolved(&self) -> Rc<ResolvedRenderOptions> {
        let current = self.settings.get();
        if let Some((value, resolved)) = &*self.resolved_memo.borrow()
            && Rc::ptr_eq(value, &current)
        {
            return resolved.clone();
        }
        let config = self.decode_settings(&current);
        let resolved = Rc::new(resolve_render_options(&config));
        *self.resolved_memo.borrow_mut() = Some((current, resolved.clone()));
        resolved
    }

    fn get_settings(&self) -> RenderSettings {
        self.decode_settings(&self.settings.get())
    }

    // ── what a picture draws ──

    fn can_render(&self) -> bool {
        self.cx.allows(RENDER_SCOPE)
    }

    /// What the session may read of the parts a picture draws.
    fn rights(&self) -> PageLayerRights {
        PageLayerRights {
            annotations: self.cx.allows(ANNOTATIONS_SCOPE),
            form_fields: self.cx.allows(FORMS_SCOPE),
        }
    }

    /// The engine's rule: each option as given, and one left out draws what
    /// the session may read. Reads (keys, epochs, plans) never throw: they
    /// count an option the session may not read as left out.
    fn layers_of(&self, options: PageLayerOptions) -> PageRenderLayers {
        let may = self.rights();
        let keep = |asked: Option<bool>, allowed: bool| asked.filter(|asked| allowed || !*asked);
        resolve_page_layers(
            keep(options.include_annotations, may.annotations),
            keep(options.include_form_fields, may.form_fields),
            may,
        )
        .expect("an option the session may not read was left out")
    }

    /// The doors refuse an explicit `true` the session may not read, as the
    /// engine would.
    fn picture_layers(
        &self,
        options: PageLayerOptions,
        operation: &str,
    ) -> Result<PageRenderLayers, Error> {
        if options.include_annotations == Some(true) {
            self.cx.assert_allowed(ANNOTATIONS_SCOPE, operation)?;
        }
        if options.include_form_fields == Some(true) {
            self.cx.assert_allowed(FORMS_SCOPE, operation)?;
        }
        Ok(self.layers_of(options))
    }

    // ── one-shot developer hints ──

    fn warn_dropped_format(&self, strategy: &ResolvedStrategy, requested: Option<RenderFormat>) {
        let Some(requested) = requested else { return };
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
        if self.warned_budget.get()
            || self.resolved().tiles.enabled
            || demand_width <= supplied * 2.0
        {
            return;
        }
        self.warned_budget.set(true);
        (self.host.warn)(&format!(
            "[render] demand is {}× over the base budget and the tile plane is disabled (tiles: false) — the page rests blurry. Raise fullPage.maxWidth or re-enable tiles.",
            js::to_fixed(demand_width / supplied, 1)
        ));
    }

    // ── strategy ∧ policy ──

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

    /// Composed with the strategy once per policy and settings.
    fn strategy(&self) -> Rc<ResolvedStrategy> {
        let policy = self.policy();
        let options = self.resolved();
        let memoized = match &*self.strategy_memo.borrow() {
            Some((p, o, strategy)) if *p == policy && Rc::ptr_eq(o, &options) => {
                Some(strategy.clone())
            }
            _ => None,
        };
        if let Some(strategy) = memoized {
            return strategy;
        }
        let strategy = Rc::new(resolve_strategy(&policy, &options));
        *self.strategy_memo.borrow_mut() = Some((policy, options.clone(), strategy.clone()));
        self.warn_dropped_format(&strategy, options.format);
        strategy
    }

    // ── pages ──

    /// `ctx.getPage(page)`: the page's ref and size, or `None` for a page
    /// this document does not have.
    fn find_page(&self, page: PageArg) -> Option<(PageRef, PageSizePt)> {
        self.with_meta(|meta| {
            let pages = meta?.pages.as_array()?;
            let info = match page {
                PageArg::Ref(page) => pages.iter().find(|p| page.names(p))?,
                PageArg::Index(index) => {
                    if index < 0.0 || index.fract() != 0.0 {
                        return None;
                    }
                    pages.get(index as usize)?
                }
            };
            let reference = info.get("ref").and_then(PageRef::from_json)?;
            let size = info.get("size");
            let side = |key: &str| {
                size.and_then(|s| s.get(key)).and_then(Value::as_f64).unwrap_or(f64::NAN)
            };
            Some((reference, PageSizePt { width: side("width"), height: side("height") }))
        })
    }

    /// `ctx.pageOf(page)`: `not-found` for a page this document does not
    /// have.
    fn page_of(&self, page: PageArg) -> Result<(PageRef, PageSizePt), Error> {
        self.find_page(page).ok_or_else(|| {
            let message = match page {
                PageArg::Ref(page) => {
                    format!("page {} is not in this document", page.page_object_number)
                }
                PageArg::Index(index) => {
                    format!("there is no page {} in this document", num(index))
                }
            };
            plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
        })
    }

    fn page_size(&self, pon: u32) -> Option<PageSizePt> {
        self.find_page(PageArg::Ref(PageRef::new(pon))).map(|(_, size)| size)
    }

    /// The width a host-door page converts scale against: `not-found` for a
    /// page this document does not have.
    fn page_width_of(&self, page: PageRef) -> Result<f64, Error> {
        match self.page_size(page.page_object_number) {
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

    /// The conformed base width: the one base-sizing path (`render_source`,
    /// `get_source_key`, `conform_viewport` and, through the tile manager,
    /// engagement). `None` where the TypeScript's is `undefined` (an empty
    /// client ladder, M26).
    fn base_width(&self, page: PageRef, scale: f64) -> Result<Option<f64>, Error> {
        let demand_width = scale * self.page_width_of(page)?;
        let width = base_ask_width(&self.strategy(), demand_width);
        self.warn_past_budget_no_tiles(demand_width, width.unwrap_or(f64::NAN));
        Ok(width)
    }

    fn conform_viewport(&self, page: PageRef, scale: f64) -> Result<PageRenderViewport, Error> {
        // The TypeScript's width-less viewport (an empty ladder) has no
        // Rust shape; NaN stands in (M26).
        Ok(PageRenderViewport::Width { width: self.base_width(page, scale)?.unwrap_or(f64::NAN) })
    }

    fn epoch_of(&self, pon: u32, layers: PageRenderLayers) -> f64 {
        render_epoch_of(&self.cx.state(), pon, layers) as f64
    }

    /// A raster's identity includes its encode format (the resolved format
    /// can change under a live store) and an explicit quality.
    fn raster_key(
        &self,
        pon: u32,
        width: Option<f64>,
        layers: PageRenderLayers,
        format: Option<RenderFormat>,
        quality: Option<f64>,
    ) -> String {
        format!(
            "{pon}|w{}|{}|e{}{}{}",
            width.map_or_else(|| "undefined".to_string(), num),
            layers_key_of(layers),
            num(self.epoch_of(pon, layers)),
            format.map_or(String::new(), |f| format!("|f{}", f.as_str())),
            quality.map_or(String::new(), |q| format!("|q{}", num(q)))
        )
    }

    // ── the raster store door (shared by both render doors and the tiles) ──

    fn acquire_raster(
        self: &Rc<Self>,
        page: PageRef,
        key: String,
        request: RasterRequest,
        signal: Option<&AbortSignal>,
    ) -> Promise<PageImageHandle> {
        let result = self.store.acquire(
            &key,
            |store_signal| {
                let mut options = Map::new();
                options.insert(
                    "viewport".into(),
                    serde_json::to_value(request.viewport).unwrap_or_default(),
                );
                options
                    .insert("includeAnnotations".into(), json!(request.layers.include_annotations));
                options
                    .insert("includeFormFields".into(), json!(request.layers.include_form_fields));
                if let Some(format) = request.format {
                    options.insert("format".into(), json!(format.as_str()));
                }
                if let Some(quality) = request.quality {
                    options.insert("quality".into(), json!(quality));
                }
                let facts = high_for(request.view.clone());
                Ok(render_image(
                    &self.cx.doc()?,
                    &facts,
                    page,
                    Value::Object(options),
                    store_signal,
                ))
            },
            signal,
        );
        let result = match result {
            Ok(result) => result,
            Err(error) => return Promise::rejected(error),
        };
        let this = self.clone();
        Promise::spawn(async move {
            match result.await {
                Ok(image) => {
                    this.render_completed.emit(&RenderCompletedEvent { page, key });
                    Ok(image)
                }
                Err(error) => {
                    let mapped = to_plugin_error(CAPABILITY, &error);
                    if !mapped.is_plugin_error(Some(code::OPERATION_CANCELLED))
                        && !mapped.is_plugin_error(Some(code::INSTANCE_CLOSED))
                    {
                        this.render_failed.emit(&RenderFailedEvent {
                            page,
                            key,
                            error: to_plugin_error_info(&mapped),
                        });
                    }
                    Err(mapped)
                }
            }
        })
    }

    /// The host door: conformed to the render points.
    fn render_source(
        self: &Rc<Self>,
        page: PageRef,
        options: RenderSourceOptions,
    ) -> Promise<PageImage> {
        let this = self.clone();
        Promise::spawn(async move {
            this.cx.assert_allowed(RENDER_SCOPE, "render.renderSource")?;
            let layers = this.picture_layers(options.layers, "render.renderSource")?;
            let width = this.base_width(page, options.scale)?;
            let viewport = PageRenderViewport::Width { width: width.unwrap_or(f64::NAN) };
            let strategy = this.strategy();
            let key =
                this.raster_key(page.page_object_number, width, layers, strategy.format, None);
            let request = RasterRequest {
                viewport,
                layers,
                format: strategy.format,
                quality: strategy.quality,
                view: options.view.clone(),
            };
            let raster = this.acquire_raster(page, key, request, options.signal.as_ref());
            adopt(raster).await
        })
    }

    /// The public door: the requested size, exactly.
    fn render_page(
        self: &Rc<Self>,
        page: PageArg,
        options: RenderPageOptions,
    ) -> Promise<PageImage> {
        let this = self.clone();
        Promise::spawn(async move {
            this.cx.assert_allowed(RENDER_SCOPE, "render.renderPage")?;
            let (page, size) = this.page_of(page)?;
            let width = js::max(
                1.0,
                js::round(options.width.unwrap_or(options.scale.unwrap_or(1.0) * size.width)),
            );
            let layers = this.picture_layers(options.layers, "render.renderPage")?;
            let strategy = this.strategy();
            let format = options.format.or(strategy.format);
            let quality = options.quality.or(strategy.quality);
            // A width that matches a conformed raster (same format, default
            // quality) is the same key, so the cached image serves it.
            let key = this.raster_key(
                page.page_object_number,
                Some(width),
                layers,
                format,
                options.quality,
            );
            let request = RasterRequest {
                viewport: PageRenderViewport::Width { width },
                layers,
                format,
                quality,
                view: None,
            };
            let raster = this.acquire_raster(page, key, request, options.signal.as_ref());
            adopt(cancellable(CAPABILITY, options.signal.clone(), raster)).await
        })
    }

    fn render_pages(
        self: &Rc<Self>,
        pages: Vec<PageArg>,
        options: RenderPagesOptions,
    ) -> Promise<RenderBatchResult> {
        let this = self.clone();
        Promise::spawn(async move {
            this.cx.assert_allowed(RENDER_SCOPE, "render.renderPages")?;
            let concurrency =
                js::max(1.0, options.concurrency.unwrap_or(DEFAULT_BATCH_CONCURRENCY));
            // Each entry keeps its place in the input, so the result reads in
            // the order given whatever order the renders finished in.
            let applied: Rc<RefCell<Vec<(usize, PageRender)>>> = Rc::default();
            let failed: Rc<RefCell<Vec<(usize, PageRenderFailure)>>> = Rc::default();
            let queue: Rc<RefCell<VecDeque<(usize, PageArg)>>> =
                Rc::new(RefCell::new(pages.iter().copied().enumerate().collect()));
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
                        let Some((index, page)) = queue.borrow_mut().pop_front() else { break };
                        match this.render_page(page, page_options.clone()).await {
                            Ok(image) => {
                                let reference = match this.page_of(page) {
                                    Ok((reference, _)) => reference,
                                    Err(error) => {
                                        // The page left the document as it
                                        // rendered: `ctx.pageOf` threw.
                                        failed.borrow_mut().push((
                                            index,
                                            PageRenderFailure {
                                                page,
                                                error: to_plugin_error_info(&error),
                                            },
                                        ));
                                        continue;
                                    }
                                };
                                applied
                                    .borrow_mut()
                                    .push((index, PageRender { page: reference, image }));
                            }
                            Err(error) => failed.borrow_mut().push((
                                index,
                                PageRenderFailure {
                                    page,
                                    error: to_plugin_error_info(&to_plugin_error(
                                        CAPABILITY, &error,
                                    )),
                                },
                            )),
                        }
                    }
                    Ok(())
                }));
            }
            let batch = all(running);
            cancellable(CAPABILITY, options.page.signal.clone(), batch).reaction().await?;
            let mut applied = std::mem::take(&mut *applied.borrow_mut());
            let mut failed = std::mem::take(&mut *failed.borrow_mut());
            applied.sort_by_key(|(index, _)| *index);
            failed.sort_by_key(|(index, _)| *index);
            Ok(RenderBatchResult {
                applied: applied.into_iter().map(|(_, render)| render).collect(),
                skipped: Vec::new(),
                failed: failed.into_iter().map(|(_, failure)| failure).collect(),
            })
        })
    }

    // ── appearance pictures: the owners' items, shared between their views ──

    fn render_appearances(
        self: &Rc<Self>,
        page: PageArg,
        options: AppearancesOptions,
    ) -> Promise<Rc<Vec<AppearancePicture>>> {
        let this = self.clone();
        Promise::spawn(async move {
            let scope = match options.family {
                AppearanceFamily::Widgets => FORMS_SCOPE,
                AppearanceFamily::Annotations => ANNOTATIONS_SCOPE,
            };
            this.cx.assert_allowed(scope, "render.renderAppearances")?;
            let pon = this.page_of(page)?.0.page_object_number;
            let mut versions: IndexMap<String, f64> = IndexMap::new();
            for item in &options.items {
                versions.insert(annotation_key(&item.reference), item.version);
            }
            let mut asked: Vec<String> = versions
                .iter()
                .map(|(item, version)| format!("{item}@{}", num(*version)))
                .collect();
            asked.sort();
            let family = options.family;
            let group = format!("{}|{pon}", family.as_str());
            let key = format!("{group}|s{}|{}", num(options.scale), asked.join(","));
            let scale = options.scale;
            let fetcher = this.clone();
            let pictures = this.appearances.acquire(
                &group,
                &key,
                move |fetch_signal| {
                    fetcher.fetch_appearances(pon, family, scale, versions, fetch_signal)
                },
                options.signal.as_ref(),
            );
            let mapped = Promise::spawn(async move {
                pictures.await.map_err(|error| to_plugin_error(CAPABILITY, &error))
            });
            adopt(cancellable(CAPABILITY, options.signal.clone(), mapped)).await
        })
    }

    /// The look at rest only, in every state: a check box keeps both its
    /// pictures, and the looks under the pointer and pressed aren't drawn.
    fn fetch_appearances(
        self: &Rc<Self>,
        pon: u32,
        family: AppearanceFamily,
        scale: f64,
        versions: IndexMap<String, f64>,
        signal: &AbortSignal,
    ) -> Promise<Vec<AppearancePicture>> {
        let doc = match self.cx.doc() {
            Ok(doc) => doc,
            Err(error) => return Promise::rejected(error),
        };
        let path = match family {
            AppearanceFamily::Widgets => "page.forms.renderAppearances",
            AppearanceFamily::Annotations => "page.annotations.renderAppearances",
        };
        let options =
            json!({ "viewport": { "kind": "scale", "scale": scale }, "modes": ["normal"] });
        let task = doc.call(path, Some(pon), vec![options]);
        // The listener holds the task, and through it the pictures: it
        // comes off the entry's signal once the task settles.
        let listener = if signal.aborted() {
            task.abort(None);
            None
        } else {
            let aborted = task.clone();
            Some(signal.on_abort(Rc::new(move |_| aborted.abort(None))))
        };
        let this = self.clone();
        let signal = signal.clone();
        Promise::spawn(async move {
            let answer = task.await;
            if let Some(id) = listener {
                signal.remove(id);
            }
            let answer = answer?;
            let drawn: Vec<AppearancePicture> = answer
                .get("appearances")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|picture| {
                    let reference = picture.get("ref").unwrap_or(&Value::Null);
                    let version = versions.get(&annotation_key(reference))?;
                    Some(AppearancePicture { picture: picture.clone(), version: *version })
                })
                .collect();
            this.recent_appearances
                .remember(family.as_str(), &drawn, |picture| annotation_key(picture.reference()));
            Ok(drawn)
        })
    }

    // ── invalidation: the one place the ledger bumps and onInvalidated fires ──

    fn publish_invalidation(&self, pons: Vec<u32>, scope: InvalidateScope, origin: Value) {
        if pons.is_empty() {
            return;
        }
        self.cx.update_state(|state| match invalidate_pages(&state, &pons, scope) {
            Some(next) => Rc::new(next),
            None => state,
        });
        // Every view's plan for these pages points at old-epoch pixels:
        // re-plan now, so `get_plan` reads fresh on the next look.
        for &pon in &pons {
            self.replan_page(pon);
        }
        self.invalidated.emit(&RenderInvalidatedEvent {
            pages: pons.into_iter().map(PageRef::new).collect(),
            scope,
            origin,
        });
    }

    fn invalidate(&self, options: InvalidateOptions) {
        let pons = match options.pages {
            Some(pages) => pages
                .into_iter()
                .filter_map(|page| self.find_page(page).map(|(r, _)| r.page_object_number))
                .collect(),
            None => self.all_pons(),
        };
        self.publish_invalidation(
            pons,
            options.scope.unwrap_or(InvalidateScope::Content),
            Value::Null,
        );
    }

    // ── tiles ──

    /// Tile resolutions arrive in bursts: coalesce the re-plans per frame.
    fn wake(self: &Rc<Self>, pon: u32) {
        self.pending_advance.borrow_mut().insert(pon);
        if self.advance_scheduled.get() {
            return;
        }
        self.advance_scheduled.set(true);
        let this = Rc::downgrade(self);
        self.cx.clock().next_frame(move |_| {
            let Some(this) = this.upgrade() else { return };
            this.advance_scheduled.set(false);
            let pons: Vec<u32> =
                std::mem::take(&mut *this.pending_advance.borrow_mut()).into_iter().collect();
            for pon in pons {
                this.replan_page(pon);
            }
        });
    }

    /// Re-plan a page for every view that wants it (schedules the next want
    /// set), and wake readers once when any plan changed.
    fn replan_page(&self, pon: u32) {
        let views: Vec<(String, Pages)> =
            self.views.borrow().iter().map(|(id, v)| (id.clone(), v.pages.clone())).collect();
        let mut changed = false;
        for (view_id, pages) in views {
            let Some((demand, layers)) = pages.borrow().get(&pon).map(|e| (e.demand, e.layers))
            else {
                continue;
            };
            let plan = self.tiles.plan(&view_id, pon, &demand, layers);
            if let Some(entry) = pages.borrow_mut().get_mut(&pon)
                && !Rc::ptr_eq(&plan, &entry.plan)
            {
                entry.plan = plan;
                changed = true;
            }
        }
        if changed {
            self.cx.notify();
        }
    }

    /// What the layers paint with: the same `Rc` until a setting it reads
    /// changes.
    fn paint_settings(&self) -> Rc<PaintSettings> {
        let enabled = self.resolved().tiles.enabled;
        if let Some((memo, settings)) = &*self.paint_settings_memo.borrow()
            && *memo == enabled
        {
            return settings.clone();
        }
        let settings = Rc::new(PaintSettings { tiles: enabled });
        *self.paint_settings_memo.borrow_mut() = Some((enabled, settings.clone()));
        settings
    }

    /// A settings change reaches every view's plans at once.
    fn on_settings_changed(&self) {
        let mut shown: IndexSet<u32> = IndexSet::new();
        for view in self.views.borrow().values() {
            shown.extend(view.pages.borrow().keys().copied());
        }
        for pon in shown {
            self.replan_page(pon);
        }
    }

    fn on_document_event(&self, event: &Value) {
        let Some(change) = pixel_change_of(event, || self.all_pons()) else { return };
        // `'origin' in event ? event.origin : null`.
        let origin = event.get("origin").cloned().unwrap_or(Value::Null);
        self.publish_invalidation(change.pages, change.scope, origin);
    }
}

/// `return promise` from an `async` function: the outer promise adopts it
/// as a thenable, one turn for the adoption job and one for its reaction.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
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

/// `{priority: 'high', view}`, the view left out when the caller names none.
fn high_for(view: Option<String>) -> CallFacts {
    CallFacts { priority: Some(CallPriority::High), view }
}

/// `ctx.doc.with(facts).page(page).render.image(options)`, aborted with
/// `signal`. The TypeScript hands the engine the signal's reason only when
/// it is a string; a Rust abort reason is always an error, so the engine
/// gets none.
fn render_image(
    doc: &Rc<dyn DocumentHandle>,
    facts: &CallFacts,
    page: PageRef,
    options: Value,
    signal: &AbortSignal,
) -> Promise<Value> {
    let task =
        doc.call_with(facts, "page.render.image", Some(page.page_object_number), vec![options]);
    if signal.aborted() {
        task.abort(None);
    } else {
        let task = task.clone();
        signal.on_abort(Rc::new(move |_| task.abort(None)));
    }
    task
}

// ── the capability ────────────────────────────────────────────────────────────

/// `RenderHostCapability`, the public lens (`RenderCapability` in the
/// TypeScript) and the host lens in one type. Cheap to clone; clones are the
/// same capability.
#[derive(Clone)]
pub struct RenderCapability(Rc<Core>);

impl RenderCapability {
    // ── settings ──

    /// `getSettings()`: the settings in effect.
    pub fn get_settings(&self) -> RenderSettings {
        self.0.get_settings()
    }

    /// `updateSettings(changes)`: merge `changes` in, for every document.
    /// Fires `on_settings_changed` once when a setting changed.
    pub fn update_settings(&self, changes: &RenderConfig) {
        let changes = serde_json::to_value(changes).expect("plain data");
        self.0.settings.store.update_settings(&changes);
    }

    /// `resetSettings()`: back to what the app registered.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    /// `onSettingsChanged`.
    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&RenderSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let core = Rc::downgrade(&self.0);
        self.0.settings.events.on(move |event: &SettingsChanged| {
            let settings = match core.upgrade() {
                Some(core) => core.decode_settings(&event.settings),
                None => config_of(&event.settings).unwrap_or_else(|_| render_defaults()),
            };
            listener(&RenderSettingsChanged { settings, changed: event.changed.clone() })
        })
    }

    // ── public lens ──

    /// Would page rasters be served to this session (`doc.render`)?
    pub fn can_render(&self) -> bool {
        self.0.can_render()
    }

    /// Render one page, by its ref or its index, at the requested size,
    /// exactly. Rejects `permission-denied` without `doc.render`,
    /// `not-found` for a page not in this document, `operation-cancelled`
    /// on abort.
    pub fn render_page(
        &self,
        page: impl Into<PageArg>,
        options: RenderPageOptions,
    ) -> Promise<PageImage> {
        self.0.render_page(page.into(), options)
    }

    /// A small raster at `max_width` device pixels.
    pub fn render_thumbnail(
        &self,
        page: impl Into<PageArg>,
        options: RenderThumbnailOptions,
    ) -> Promise<PageImage> {
        self.0.render_page(
            page.into(),
            RenderPageOptions {
                width: Some(options.max_width),
                layers: options.layers,
                signal: options.signal,
                ..Default::default()
            },
        )
    }

    /// Render several pages with bounded concurrency; best-effort per page.
    pub fn render_pages(
        &self,
        pages: Vec<PageArg>,
        options: RenderPagesOptions,
    ) -> Promise<RenderBatchResult> {
        self.0.render_pages(pages, options)
    }

    /// The deployment's advertised render policy (a document fact).
    pub fn get_render_policy(&self) -> EngineRenderPolicy {
        self.0.policy()
    }

    /// The version of the picture these options would produce: content
    /// facts, plus annotation facts when it draws annotations and form facts
    /// when it draws form fields. 0 for a page that isn't in the document.
    pub fn get_render_epoch(&self, page: impl Into<PageArg>, layers: PageLayerOptions) -> f64 {
        match self.0.find_page(page.into()) {
            Some((page, _)) => self.0.epoch_of(page.page_object_number, self.0.layers_of(layers)),
            None => 0.0,
        }
    }

    /// Declare that page pixels changed. A page that isn't in the document
    /// is skipped.
    pub fn invalidate(&self, options: InvalidateOptions) {
        self.0.invalidate(options);
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
        layers: PageLayerOptions,
    ) -> Result<String, Error> {
        let width = self.0.base_width(page, scale)?;
        Ok(self.0.raster_key(
            page.page_object_number,
            width,
            self.0.layers_of(layers),
            self.0.strategy().format,
            None,
        ))
    }

    /// The canonical (width-kind) viewport a desired scale conforms to.
    pub fn conform_viewport(&self, page: PageRef, scale: f64) -> Result<PageRenderViewport, Error> {
        self.0.conform_viewport(page, scale)
    }

    /// The resolved paint settings: the same `Rc` until one changes.
    pub fn get_paint_settings(&self) -> Rc<PaintSettings> {
        self.0.paint_settings()
    }

    /// What the user may read of the parts a picture draws beside the page
    /// content.
    pub fn get_layer_rights(&self) -> PageLayerRights {
        self.0.rights()
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

    /// The engine's pictures of the owner's items on a page: each item's
    /// look at rest, in every state it stores, tagged with the item's
    /// version. Asks for the same page, family, scale and items share one
    /// fetch, and the last few sets of each page are kept. Rejects
    /// `permission-denied` without `doc.annotate.read` (annotations) or
    /// `doc.forms.read` (widgets).
    pub fn render_appearances(
        &self,
        page: impl Into<PageArg>,
        options: AppearancesOptions,
    ) -> Promise<Rc<Vec<AppearancePicture>>> {
        self.0.render_appearances(page.into(), options)
    }

    /// An item's last pictures, every state, kept a while after it left its
    /// page. Empty for one never drawn.
    pub fn kept_appearances(
        &self,
        family: AppearanceFamily,
        item: &Value,
    ) -> Rc<Vec<AppearancePicture>> {
        self.0.recent_appearances.of(family.as_str(), &annotation_key(item))
    }

    /// The appearance scale a render scale conforms to under the
    /// deployment's policy. A lattice with an empty appearance ladder is
    /// refused as the engine refuses it.
    pub fn get_appearance_scale(&self, render_scale: f64) -> Result<f64, Error> {
        crate::policy::snap_appearance_scale(&self.0.policy(), render_scale)
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

    #[cfg(feature = "uniffi")]
    pub(crate) fn lifetime(&self) -> &AbortSignal {
        self.0.cx.lifetime()
    }

    /// The page's width in points, as `render_page` sizes against it.
    /// `not-found` for a page not in this document.
    #[cfg(feature = "uniffi")]
    pub(crate) fn page_width(&self, page: PageRef) -> Result<f64, Error> {
        self.0.page_of(PageArg::Ref(page)).map(|(_, size)| size.width)
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
    /// What this view wants for a page right now. Idempotent; schedules the
    /// want set.
    pub fn set_demand(&self, page: PageRef, demand: PageViewDemand, options: PageLayerOptions) {
        let inner = &self.0;
        let pon = page.page_object_number;
        let layers = inner.core.layers_of(options);
        let previous = inner.pages.borrow().get(&pon).map(|e| e.plan.clone());
        let plan = inner.core.tiles.plan(&inner.view_id, pon, &demand, layers);
        let changed = previous.is_none_or(|p| !Rc::ptr_eq(&p, &plan));
        inner.pages.borrow_mut().insert(pon, PageDemand { demand, layers, plan });
        if changed {
            inner.core.cx.notify();
        }
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

/// `createRenderController(ctx)`: the capability and its `connect`.
pub fn create_render_controller(
    cx: Controller<RenderState, Value>,
    host: RenderHost,
) -> Result<(RenderCapability, Option<ConnectFn>), Error> {
    let settings = cx.settings()?;
    let store = match &host.release_image {
        Some(release) => RasterStore::with_release(DEFAULT_MAX_COST, release.clone()),
        None => RasterStore::default(),
    };
    let invalidated = cx.event_source::<RenderInvalidatedEvent>();
    let render_completed = cx.event_source::<RenderCompletedEvent>();
    let render_failed = cx.event_source::<RenderFailedEvent>();
    let clock = cx.clock();

    let core = Rc::new_cyclic(|weak: &Weak<Core>| {
        let debug_sink = host.debug.clone();
        let tiles = TileManager::new(TileManagerDeps {
            store: store.clone(),
            get_options: {
                let weak = weak.clone();
                Rc::new(move || {
                    weak.upgrade().map_or_else(
                        || Rc::new(resolve_render_options(&RenderConfig::default())),
                        |c| c.resolved(),
                    )
                })
            },
            get_policy: {
                let weak = weak.clone();
                Rc::new(move || weak.upgrade().map_or(CONTINUOUS_RENDER_POLICY, |c| c.policy()))
            },
            get_page_size: {
                let weak = weak.clone();
                Rc::new(move |pon| weak.upgrade()?.page_size(pon))
            },
            get_epoch: {
                let weak = weak.clone();
                Rc::new(move |pon, layers| weak.upgrade().map_or(0.0, |c| c.epoch_of(pon, layers)))
            },
            after: Rc::new(move |ms, run| clock.after(ms, run)),
            fetch_tile: {
                let weak = weak.clone();
                Rc::new(move |view: &str, pon, rect: Rect, scale, layers, signal: &AbortSignal| {
                    let Some(core) = weak.upgrade() else {
                        return Ok(Promise::rejected(Error::new("render: the controller is gone")));
                    };
                    Ok(fetch_tile(&core, view, pon, rect, scale, layers, signal))
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
            debug: {
                let weak = weak.clone();
                Some(Rc::new(move |message: &str| {
                    if weak.upgrade().is_some_and(|c| c.resolved().debug) {
                        debug_sink(&format!("[render] {message}"));
                    }
                }) as Rc<dyn Fn(&str)>)
            },
            warn: host.warn.clone(),
        });
        Core {
            cx: cx.clone(),
            settings: settings.clone(),
            resolved_memo: RefCell::new(None),
            host: host.clone(),
            store: store.clone(),
            appearances: AppearanceStore::default(),
            recent_appearances: RecentPictures::default(),
            tiles,
            invalidated,
            render_completed,
            render_failed,
            policy_memo: RefCell::new(None),
            strategy_memo: RefCell::new(None),
            paint_settings_memo: RefCell::new(None),
            warned_format: Cell::new(false),
            warned_budget: Cell::new(false),
            warned_settings: Cell::new(false),
            pending_advance: RefCell::new(IndexSet::new()),
            advance_scheduled: Cell::new(false),
            views: RefCell::new(IndexMap::new()),
        }
    });

    // A settings change reaches every view's plans at once.
    let listener = Rc::downgrade(&core);
    cx.listen(settings.events.on(move |_| {
        if let Some(core) = listener.upgrade() {
            core.on_settings_changed();
        }
        Ok(())
    }));

    let connecting = core.clone();
    let connect: ConnectFn = Rc::new(move || {
        // Confirmed pixel-changing facts, own or remote, and the stream's
        // notice that some were lost.
        let doc = connecting.cx.doc()?;
        let listener = Rc::downgrade(&connecting);
        connecting.cx.listen(doc.subscribe(Rc::new(move |event: &Value| {
            if let Some(core) = listener.upgrade() {
                core.on_document_event(event);
            }
        })));
        Ok(())
    });
    Ok((RenderCapability(core), Some(connect)))
}

/// The tile manager's `fetchTile`: the same refusal the engine would send,
/// without the round trip (a denied session's viewport would otherwise be
/// refused once per tile), then a page-space region at a scale.
fn fetch_tile(
    core: &Rc<Core>,
    view: &str,
    pon: u32,
    rect: Rect,
    scale: f64,
    layers: PageRenderLayers,
    signal: &AbortSignal,
) -> Promise<Value> {
    if let Err(error) = core.cx.assert_allowed(RENDER_SCOPE, "render.tile") {
        return Promise::rejected(error);
    }
    let strategy = core.strategy();
    let doc = match core.cx.doc() {
        Ok(doc) => doc,
        Err(error) => return Promise::rejected(error),
    };
    let mut options = Map::new();
    options.insert(
        "target".into(),
        json!({ "kind": "rect", "rect": {
            "x": rect.x, "y": rect.y, "width": rect.width, "height": rect.height,
        } }),
    );
    options.insert("viewport".into(), json!({ "kind": "scale", "scale": scale }));
    options.insert("includeAnnotations".into(), json!(layers.include_annotations));
    options.insert("includeFormFields".into(), json!(layers.include_form_fields));
    if let Some(format) = strategy.format {
        options.insert("format".into(), json!(format.as_str()));
    }
    if let Some(quality) = strategy.quality {
        options.insert("quality".into(), json!(quality));
    }
    let facts = high_for(Some(view.to_string()));
    render_image(&doc, &facts, PageRef::new(pon), Value::Object(options), signal)
}
