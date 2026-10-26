//! `controller.ts`: the search controller. `search` runs on a `latest` lane
//! (a newer search supersedes the older one, which can no longer publish);
//! `find_all` runs on its caller's signal and never touches the session.
//! Both share `collect`, the one cursor loop over the engine's budgeted
//! slices.
//!
//! The scan's own events (started, completed, cancelled, failed) fire where
//! the scan reaches those points. The progress, active-hit and cleared
//! events are derived from state changes, so they always agree with the
//! getters. The settings are read where they're used, never copied, so
//! `update_settings` applies to the next move at once.
//!
//! Each TypeScript `await` is a plain `.await` (the kernel's promises take
//! JavaScript's turns); awaiting the ported `async` `collect` is
//! [`rt::returned`], and `return promise` from an `async` function is
//! [`adopt`].

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use epdf_geometry::bounds_of_rects;
use epdf_kernel::engine::{CallFacts, CallPriority, decode};
use epdf_kernel::error::{code, plugin_error, to_plugin_error};
use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::number_to_string;
use epdf_kernel::lanes::{LatestLane, LatestRun};
use epdf_kernel::plugin::ConnectFn;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::settings::InstanceSettings;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Controller, Error, Unsubscribe, is_undefined};
use epdf_plugin_stage::{RevealAnchor, RevealOptions, STAGE, ScrollBehaviorKind};
use serde_json::{Value, json};

use crate::contract::*;
use crate::model::{
    HitList, SearchState, append_hits, cancel_session, clear_session, complete_session,
    fail_session, index_of_hit, pages_with_hits, set_active_hit, start_session,
};

const CAPABILITY: &str = "search";

/// Reruns after document mutations are coalesced: a burst of events
/// triggers one rescan.
const RERUN_DELAY_MS: f64 = 250.0;

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `then` reaction that settles the function's own promise.
async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

/// `toPluginErrorInfo(error)`.
fn error_info(error: &Error) -> PluginErrorInfo {
    let text =
        |key: &str| error.member(key).and_then(Value::as_str).unwrap_or_default().to_string();
    PluginErrorInfo {
        code: text("code"),
        message: error.message.clone(),
        capability: text("capability"),
        permission: error.member("permission").and_then(Value::as_str).map(str::to_string),
    }
}

/// The value an aborted signal's reason reaches the engine as:
/// `abortable.abort?.(signal.reason)`. A lane aborts its run with a string
/// (`'superseded'`, `'cancelled'`, `'cleared'`, `'instance-closed'`), which
/// the kernel's `AbortSignal` carries as an error whose message is that
/// string; a caller's controller aborts with an error.
type ReasonOf = fn(&Error) -> Value;

fn lane_reason(reason: &Error) -> Value {
    Value::String(reason.message.clone())
}

fn error_reason(reason: &Error) -> Value {
    reason.to_canonical()
}

/// `ctx.cancellable(signal, task)` (`cancellable.ts`): race an engine call
/// against the signal. When it fires, the call is aborted with the signal's
/// reason and the returned promise rejects `operation-cancelled` at once.
/// The kernel's `cancellable` hands the engine every reason in its error
/// form; a lane's reasons are strings in the TypeScript, so this one takes
/// the reason's form from the caller.
fn cancellable(
    signal: Option<&AbortSignal>,
    task: Promise<Value>,
    reason_of: ReasonOf,
) -> Promise<Value> {
    let Some(signal) = signal else {
        return task;
    };
    let (result, resolver) = Promise::pending();
    let abort = task.abort_handle();
    let (s, r) = (signal.clone(), resolver.clone());
    let cancel: Rc<dyn Fn(&Error)> = Rc::new(move |_| {
        let reason = s.reason();
        if let Some(abort) = &abort {
            abort(reason.as_ref().map(reason_of));
        }
        r.reject(plugin_error(
            code::OPERATION_CANCELLED,
            CAPABILITY,
            "operation cancelled",
            reason.as_ref().map(reason_of),
            None,
        ));
    });
    // Listened to even after a cancel: settling a settled promise changes
    // nothing, and the call's late rejection is never left unhandled.
    let registration = if signal.aborted() {
        cancel(&signal.reason().unwrap_or_else(|| Error::new("cancelled")));
        None
    } else {
        Some(signal.on_abort(cancel))
    };
    let signal = signal.clone();
    rt::spawn(async move {
        let answer = task.reaction().await;
        if let Some(id) = registration {
            signal.remove(id);
        }
        resolver.settle(answer.map_err(|e| to_plugin_error(CAPABILITY, &e)));
    });
    result
}

struct Events {
    started: EventHookSource<SearchStartedEvent>,
    progress_changed: EventHookSource<SearchProgressChangedEvent>,
    completed: EventHookSource<SearchCompletedEvent>,
    cancelled: EventHookSource<SearchCancelledEvent>,
    failed: EventHookSource<SearchFailedEvent>,
    active_hit_changed: EventHookSource<SearchActiveHitChangedEvent>,
    cleared: EventHookSource<SearchClearedEvent>,
}

/// The page registry, decoded once per page list: object number to the
/// row's position. A slice's matches look their pages up through it, so a
/// slice of many matches on a large document doesn't rescan the registry
/// per match, or once per slice across a chain (the pre-13.20 port cached
/// only the last page, which rescanned on every page change).
struct Registry {
    pages: Rc<Value>,
    by_pon: HashMap<u32, usize>,
}

struct Core {
    cx: Controller<SearchState, Value>,
    settings: InstanceSettings,
    session: LatestLane,
    events: Events,
    registry: RefCell<Option<Rc<Registry>>>,
}

/// `SearchCapability`. Cheap to clone; clones share the session.
#[derive(Clone)]
pub struct SearchCapability(Rc<Core>);

/// `createSearchController(ctx)`: the capability and its connect, which
/// subscribes the mutation rerun.
pub fn create_search_controller(
    cx: Controller<SearchState, Value>,
) -> Result<(SearchCapability, ConnectFn), Error> {
    let settings = cx.settings()?;
    let events = Events {
        started: cx.event_source(),
        progress_changed: cx.event_source(),
        completed: cx.event_source(),
        cancelled: cx.event_source(),
        failed: cx.event_source(),
        active_hit_changed: cx.event_source(),
        cleared: cx.event_source(),
    };
    let session = cx.latest("session");
    let api = SearchCapability(Rc::new(Core {
        cx,
        settings,
        session,
        events,
        registry: RefCell::new(None),
    }));

    let weak = Rc::downgrade(&api.0);
    api.0.cx.on_state_change(move |next, previous| {
        let Some(core) = weak.upgrade() else { return Ok(()) };
        // A slice moves it forward; a new search or clear() puts it back to none.
        if !Rc::ptr_eq(&previous.progress, &next.progress) {
            core.events.progress_changed.emit(&SearchProgressChangedEvent {
                pages_searched: next.progress.pages_searched,
                page_count: next.progress.page_count,
                hit_count: next.hits.len(),
            });
        }
        if previous.active_hit_index != next.active_hit_index {
            let hit =
                usize::try_from(next.active_hit_index).ok().and_then(|i| next.hits.get(i)).cloned();
            core.events
                .active_hit_changed
                .emit(&SearchActiveHitChangedEvent { index: next.active_hit_index, hit });
        }
        if previous.status != SearchStatus::Idle && next.status == SearchStatus::Idle {
            core.events.cleared.emit(&SearchClearedEvent {});
        }
        Ok(())
    });

    let connect_api = api.clone();
    let connect: ConnectFn = Rc::new(move || connect_api.connect());
    Ok((api, connect))
}

impl Core {
    fn state(&self) -> Rc<SearchState> {
        self.cx.state()
    }

    fn update(&self, transition: impl FnOnce(&Rc<SearchState>) -> Rc<SearchState>) {
        self.cx.update_state(|state| transition(&state));
    }

    /// The document's page list (`ctx.document()?.pages ?? []`).
    fn pages(&self) -> Option<Rc<Value>> {
        self.cx.plugin_context().document().map(|meta| meta.pages)
    }

    fn registry(&self) -> Option<Rc<Registry>> {
        let pages = self.pages()?;
        if let Some(cached) = self.registry.borrow().as_ref()
            && Rc::ptr_eq(&cached.pages, &pages)
        {
            return Some(cached.clone());
        }
        let by_pon = pages
            .as_array()
            .map(|rows| {
                let mut by_pon = HashMap::with_capacity(rows.len());
                for (i, row) in rows.iter().enumerate() {
                    if let Some(page) = row.get("ref").and_then(PageRef::from_json) {
                        // `Array.prototype.find`: the first row wins.
                        by_pon.entry(page.page_object_number).or_insert(i);
                    }
                }
                by_pon
            })
            .unwrap_or_default();
        let registry = Rc::new(Registry { pages, by_pon });
        *self.registry.borrow_mut() = Some(registry.clone());
        Some(registry)
    }

    /// `ctx.getPage(page)`: the page's registry row, or `None` for a page
    /// that isn't in the document. An index reads `pages[index]`.
    fn get_page(&self, target: PageTarget) -> Option<Value> {
        let registry = self.registry()?;
        let rows = registry.pages.as_array()?;
        match target {
            PageTarget::Ref(page) => {
                registry.by_pon.get(&page.page_object_number).and_then(|&i| rows.get(i)).cloned()
            }
            PageTarget::Index(index) => {
                if index.fract() != 0.0 || index < 0.0 {
                    return None;
                }
                rows.get(index as usize).cloned()
            }
        }
    }

    /// `ctx.pageOf(page).ref`: `not-found` for a page that isn't in the
    /// document.
    fn page_of(&self, target: PageTarget) -> Result<PageRef, Error> {
        self.get_page(target)
            .and_then(|row| row.get("ref").and_then(PageRef::from_json))
            .ok_or_else(|| {
                let message = match target {
                    PageTarget::Ref(page) => {
                        format!("page {} is not in this document", page.page_object_number)
                    }
                    PageTarget::Index(index) => {
                        format!("there is no page {} in this document", number_to_string(index))
                    }
                };
                plugin_error(code::NOT_FOUND, CAPABILITY, message, None, None)
            })
    }

    /// Engine slice to page-space hits. A match on a page that vanished
    /// mid-search (deleted) is dropped.
    fn hits_from_slice(&self, slice: SearchSlice) -> Vec<Rc<SearchHit>> {
        let Some(registry) = self.registry() else { return Vec::new() };
        let rows = registry.pages.as_array();
        let mut hits = Vec::with_capacity(slice.matches.len());
        for found in slice.matches {
            let Some(row) = registry
                .by_pon
                .get(&found.page.page_object_number)
                .and_then(|&i| rows.and_then(|rows| rows.get(i)))
            else {
                continue;
            };
            let page_index = row.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            // Segments arrive in page space, ready to draw.
            let bounds = if found.segments.is_empty() {
                None
            } else {
                bounds_of_rects(&found.segments.iter().map(|s| s.rect).collect::<Vec<_>>())
            };
            hits.push(Rc::new(SearchHit {
                page: found.page,
                page_index,
                start: found.start,
                count: found.count,
                segments: found.segments,
                bounds,
                snippet: found.snippet,
            }));
        }
        hits
    }

    /// The settings as typed; a member of the wrong type keeps its default.
    fn typed_settings(&self) -> SearchSettings {
        let value = self.settings.get();
        if let Ok(settings) = serde_json::from_value::<SearchSettings>((*value).clone()) {
            return settings;
        }
        let defaults = search_defaults();
        let member = |key: &str| value.get(key).cloned().unwrap_or(Value::Null);
        SearchSettings {
            reveal: serde_json::from_value(member("reveal")).unwrap_or(defaults.reveal),
            highlight: serde_json::from_value(member("highlight")).unwrap_or(defaults.highlight),
        }
    }
}

/// How a `collect` consumer observes the loop: the session updates state,
/// `find_all` accumulates.
trait CollectSink {
    /// Before the first slice, and again on a stale-cursor restart: reset
    /// any accumulation.
    fn on_start(&self);
    fn on_slice(&self, hits: Vec<Rc<SearchHit>>, pages_searched: f64, page_count: f64);
}

/// What `collect` scans: the query, where from, whether snippets are
/// pinned, and the signal that stops it.
struct Scan<'a> {
    query: &'a SearchQuery,
    from: Option<PageRef>,
    snippets: Option<bool>,
    signal: Option<AbortSignal>,
    reason_of: ReasonOf,
}

/// One cursor loop over budgeted slices: permission fallback (snippets to
/// none when they are denied, unless pinned), restart-once on a stale
/// cursor, projection per slice. `Ok(true)` on exhaustion, `Ok(false)` when
/// the signal aborted; `Err` on a real error (already a `PluginError`: the
/// handle is guarded).
async fn collect(core: &Core, scan: Scan<'_>, sink: &dyn CollectSink) -> Result<bool, Error> {
    let signal = scan.signal.as_ref();
    let aborted = || signal.is_some_and(AbortSignal::aborted);
    sink.on_start();

    let mut snippets = scan.snippets.unwrap_or(true);
    let snippets_pinned = scan.snippets.is_some();
    let mut cursor: Option<String> = None;
    let mut restarted = false;
    loop {
        if aborted() {
            return Ok(false);
        }
        let request = SearchRequest {
            query: scan.query.clone(),
            snippets: Some(snippets),
            from: if cursor.is_none() { scan.from } else { None },
            cursor: cursor.clone(),
            limit: None,
        };
        // A scan nobody waits on, so it runs once the views' work is done.
        // Cancelling stops the engine working on the slice, not only the
        // wait for it.
        let answer = match core.cx.doc() {
            Ok(doc) => {
                let request = serde_json::to_value(&request).unwrap_or(Value::Null);
                let facts = CallFacts { priority: Some(CallPriority::Low), view: None };
                let call = doc.call_with(&facts, "search.query", None, vec![request]);
                cancellable(signal, call, scan.reason_of).await.and_then(decode::<SearchSlice>)
            }
            Err(error) => Err(error),
        };
        let slice = match answer {
            Ok(slice) => slice,
            Err(error) => {
                if aborted() {
                    return Ok(false);
                }
                // Snippets denied (no doc.text.copy)? Degrade to matches
                // without them.
                if !snippets_pinned
                    && snippets
                    && cursor.is_none()
                    && error.is_plugin_error(Some(code::PERMISSION_DENIED))
                {
                    snippets = false;
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
        if aborted() {
            return Ok(false);
        }
        let next_cursor = slice.next_cursor.clone();
        let (pages_searched, page_count) = (slice.pages_searched, slice.page_count);
        sink.on_slice(core.hits_from_slice(slice), pages_searched, page_count);
        match next_cursor {
            None => return Ok(true),
            Some(next) => cursor = Some(next),
        }
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
            core.update(|state| start_session(state, query, &run.id));
            core.events.started.emit(&SearchStartedEvent { query: query.clone() });
        });
    }

    fn on_slice(&self, hits: Vec<Rc<SearchHit>>, pages_searched: f64, page_count: f64) {
        let progress = SearchProgress { pages_searched, page_count };
        self.run.commit(|| self.core.update(|state| append_hits(state, &hits, progress)));
    }
}

struct FindAllSink {
    all: RefCell<Vec<Rc<SearchHit>>>,
}

impl CollectSink for FindAllSink {
    fn on_start(&self) {
        self.all.borrow_mut().clear();
    }

    fn on_slice(&self, hits: Vec<Rc<SearchHit>>, _pages_searched: f64, _page_count: f64) {
        self.all.borrow_mut().extend(hits);
    }
}

/// `{...settings.reveal, ...options}` as the Stage's reveal reads it.
fn arrival(reveal: Option<&Value>, options: Option<SearchRevealOptions>) -> RevealArrival {
    // The setting's members: absent and `undefined` read as unset, a `null`
    // anchor as a positioned reveal with no axis.
    let member = |key: &str| reveal.and_then(|r| r.get(key)).filter(|v| !is_undefined(v));
    let mut anchor = member("anchor").and_then(|a| match a {
        Value::Null => Some(RevealAnchor::default()),
        a => serde_json::from_value::<RevealAnchor>(a.clone()).ok(),
    });
    let mut behavior = member("behavior")
        .and_then(|b| serde_json::from_value::<ScrollBehaviorKind>(b.clone()).ok());
    if let Some(options) = options {
        if let Some(over) = options.anchor {
            anchor = over;
        }
        if let Some(over) = options.behavior {
            behavior = over;
        }
    }
    RevealArrival { anchor, behavior }
}

struct RevealArrival {
    anchor: Option<RevealAnchor>,
    behavior: Option<ScrollBehaviorKind>,
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
        Ok(self.0.cx.try_get(&STAGE)?.and_then(|stage| stage.get_current_page()))
    }

    /// `connect()`: every confirmed mutation (own or remote) invalidates
    /// cursors and can change what is findable, so a running or finished
    /// session re-runs its query, 250 ms after the burst goes quiet.
    fn connect(&self) -> Result<(), Error> {
        let armed: Rc<RefCell<Option<Unsubscribe>>> = Rc::default();
        let doc = self.0.cx.doc()?;
        let weak = Rc::downgrade(&self.0);
        self.0.cx.listen(doc.subscribe(Rc::new(move |_event: &Value| {
            let Some(core) = weak.upgrade() else { return };
            if core.state().status == SearchStatus::Idle {
                return;
            }
            if let Some(cancel) = armed.borrow_mut().take() {
                cancel.call();
            }
            let (rerun, slot) = (Rc::downgrade(&core), armed.clone());
            let cancel = core.cx.clock().after(RERUN_DELAY_MS, move || {
                slot.borrow_mut().take();
                if let Some(core) = rerun.upgrade() {
                    // Surfaced through get_status() and on_failed.
                    SearchCapability(core).refresh(OperationOptions::default()).ignore();
                }
            });
            *armed.borrow_mut() = Some(cancel);
        })));
        Ok(())
    }

    // ── settings ──────────────────────────────────────────────────────────────

    /// The settings, for every open document.
    pub fn get_settings(&self) -> SearchSettings {
        self.0.typed_settings()
    }

    /// Merge `changes` into the settings, for every document. Fires
    /// `on_settings_changed` once when a setting changed.
    pub fn update_settings(&self, changes: &SearchConfig) {
        self.0.settings.store.update_settings(&changes.to_json());
    }

    /// Back to the settings the app registered.
    pub fn reset_settings(&self) {
        self.0.settings.store.reset_settings();
    }

    pub fn on_settings_changed(
        &self,
        listener: impl Fn(&SearchSettingsChanged) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        let weak = Rc::downgrade(&self.0);
        self.0.settings.events.on(move |event| {
            let Some(core) = weak.upgrade() else { return Ok(()) };
            listener(&SearchSettingsChanged {
                settings: core.typed_settings(),
                changed: event.changed.clone(),
            })
        })
    }

    /// Would a search be served now: `doc.text.search`. With `snippets:
    /// Some(true)` it also needs `doc.text.copy`, because a snippet
    /// reproduces document text.
    pub fn can_search(&self, snippets: Option<bool>) -> bool {
        let cx = &self.0.cx;
        cx.allows("doc.text.search") && (snippets != Some(true) || cx.allows("doc.text.copy"))
    }

    // ── the session ───────────────────────────────────────────────────────────

    /// Start a new search, superseding and aborting a running one. Hits
    /// stream into the session; the first becomes active, but the camera
    /// does not move until you navigate. Resolves when the scan completes,
    /// is superseded or is cancelled (also through `options.signal`);
    /// rejects only on a real failure, `permission-denied` without
    /// `doc.text.search`, or `not-found` for a `from` page that isn't in the
    /// document. A refusal only rejects: the session stays as it was and no
    /// event fires. An empty `query.text` is `clear()`.
    pub fn search(&self, query: SearchQuery, options: SearchOptions) -> Promise<SearchResult> {
        let api = self.clone();
        // Async so a refusal rejects before the session is touched. The run
        // still starts synchronously, so a newer search supersedes an older
        // one in the order they were called.
        Promise::spawn(async move {
            if query.text.is_empty() {
                api.clear();
                return Ok(SearchResult { status: SearchResultStatus::Complete, hit_count: 0 });
            }
            let core = api.core().clone();
            core.cx.assert_allowed("doc.text.search", "search")?;
            let from = match options.from {
                None => api.viewport_first_page()?,
                Some(target) => Some(core.page_of(target)?),
            };
            let operation_id: Rc<RefCell<String>> = Rc::default();
            let op = operation_id.clone();
            let work_core = core.clone();
            let running = core.session.run(
                move |run: LatestRun| {
                    Promise::spawn(async move {
                        *op.borrow_mut() = run.id.clone();
                        let core = work_core;
                        let sink = SessionSink {
                            core: core.clone(),
                            run: run.clone(),
                            query: query.clone(),
                        };
                        let scan = Scan {
                            query: &query,
                            from,
                            snippets: None,
                            signal: Some(run.signal.clone()),
                            reason_of: lane_reason,
                        };
                        let complete = rt::returned(collect(&core, scan, &sink)).await?;
                        if !complete {
                            return Err(plugin_error(
                                code::OPERATION_CANCELLED,
                                CAPABILITY,
                                "search aborted",
                                None,
                                None,
                            ));
                        }
                        run.commit(|| {
                            core.update(complete_session);
                            core.events
                                .completed
                                .emit(&SearchCompletedEvent { hit_count: core.state().hits.len() });
                        });
                        Ok(SearchResult {
                            status: SearchResultStatus::Complete,
                            hit_count: core.state().hits.len(),
                        })
                    })
                },
                options.signal,
            );
            let caught = Promise::spawn(async move {
                let error = match running.await {
                    Ok(result) => return Ok(result),
                    Err(error) => error,
                };
                let operation_id = operation_id.borrow().clone();
                let owns = |core: &Core| {
                    core.state().operation_id.as_deref() == Some(operation_id.as_str())
                };
                if error.is_plugin_error(Some(code::INSTANCE_CLOSED)) {
                    return Ok(SearchResult {
                        status: SearchResultStatus::Cancelled,
                        hit_count: 0,
                    });
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
                    // A superseded or cleared run has already lost the
                    // session; a cancelled one keeps its hits.
                    if reason == SearchCancelReason::Cancelled && owns(&core) {
                        core.update(cancel_session);
                    }
                    core.events.cancelled.emit(&SearchCancelledEvent { reason });
                    return Ok(SearchResult {
                        status: if reason == SearchCancelReason::Superseded {
                            SearchResultStatus::Superseded
                        } else {
                            SearchResultStatus::Cancelled
                        },
                        hit_count: core.state().hits.len(),
                    });
                }
                let info = error_info(&to_plugin_error(CAPABILITY, &error));
                if owns(&core) {
                    core.update(|state| fail_session(state, &info));
                }
                core.events.failed.emit(&SearchFailedEvent { error: info });
                Err(error)
            });
            adopt(caught).await
        })
    }

    /// Re-run the current query from scratch (the plugin does this itself
    /// after document mutations). Resolves and rejects as `search` does.
    pub fn refresh(&self, options: OperationOptions) -> Promise<SearchResult> {
        let state = self.state();
        match &state.query {
            Some(query) if state.status != SearchStatus::Idle => {
                self.search(query.clone(), SearchOptions { from: None, signal: options.signal })
            }
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
        self.0.update(clear_session);
    }

    /// Step to the next hit, wrapping, and reveal it.
    pub fn next_hit(
        &self,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<Rc<SearchHit>>, Error> {
        self.go_to_hit(HitTarget::Index(self.state().active_hit_index + 1), options)
    }

    /// Step to the previous hit, wrapping, and reveal it.
    pub fn previous_hit(
        &self,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<Rc<SearchHit>>, Error> {
        self.go_to_hit(HitTarget::Index(self.state().active_hit_index - 1), options)
    }

    /// Make a hit the active one and reveal it: the hit itself, or its index
    /// (which wraps). `None` when there are no hits or the given one isn't
    /// among them. `Err` only when the Stage's constructor throws.
    pub fn go_to_hit(
        &self,
        target: impl Into<HitTarget>,
        options: Option<SearchRevealOptions>,
    ) -> Result<Option<Rc<SearchHit>>, Error> {
        let hits: HitList = self.state().hits.clone();
        if hits.is_empty() {
            return Ok(None);
        }
        let index = match target.into() {
            HitTarget::Index(index) => {
                let len = hits.len() as isize;
                ((index % len) + len) % len
            }
            HitTarget::Hit(hit) => index_of_hit(&hits, &hit),
        };
        if index < 0 {
            return Ok(None);
        }
        self.0.update(|state| set_active_hit(state, index));
        let hit = hits[index as usize].clone();
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

    /// Positioned reveal: the hit (not just its page) arrives at the anchor,
    /// the call's options over the `reveal` setting. Zoom never changes.
    fn reveal(&self, hit: &SearchHit, options: Option<SearchRevealOptions>) -> Result<(), Error> {
        let settings = self.0.settings.get();
        let arrival = arrival(settings.get("reveal"), options);
        if let Some(stage) = self.0.cx.try_get(&STAGE)? {
            stage.reveal(
                hit.page,
                Some(RevealOptions {
                    rect: hit.bounds,
                    anchor: arrival.anchor,
                    behavior: arrival.behavior,
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

    /// One page's hits, by the page's ref or index. A page that isn't in
    /// the document has none: a layer may read while its page is deleted.
    fn hits_on(&self, page: PageTarget) -> HitList {
        let on = self
            .0
            .get_page(page)
            .and_then(|row| row.get("ref").and_then(PageRef::from_json))
            .and_then(|page| self.state().hits_by_page.get(&page.page_object_number).cloned());
        on.unwrap_or_else(|| Rc::new(Vec::new()))
    }

    /// Hits found so far, or those on one page. The same `Rc` (per page too)
    /// until new hits land there.
    pub fn list_hits(&self, page: Option<PageTarget>) -> HitList {
        match page {
            None => self.state().hits.clone(),
            Some(page) => self.hits_on(page),
        }
    }

    /// How many hits were found so far, or on one page; 0 for a page that
    /// isn't in the document.
    pub fn get_hit_count(&self, page: Option<PageTarget>) -> usize {
        self.list_hits(page).len()
    }

    /// The pages that have at least one hit, in document order.
    pub fn list_pages_with_hits(&self) -> Vec<PageRef> {
        let state = self.state();
        if state.hits.is_empty() {
            return Vec::new();
        }
        let pages: Vec<PageRef> = self
            .0
            .pages()
            .and_then(|pages| {
                pages.as_array().map(|rows| {
                    rows.iter().filter_map(|r| r.get("ref").and_then(PageRef::from_json)).collect()
                })
            })
            .unwrap_or_default();
        pages_with_hits(&state, &pages)
    }

    /// `-1` when no hit is active.
    pub fn get_active_hit_index(&self) -> isize {
        self.state().active_hit_index
    }

    pub fn get_active_hit(&self) -> Option<Rc<SearchHit>> {
        let state = self.state();
        usize::try_from(state.active_hit_index).ok().and_then(|i| state.hits.get(i)).cloned()
    }

    pub fn get_progress(&self) -> Rc<SearchProgress> {
        self.state().progress.clone()
    }

    pub fn get_error(&self) -> Option<PluginErrorInfo> {
        self.state().error.clone()
    }

    // ── the service ───────────────────────────────────────────────────────────

    /// Run a query to completion and return every hit, touching no session
    /// state. Scans in natural page order; concurrent calls are
    /// independent. Rejects `operation-cancelled` when `options.signal`
    /// aborts, and `permission-denied` without `doc.text.search`.
    pub fn find_all(
        &self,
        query: SearchQuery,
        options: SearchFindAllOptions,
    ) -> Promise<Vec<Rc<SearchHit>>> {
        let core = self.core().clone();
        Promise::spawn(async move {
            core.cx.assert_allowed("doc.text.search", "findAll")?;
            let sink = FindAllSink { all: RefCell::new(Vec::new()) };
            let scan = Scan {
                query: &query,
                from: None,
                snippets: options.snippets,
                signal: options.signal,
                reason_of: error_reason,
            };
            let complete = rt::returned(collect(&core, scan, &sink)).await?;
            if !complete {
                return Err(plugin_error(
                    code::OPERATION_CANCELLED,
                    CAPABILITY,
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

    /// `get_progress()` changed: once per slice of pages a search gets
    /// through, and back to none when a new search starts or the session is
    /// cleared.
    pub fn on_progress_changed(
        &self,
        listener: impl Fn(&SearchProgressChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.progress_changed.on(listener)
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
