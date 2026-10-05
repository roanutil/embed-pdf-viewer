//! `controller.ts`: the search controller. `search` runs on a `latest` lane
//! (a newer search supersedes the older one, which can no longer publish);
//! `find_all` runs on its own signal and never touches the session. Both
//! share `collect`, the one cursor loop over the engine's budgeted slices.
//!
//! Turns are spelled out where they cost more than one: an engine call
//! through the guarded handle is an `AbortablePromise`, which `await` adopts
//! as a thenable ([`await_engine`]), and awaiting the ported `async`
//! `collect` is `rt::returned`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use epdf_geometry::{Point, TextQuad, bounds_of_rects, text_quad_bounds};
use epdf_kernel::engine::decode;
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::{PageRef, PageSpace};
use epdf_kernel::lanes::{LatestLane, LatestRun};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise, TimerId};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error, Unsubscribe};
use epdf_plugin_stage::{
    RevealAnchor, RevealAnchorValue, RevealOptions, STAGE, ScrollBehaviorKind,
};
use serde_json::{Value, json};

use crate::contract::*;
use crate::model::{SearchAction, SearchState, pages_with_hits};

/// The browser find-bar feel: the hit at the top-middle, no zoom change.
fn default_reveal() -> SearchRevealOptions {
    SearchRevealOptions {
        anchor: Some(Some(RevealAnchor { x: None, y: Some(RevealAnchorValue::Fraction(0.35)) })),
        behavior: Some(Some(ScrollBehaviorKind::Smooth)),
    }
}

/// Reruns after document mutations are coalesced: a burst of events
/// triggers one rescan.
const RERUN_DELAY_MS: f64 = 250.0;

/// `await pending` for an `AbortablePromise`: not a native promise, so
/// `await` adopts it as a thenable, one turn before the await's own.
async fn await_engine(call: Promise<Value>) -> Result<Value, Error> {
    Promise::spawn(async move {
        rt::tick().await;
        call.await
    })
    .await
}

/// How a `collect` consumer observes the loop: the session dispatches,
/// `find_all` accumulates.
trait CollectSink {
    /// Before the first slice and again on a stale-cursor restart: reset
    /// any accumulation.
    fn on_start(&self);
    fn on_slice(&self, hits: Vec<SearchHit>, scanned: f64, total: f64);
}

/// The value an aborted signal's reason reaches the engine as:
/// `pending.abort?.(signal.reason)`. A lane aborts with a string, a
/// caller's controller with an error.
type ReasonOf = fn(&Error) -> Value;

fn lane_reason(reason: &Error) -> Value {
    Value::String(reason.message.clone())
}

fn error_reason(reason: &Error) -> Value {
    reason.to_canonical()
}

struct Events {
    started: EventHookSource<SearchStartedEvent>,
    progress: EventHookSource<SearchProgressEvent>,
    completed: EventHookSource<SearchCompletedEvent>,
    cancelled: EventHookSource<SearchCancelledEvent>,
    failed: EventHookSource<SearchFailedEvent>,
    active_hit_changed: EventHookSource<SearchActiveHitChangedEvent>,
    cleared: EventHookSource<SearchClearedEvent>,
}

struct Core {
    cx: Controller<SearchState, SearchAction>,
    config: SearchConfig,
    session: LatestLane,
    events: Events,
}

/// `SearchCapability`. Cheap to clone; clones share the session.
#[derive(Clone)]
pub struct SearchCapability(Rc<Core>);

/// `createSearchController(ctx, config)`: the capability and its connect,
/// which subscribes the mutation rerun.
pub fn create_search_controller(
    cx: Controller<SearchState, SearchAction>,
    config: SearchConfig,
) -> (SearchCapability, Option<ConnectFn>) {
    let events = Events {
        started: cx.event_source(),
        progress: cx.event_source(),
        completed: cx.event_source(),
        cancelled: cx.event_source(),
        failed: cx.event_source(),
        active_hit_changed: cx.event_source(),
        cleared: cx.event_source(),
    };
    let session = cx.latest("session");
    let api = SearchCapability(Rc::new(Core { cx, config, session, events }));
    let connect_api = api.clone();
    let connect: ConnectFn = Rc::new(move || connect_api.connect());
    (api, Some(connect))
}

impl Core {
    fn state(&self) -> Rc<SearchState> {
        self.cx.state()
    }

    /// Engine slice to page-space hits. A match on a page that vanished
    /// mid-search (deleted) is dropped.
    fn hits_from_slice(&self, slice: &SearchSlice) -> Result<Vec<SearchHit>, Error> {
        let mut hits = Vec::new();
        // A slice's matches come a page at a time, and finding a page scans
        // the document's registry: look each page up once, not twice per
        // match (32,000 matches on a 400-page document was most of a
        // search's main thread).
        let mut last: Option<(PageRef, Value, PageSpace)> = None;
        for m in &slice.matches {
            if last.as_ref().is_none_or(|(at, _, _)| *at != m.page) {
                let Some(page) = self.cx.get_page(m.page) else { continue };
                last = Some((m.page, page, self.cx.for_page(m.page)?));
            }
            let Some((_, page, space)) = &last else { continue };
            // Engine quads carry frame-geometric slot semantics (p1..p4 =
            // upper-start, upper-end, lower-start, lower-end): the y-flip
            // maps the corners onto their names.
            let to_quad = |q: epdf_geometry::Quad| -> TextQuad {
                let p = space.pdf_quad_to_page(epdf_kernel::geometry::Quad {
                    p1: epdf_kernel::geometry::Point { x: q.p1.x, y: q.p1.y },
                    p2: epdf_kernel::geometry::Point { x: q.p2.x, y: q.p2.y },
                    p3: epdf_kernel::geometry::Point { x: q.p3.x, y: q.p3.y },
                    p4: epdf_kernel::geometry::Point { x: q.p4.x, y: q.p4.y },
                });
                let pt = |p: epdf_kernel::geometry::Point| Point { x: p.x, y: p.y };
                TextQuad {
                    upper_start: pt(p.p1),
                    upper_end: pt(p.p2),
                    lower_start: pt(p.p3),
                    lower_end: pt(p.p4),
                }
            };
            let segments: Vec<TextSegment> = m
                .segments
                .iter()
                .map(|segment| {
                    let quad = to_quad(segment.quad);
                    TextSegment { quad, rect: text_quad_bounds(quad), advance: segment.advance }
                })
                .collect();
            let bounds = if segments.is_empty() {
                None
            } else {
                bounds_of_rects(&segments.iter().map(|s| s.rect).collect::<Vec<_>>())
            };
            hits.push(SearchHit {
                page: m.page,
                page_index: page.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
                char_start: m.char_start,
                char_count: m.char_count,
                segments,
                bounds,
                snippet: m.snippet.clone(),
            });
        }
        Ok(hits)
    }
}

/// One cursor loop over budgeted slices: the permission fallback (`Full` to
/// `Rects` when snippets are denied, unless pinned), restart-once on a
/// stale cursor, projection per slice. `Ok(true)` on exhaustion, `Ok(false)`
/// when the signal aborted; `Err` on a real error (already a `PluginError`:
/// the handle is guarded).
async fn collect(
    core: Rc<Core>,
    query: SearchQuery,
    start_page: Option<PageRef>,
    pinned_mode: Option<SearchMode>,
    signal: AbortSignal,
    reason_of: ReasonOf,
    sink: &dyn CollectSink,
) -> Result<bool, Error> {
    sink.on_start();

    let mut mode = pinned_mode.unwrap_or(SearchMode::Full);
    let mode_pinned = pinned_mode.is_some();
    let mut cursor: Option<String> = None;
    let mut restarted = false;
    loop {
        if signal.aborted() {
            return Ok(false);
        }
        let request = SearchRequest {
            query: query.clone(),
            mode: Some(mode),
            start_page: if cursor.is_none() { start_page } else { None },
            cursor: cursor.clone(),
            skip: None,
            budget: None,
        };
        let request = serde_json::to_value(&request).unwrap_or(Value::Null);
        let pending = core.cx.doc()?.call("search.query", None, vec![request]);
        let on_abort = {
            let pending = pending.clone();
            signal.on_abort(Rc::new(move |reason: &Error| pending.abort(Some(reason_of(reason)))))
        };
        let result = await_engine(pending).await.and_then(decode::<SearchSlice>);
        signal.remove(on_abort);
        let slice = match result {
            Ok(slice) => slice,
            Err(error) => {
                if signal.aborted() {
                    return Ok(false);
                }
                // Snippets denied (no doc.text.copy)? Degrade to rect-only
                // matches.
                if !mode_pinned
                    && mode == SearchMode::Full
                    && cursor.is_none()
                    && error.is_plugin_error(Some(code::PERMISSION_DENIED))
                {
                    mode = SearchMode::Rects;
                    continue;
                }
                // A stale cursor rejects with InvalidArg: the document
                // changed under the loop. Position is lost, the query is
                // not: restart once from scratch.
                if cursor.is_some()
                    && !restarted
                    && error.is_plugin_error(Some(code::INVALID_INPUT))
                {
                    restarted = true;
                    cursor = None;
                    sink.on_start();
                    continue;
                }
                return Err(error);
            }
        };
        if signal.aborted() {
            return Ok(false);
        }
        let hits = core.hits_from_slice(&slice)?;
        sink.on_slice(hits, slice.scanned_pages, slice.total_pages);
        match slice.next_cursor {
            None => return Ok(true),
            Some(next) => cursor = Some(next),
        }
    }
}

/// `{...DEFAULT_REVEAL, ...config.reveal, ...options}`, one layer.
fn merge_reveal(
    base: SearchRevealOptions,
    over: Option<SearchRevealOptions>,
) -> SearchRevealOptions {
    match over {
        None => base,
        Some(over) => SearchRevealOptions {
            anchor: over.anchor.or(base.anchor),
            behavior: over.behavior.or(base.behavior),
        },
    }
}

struct SessionSink {
    core: Rc<Core>,
    run: LatestRun,
    query: SearchQuery,
}

impl CollectSink for SessionSink {
    fn on_start(&self) {
        let (core, run, query) = (&self.core, &self.run, &self.query);
        run.commit(|| {
            core.cx.dispatch(SearchAction::Start {
                query: query.clone(),
                operation_id: run.id.clone(),
            });
            core.events
                .started
                .emit(&SearchStartedEvent { query: query.clone(), operation_id: run.id.clone() });
        });
    }

    fn on_slice(&self, hits: Vec<SearchHit>, scanned: f64, total: f64) {
        let (core, run) = (&self.core, &self.run);
        run.commit(|| {
            let hits = hits.into_iter().map(Rc::new).collect();
            core.cx.dispatch(SearchAction::Append { hits, scanned, total });
            core.events.progress.emit(&SearchProgressEvent {
                scanned,
                total,
                hit_count: core.state().hits.len(),
                operation_id: run.id.clone(),
            });
        });
    }
}

struct FindAllSink {
    all: RefCell<Vec<SearchHit>>,
}

impl CollectSink for FindAllSink {
    fn on_start(&self) {
        self.all.borrow_mut().clear();
    }

    fn on_slice(&self, hits: Vec<SearchHit>, _scanned: f64, _total: f64) {
        self.all.borrow_mut().extend(hits);
    }
}

impl SearchCapability {
    fn core(&self) -> &Rc<Core> {
        &self.0
    }

    fn state(&self) -> Rc<SearchState> {
        self.0.state()
    }

    /// The Stage's current page, when a Stage is installed. `Err` is the
    /// Stage's constructor failing inside `ctx.tryGet`.
    fn viewport_first_page(&self) -> Result<Option<PageRef>, Error> {
        let Some(stage) = self.0.cx.try_get(&STAGE)? else { return Ok(None) };
        Ok(stage.get_current_page().and_then(|page| page.get("ref").and_then(PageRef::from_json)))
    }

    /// `connect()`: every confirmed mutation (own or remote) invalidates
    /// cursors and can change what is findable, so a running or finished
    /// session re-runs its query, 250 ms after the burst goes quiet.
    fn connect(&self) -> Result<(), Error> {
        let timer: Rc<Cell<Option<TimerId>>> = Rc::default();
        let doc = self.0.cx.doc()?;
        let (api, armed) = (self.clone(), timer.clone());
        self.0.cx.listen(doc.subscribe(Rc::new(move |_event: &Value| {
            if api.state().status == SearchStatus::Idle {
                return;
            }
            if let Some(id) = armed.take() {
                rt::clear_timer(id);
            }
            let (api, slot) = (api.clone(), armed.clone());
            armed.set(Some(rt::set_timeout(RERUN_DELAY_MS, move || {
                slot.set(None);
                // Surfaced through get_status() and on_failed.
                api.refresh(OperationOptions::default()).ignore();
            })));
        })));
        self.0.cx.cleanup_fn(move || {
            if let Some(id) = timer.take() {
                rt::clear_timer(id);
            }
        });
        Ok(())
    }

    /// Would a search be served now. `None` or `Rects` asks
    /// `doc.text.search`; `Full` also needs `doc.text.copy`, because a
    /// snippet reproduces document text.
    pub fn can_search(&self, mode: Option<SearchMode>) -> bool {
        // A document-scoped context always has its handle.
        let Ok(doc) = self.0.cx.doc() else { return false };
        let security = doc.security();
        security.allows("doc.text.search")
            && (mode != Some(SearchMode::Full) || security.allows("doc.text.copy"))
    }

    // ── the session ───────────────────────────────────────────────────────────

    /// Start a new search, superseding and aborting a running one. Hits
    /// stream into the session; the first becomes active, but the camera
    /// does not move until you navigate. Resolves when the scan completes,
    /// is superseded or is cancelled; rejects only on a real failure. An
    /// empty `query.text` is `clear()`.
    pub fn search(&self, query: SearchQuery, options: SearchOptions) -> Promise<SearchResult> {
        if query.text.is_empty() {
            self.clear();
            return Promise::resolved(SearchResult {
                status: SearchResultStatus::Complete,
                hit_count: 0,
            });
        }
        let operation_id = Rc::new(RefCell::new(String::new()));
        let api = self.clone();
        let op = operation_id.clone();
        let start_page = options.start_page;
        let running = self.0.session.run(
            move |run: LatestRun| {
                Promise::spawn(async move {
                    *op.borrow_mut() = run.id.clone();
                    let start_page = match start_page {
                        Some(page) => Some(page),
                        None => api.viewport_first_page()?,
                    };
                    let core = api.core().clone();
                    let sink =
                        SessionSink { core: core.clone(), run: run.clone(), query: query.clone() };
                    let complete = rt::returned(collect(
                        core.clone(),
                        query,
                        start_page,
                        None,
                        run.signal.clone(),
                        lane_reason,
                        &sink,
                    ))
                    .await?;
                    if !complete {
                        return Err(plugin_error(
                            code::OPERATION_CANCELLED,
                            "search",
                            "search aborted",
                            None,
                            None,
                        ));
                    }
                    run.commit(|| {
                        core.cx.dispatch(SearchAction::Complete);
                        core.events.completed.emit(&SearchCompletedEvent {
                            hit_count: core.state().hits.len(),
                            operation_id: run.id.clone(),
                        });
                    });
                    Ok(SearchResult {
                        status: SearchResultStatus::Complete,
                        hit_count: core.state().hits.len(),
                    })
                })
            },
            options.signal,
        );
        let core = self.core().clone();
        Promise::spawn(async move {
            let error = match running.await {
                Ok(result) => return Ok(result),
                Err(error) => error,
            };
            let operation_id = operation_id.borrow().clone();
            if error.is_plugin_error(Some(code::INSTANCE_CLOSED)) {
                return Ok(SearchResult { status: SearchResultStatus::Cancelled, hit_count: 0 });
            }
            if error.is_plugin_error(Some(code::OPERATION_CANCELLED)) {
                let why = error
                    .member("details")
                    .and_then(|d| d.get("reason"))
                    .and_then(Value::as_str)
                    .unwrap_or("cancelled");
                let reason = match why {
                    "superseded" => SearchCancelReason::Superseded,
                    "cleared" => SearchCancelReason::Cleared,
                    _ => SearchCancelReason::Cancelled,
                };
                // A superseded or cleared run has already lost the session;
                // a cancelled one keeps its hits.
                if reason == SearchCancelReason::Cancelled
                    && core.state().operation_id.as_deref() == Some(operation_id.as_str())
                {
                    core.cx.dispatch(SearchAction::Cancelled);
                }
                core.events
                    .cancelled
                    .emit(&SearchCancelledEvent { operation_id: operation_id.clone(), reason });
                return Ok(SearchResult {
                    status: if reason == SearchCancelReason::Superseded {
                        SearchResultStatus::Superseded
                    } else {
                        SearchResultStatus::Cancelled
                    },
                    hit_count: core.state().hits.len(),
                });
            }
            let info = PluginErrorInfo::of(&to_plugin_error("search", &error));
            if core.state().operation_id.as_deref() == Some(operation_id.as_str()) {
                core.cx.dispatch(SearchAction::Error { error: info.clone() });
            }
            core.events.failed.emit(&SearchFailedEvent { error: info, operation_id });
            Err(error)
        })
    }

    /// Re-run the current query from scratch. The plugin does this itself
    /// after document mutations.
    pub fn refresh(&self, options: OperationOptions) -> Promise<SearchResult> {
        let state = self.state();
        match &state.query {
            Some(query) if state.status != SearchStatus::Idle => self
                .search(query.clone(), SearchOptions { start_page: None, signal: options.signal }),
            _ => Promise::resolved(SearchResult {
                status: SearchResultStatus::Complete,
                hit_count: 0,
            }),
        }
    }

    /// Stop the scan and keep the hits found so far.
    pub fn cancel(&self) {
        self.0.session.cancel(Some(&json!("cancelled")));
    }

    /// Stop the scan and drop the session.
    pub fn clear(&self) {
        self.0.session.cancel(Some(&json!("cleared")));
        if self.state().status == SearchStatus::Idle {
            return;
        }
        self.0.cx.dispatch(SearchAction::Clear);
        self.0.events.cleared.emit(&SearchClearedEvent {});
    }

    /// Step to the next hit, wrapping, and reveal it.
    pub fn next_hit(
        &self,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, Error> {
        self.go_to_hit(self.state().active_index + 1, options)
    }

    /// Step to the previous hit, wrapping, and reveal it.
    pub fn previous_hit(
        &self,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, Error> {
        self.go_to_hit(self.state().active_index - 1, options)
    }

    /// Jump to a hit by index (wrapped modulo the hit count) and reveal it.
    /// `Err` only when the Stage's constructor throws.
    pub fn go_to_hit(
        &self,
        index: isize,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, Error> {
        let hits = self.state().hits.clone();
        if hits.is_empty() {
            return Ok(None);
        }
        let len = hits.len() as isize;
        let wrapped = ((index % len) + len) % len;
        let hit = (*hits[wrapped as usize]).clone();
        if self.state().active_index != wrapped {
            self.0.cx.dispatch(SearchAction::SetActive { index: wrapped });
            self.0.events.active_hit_changed.emit(&SearchActiveHitChangedEvent {
                index: wrapped as usize,
                hit: Some(hit.clone()),
            });
        }
        self.reveal(&hit, options)?;
        Ok(Some(hit))
    }

    /// Bring the active hit back into view without changing it.
    pub fn reveal_active_hit(&self, options: Option<SearchRevealOptions>) -> Result<(), Error> {
        match self.get_active_hit() {
            Some(hit) => self.reveal(&hit, options),
            None => Ok(()),
        }
    }

    /// A positioned reveal: the hit (not just its page) arrives at the
    /// anchor. Per-call over plugin config over the find-bar default; zoom
    /// never changes.
    fn reveal(&self, hit: &SearchHit, options: Option<SearchRevealOptions>) -> Result<(), Error> {
        let arrival = merge_reveal(merge_reveal(default_reveal(), self.0.config.reveal), options);
        if let Some(stage) = self.0.cx.try_get(&STAGE)? {
            stage.reveal(
                hit.page,
                Some(RevealOptions {
                    rect: hit.bounds,
                    anchor: arrival.anchor.flatten(),
                    behavior: arrival.behavior.flatten(),
                    zoom: None,
                }),
            );
        }
        Ok(())
    }

    pub fn get_query(&self) -> Option<SearchQuery> {
        self.state().query.clone()
    }

    pub fn get_status(&self) -> SearchStatus {
        self.state().status
    }

    /// Hits found so far, or those on one page. The same `Rc` per page until
    /// new hits land there.
    pub fn list_hits(&self, page: Option<PageRef>) -> Rc<Vec<Rc<SearchHit>>> {
        let state = self.state();
        match page {
            Some(page) => state
                .hits_by_page
                .get(&page.page_object_number)
                .cloned()
                .unwrap_or_else(|| Rc::new(Vec::new())),
            None => state.hits.clone(),
        }
    }

    pub fn get_hit_count(&self, page: Option<PageRef>) -> usize {
        let state = self.state();
        match page {
            Some(page) => state.hits_by_page.get(&page.page_object_number).map_or(0, |h| h.len()),
            None => state.hits.len(),
        }
    }

    pub fn list_pages_with_hits(&self) -> Vec<PageRef> {
        let state = self.state();
        if state.hits.is_empty() { Vec::new() } else { pages_with_hits(&state) }
    }

    /// `-1` when no hit is active.
    pub fn get_active_hit_index(&self) -> isize {
        self.state().active_index
    }

    pub fn get_active_hit(&self) -> Option<SearchHit> {
        let state = self.state();
        usize::try_from(state.active_index).ok().and_then(|i| state.hits.get(i)).map(|hit| (**hit).clone())
    }

    pub fn get_progress(&self) -> SearchProgress {
        self.state().progress
    }

    pub fn get_error(&self) -> Option<PluginErrorInfo> {
        self.state().error.clone()
    }

    // ── the service ───────────────────────────────────────────────────────────

    /// Run a query to completion and return every hit, touching no session
    /// state. Scans in natural page order; concurrent calls are
    /// independent. Rejects `operation-cancelled` when `options.signal`
    /// aborts.
    pub fn find_all(
        &self,
        query: SearchQuery,
        options: SearchFindAllOptions,
    ) -> Promise<Vec<SearchHit>> {
        let core = self.core().clone();
        Promise::spawn(async move {
            let controller = AbortSignal::new();
            let caller = options.signal.clone();
            let on_caller = |controller: &AbortSignal, caller: &AbortSignal| {
                if let Some(reason) = caller.reason() {
                    controller.abort(reason);
                }
            };
            if let Some(caller) = caller.as_ref().filter(|c| c.aborted()) {
                on_caller(&controller, caller);
            }
            let listener = caller.as_ref().map(|c| {
                let (controller, c2) = (controller.clone(), c.clone());
                c.on_abort(Rc::new(move |_| on_caller(&controller, &c2)))
            });
            let sink = FindAllSink { all: RefCell::new(Vec::new()) };
            let result = rt::returned(collect(
                core,
                query,
                None,
                options.mode,
                controller,
                error_reason,
                &sink,
            ))
            .await;
            if let (Some(caller), Some(id)) = (&caller, listener) {
                caller.remove(id);
            }
            if !result? {
                return Err(plugin_error(
                    code::OPERATION_CANCELLED,
                    "search",
                    "findAll aborted",
                    None,
                    None,
                ));
            }
            Ok(sink.all.take())
        })
    }

    // ── the hooks ─────────────────────────────────────────────────────────────

    pub fn on_started(
        &self,
        listener: impl Fn(&SearchStartedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.started.on(listener)
    }

    pub fn on_progress(
        &self,
        listener: impl Fn(&SearchProgressEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.progress.on(listener)
    }

    pub fn on_completed(
        &self,
        listener: impl Fn(&SearchCompletedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.completed.on(listener)
    }

    pub fn on_cancelled(
        &self,
        listener: impl Fn(&SearchCancelledEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.cancelled.on(listener)
    }

    pub fn on_failed(
        &self,
        listener: impl Fn(&SearchFailedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.failed.on(listener)
    }

    pub fn on_active_hit_changed(
        &self,
        listener: impl Fn(&SearchActiveHitChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.active_hit_changed.on(listener)
    }

    pub fn on_cleared(
        &self,
        listener: impl Fn(&SearchClearedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.cleared.on(listener)
    }

    /// The instance lifetime, aborted when the document closes. Not in the
    /// TypeScript, where a closed capability is unreachable; a foreign handle
    /// outlives the close, checks this before every call, and ends its event
    /// streams when it aborts. Public for the experimental manual bridge.
    #[cfg(feature = "uniffi")]
    #[doc(hidden)]
    pub fn lifetime(&self) -> &AbortSignal {
        self.0.cx.lifetime()
    }
}
