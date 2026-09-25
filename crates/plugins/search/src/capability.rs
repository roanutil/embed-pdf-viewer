//! `capability.ts`: a client-driven cursor loop over the engine's budgeted
//! slices, shared by the session (`search`) and the session-free service
//! (`find_all`). One generation counter arbitrates the session: a new
//! search, a clear or a rerun supersedes the running loop, which aborts its
//! in-flight slice and stops dispatching.
//!
//! Turns are spelled out where they cost more than one: an engine call is an
//! `AbortablePromise`, which `await` adopts as a thenable
//! ([`await_engine`]), and awaiting the ported `async` `collect` is
//! `rt::returned`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use epdf_geometry::{
    CropBox, Mat2D, PageGeometryInput, PageRotation, Point, TextQuad, apply_point, bounds_of_rects,
    page_geometry, text_quad_bounds,
};
use epdf_kernel::engine::decode;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Ctx, Error};
use epdf_plugin_stage::{
    RevealAnchor, RevealAnchorValue, RevealOptions, STAGE, ScrollBehaviorKind,
};
use serde_json::{Value, json};

use crate::types::*;

/// The browser find-bar feel: the hit at the top-middle, no zoom change.
fn default_reveal() -> SearchRevealOptions {
    SearchRevealOptions {
        anchor: Some(Some(RevealAnchor { x: None, y: Some(RevealAnchorValue::Fraction(0.35)) })),
        behavior: Some(Some(ScrollBehaviorKind::Smooth)),
    }
}

/// Local engines reject a denied scope before the worker; cloud maps 403.
fn is_permission_denied(err: &Error) -> bool {
    err.name == "PermissionDenied" || is_engine_error(err, "Forbidden")
}

/// `EngineError.is(err, code)`.
fn is_engine_error(err: &Error, code: &str) -> bool {
    err.name == "EngineError" && err.member("code") == Some(&json!(code))
}

/// `Promise.resolve(abortable)` awaited: the engine's `AbortablePromise` is
/// not a native promise, so `await` adopts it as a thenable. It settles
/// three turns after an already-settled call, two after a later one.
async fn await_engine(call: Promise<Value>) -> Result<Value, Error> {
    Promise::spawn(async move {
        rt::tick().await;
        call.await
    })
    .await
}

/// One page's conversion from PDF space to content space.
#[derive(Clone, Copy)]
struct PageConverter {
    page_index: usize,
    pdf_to_content: Mat2D,
}

impl PageConverter {
    /// Engine quads carry frame-geometric slot semantics, so the y-flip maps
    /// the corners straight onto their names.
    fn to_quad(self, q: &epdf_geometry::Quad) -> TextQuad {
        let to_point = |p: Point| apply_point(self.pdf_to_content, p);
        TextQuad {
            upper_start: to_point(q.p1),
            upper_end: to_point(q.p2),
            lower_start: to_point(q.p3),
            lower_end: to_point(q.p4),
        }
    }
}

fn num(value: Option<&Value>) -> f64 {
    value.and_then(Value::as_f64).unwrap_or(f64::NAN)
}

/// How a `collect` consumer observes the loop: the session dispatches into
/// state, `find_all` accumulates locally.
trait CollectSink {
    /// Called before the first slice and again on a stale-cursor restart.
    fn on_start(&self);
    fn on_slice(&self, hits: Vec<SearchHit>, scanned: f64, total: f64);
    /// Superseded or aborted: the loop stops without completing.
    fn is_cancelled(&self) -> bool;
    fn set_inflight(&self, pending: Option<Promise<Value>>);
}

struct Inner {
    ctx: Ctx<SearchState, SearchAction>,
    config: SearchPluginConfig,
    generation: Cell<u64>,
    inflight: RefCell<Option<Promise<Value>>>,
}

/// `SearchCapability`. Cheap to clone; clones share the session.
#[derive(Clone)]
pub struct SearchCapability {
    inner: Rc<Inner>,
}

/// `createSearchCapability(ctx, config)`.
pub fn create_search_capability(
    ctx: Ctx<SearchState, SearchAction>,
    config: SearchPluginConfig,
) -> SearchCapability {
    SearchCapability {
        inner: Rc::new(Inner {
            ctx,
            config,
            generation: Cell::new(0),
            inflight: RefCell::new(None),
        }),
    }
}

struct SessionSink {
    inner: Rc<Inner>,
    query: SearchQuery,
    generation: u64,
}

impl CollectSink for SessionSink {
    fn on_start(&self) {
        self.inner.ctx.dispatch(SearchAction::Start { query: self.query.clone() });
    }
    fn on_slice(&self, hits: Vec<SearchHit>, scanned: f64, total: f64) {
        self.inner.ctx.dispatch(SearchAction::Append { hits, scanned, total });
    }
    fn is_cancelled(&self) -> bool {
        self.generation != self.inner.generation.get()
    }
    fn set_inflight(&self, pending: Option<Promise<Value>>) {
        if self.generation == self.inner.generation.get() {
            *self.inner.inflight.borrow_mut() = pending;
        }
    }
}

struct FindAllSink {
    all: Rc<RefCell<Vec<SearchHit>>>,
    pending: Rc<RefCell<Option<Promise<Value>>>>,
    signal: Option<epdf_kernel::signal::AbortSignal>,
}

impl CollectSink for FindAllSink {
    fn on_start(&self) {
        self.all.borrow_mut().clear();
    }
    fn on_slice(&self, hits: Vec<SearchHit>, _scanned: f64, _total: f64) {
        self.all.borrow_mut().extend(hits);
    }
    fn is_cancelled(&self) -> bool {
        self.signal.as_ref().is_some_and(|s| s.aborted())
    }
    fn set_inflight(&self, pending: Option<Promise<Value>>) {
        *self.pending.borrow_mut() = pending;
    }
}

/// Rect conversion, once per slice: the engine's PDF-space quads to content
/// space through the page geometry selection uses, at zoom 1. A match on a
/// page the document no longer has (deleted mid-search) is dropped.
fn hits_from_slice(
    ctx: &Ctx<SearchState, SearchAction>,
    slice: &SearchSlice,
) -> Result<Vec<SearchHit>, Error> {
    let mut converters: HashMap<PageObjectNumber, Option<PageConverter>> = HashMap::new();
    let mut hits = Vec::new();
    for m in &slice.matches {
        let pon = m.page_object_number;
        let conv = match converters.get(&pon) {
            Some(conv) => *conv,
            None => {
                let conv = converter_for(ctx, pon)?;
                converters.insert(pon, conv);
                conv
            }
        };
        let Some(conv) = conv else { continue };
        let segments: Vec<TextSegment> = m
            .segments
            .iter()
            .map(|segment| {
                let quad = conv.to_quad(&segment.quad);
                // `rect` is recomputed from the mapped quad so the pair can't drift.
                TextSegment { quad, rect: text_quad_bounds(quad), advance: segment.advance }
            })
            .collect();
        let bounds = if segments.is_empty() {
            None
        } else {
            bounds_of_rects(&segments.iter().map(|s| s.rect).collect::<Vec<_>>())
        };
        hits.push(SearchHit {
            pon,
            page_index: conv.page_index,
            char_start: m.char_start,
            char_count: m.char_count,
            segments,
            bounds,
            snippet: m.snippet.clone(),
        });
    }
    Ok(hits)
}

fn converter_for(
    ctx: &Ctx<SearchState, SearchAction>,
    pon: PageObjectNumber,
) -> Result<Option<PageConverter>, Error> {
    let layout = ctx.document().and_then(|doc| {
        doc.pages.as_array().and_then(|pages| {
            pages
                .iter()
                .find(|p| p.get("pageObjectNumber").and_then(Value::as_f64) == Some(f64::from(pon)))
                .cloned()
        })
    });
    let Some(layout) = layout else { return Ok(None) };
    let crop = layout.get("boxes").and_then(|b| b.get("crop"));
    let input = PageGeometryInput {
        crop: CropBox {
            left: num(crop.and_then(|c| c.get("left"))),
            bottom: num(crop.and_then(|c| c.get("bottom"))),
            right: num(crop.and_then(|c| c.get("right"))),
            top: num(crop.and_then(|c| c.get("top"))),
        },
        rotation: PageRotation(num(layout.get("rotation"))),
        user_unit: num(layout.get("userUnit")),
    };
    let geometry = page_geometry(input, 1.0).map_err(|e| Error::new(e.to_string()))?;
    Ok(Some(PageConverter {
        page_index: layout.get("index").and_then(Value::as_u64).unwrap_or(0) as usize,
        pdf_to_content: geometry.pdf_to_content,
    }))
}

/// THE MECHANISM: one cursor loop over the engine's budgeted slices, with
/// the permission fallback (`Full` to `Rects` when snippets are denied,
/// unless the caller pinned a mode), restart-once on a stale cursor, and
/// rect conversion per slice. `Ok(true)` on exhaustion, `Ok(false)` when
/// cancelled.
async fn collect<S: CollectSink>(
    ctx: Ctx<SearchState, SearchAction>,
    query: SearchQuery,
    start_page: Option<PageObjectNumber>,
    pinned_mode: Option<SearchMode>,
    sink: S,
) -> Result<bool, Error> {
    let Some(doc) = ctx.doc() else { return Ok(false) };

    sink.on_start();

    let mut mode = pinned_mode.unwrap_or(SearchMode::Full);
    let mode_pinned = pinned_mode.is_some();
    let mut cursor: Option<String> = None;
    let mut restarted = false;
    loop {
        let request = SearchRequest {
            query: query.clone(),
            mode: Some(mode),
            start_page: if cursor.is_none() { start_page } else { None },
            cursor: cursor.clone(),
            skip: None,
            budget: None,
        };
        let request = serde_json::to_value(&request).unwrap_or(Value::Null);
        let pending = doc.call("search.query", None, vec![request]);
        sink.set_inflight(Some(pending.clone()));
        let slice = match await_engine(pending).await.and_then(decode::<SearchSlice>) {
            Ok(slice) => slice,
            Err(err) => {
                if sink.is_cancelled() {
                    return Ok(false); // superseded: the abort was ours
                }
                // Snippets denied (no doc.text.copy)? Degrade to rect-only
                // matches instead of failing the whole search.
                if !mode_pinned
                    && mode == SearchMode::Full
                    && cursor.is_none()
                    && is_permission_denied(&err)
                {
                    mode = SearchMode::Rects;
                    continue;
                }
                // A stale cursor rejects with InvalidArg: position is lost,
                // the query isn't. Restart once from scratch.
                if cursor.is_some() && !restarted && is_engine_error(&err, "InvalidArg") {
                    restarted = true;
                    cursor = None;
                    sink.on_start();
                    continue;
                }
                return Err(err);
            }
        };
        if sink.is_cancelled() {
            return Ok(false);
        }
        let hits = hits_from_slice(&ctx, &slice)?;
        sink.on_slice(hits, slice.scanned_pages, slice.total_pages);
        match slice.next_cursor {
            None => {
                sink.set_inflight(None);
                return Ok(true);
            }
            Some(next) => cursor = Some(next),
        }
    }
}

/// `{...DEFAULT_REVEAL, ...config.reveal, ...reveal}`.
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

impl SearchCapability {
    fn ctx(&self) -> &Ctx<SearchState, SearchAction> {
        &self.inner.ctx
    }

    fn abort_inflight(&self, reason: &str) {
        let inflight = self.inner.inflight.borrow_mut().take();
        if let Some(inflight) = inflight {
            inflight.abort(Some(json!(reason)));
        }
    }

    /// THE SESSION: `collect` with state as the sink, generation-guarded.
    fn run_session(&self, query: SearchQuery, start_page: Option<PageObjectNumber>) {
        let inner = self.inner.clone();
        rt::spawn(async move {
            let generation = inner.generation.get() + 1;
            inner.generation.set(generation);
            let inflight = inner.inflight.borrow_mut().take();
            if let Some(inflight) = inflight {
                inflight.abort(Some(json!("superseded")));
            }
            let ctx = inner.ctx.clone();

            if ctx.doc().is_none() {
                ctx.dispatch(SearchAction::Clear);
                return;
            }

            // Viewport-first: begin scanning where the user is looking. A
            // Stage constructor that throws rejects this (voided) call.
            let mut start_page = start_page;
            if start_page.is_none() {
                let Ok(stage) = ctx.try_get(&STAGE) else { return };
                if let Some(stage) = stage {
                    start_page = ctx.document().and_then(|doc| {
                        doc.pages
                            .get(stage.current_page())
                            .and_then(|p| p.get("pageObjectNumber"))
                            .and_then(Value::as_u64)
                            .map(|p| p as PageObjectNumber)
                    });
                }
            }

            let sink = SessionSink { inner: inner.clone(), query: query.clone(), generation };
            let result = rt::returned(collect(ctx.clone(), query, start_page, None, sink)).await;
            match result {
                Ok(complete) => {
                    if complete && generation == inner.generation.get() {
                        ctx.dispatch(SearchAction::Complete);
                    }
                }
                Err(err) => {
                    if generation != inner.generation.get() {
                        return;
                    }
                    ctx.dispatch(SearchAction::Error { message: err.message });
                }
            }
        });
    }

    /// The twin: would a search succeed now? `None` or `Rects` asks
    /// `doc.text.search`; `Full` also needs `doc.text.copy`.
    pub fn can_search(&self, mode: Option<SearchMode>) -> bool {
        let Some(doc) = self.ctx().doc() else { return false };
        let sec = doc.security();
        sec.allows("doc.text.search")
            && (mode != Some(SearchMode::Full) || sec.allows("doc.text.copy"))
    }

    /// Start a new search, superseding and aborting any running one. An
    /// empty `query.text` is `clear()`.
    pub fn search(&self, query: SearchQuery, exec: Option<SearchExecOptions>) {
        if query.text.is_empty() {
            self.clear();
        } else {
            self.run_session(query, exec.and_then(|e| e.start_page));
        }
    }

    /// Re-run the stored query from scratch (after a document mutation).
    pub fn rerun(&self) {
        let state = self.ctx().state();
        if let Some(query) = &state.query
            && state.status != SearchStatus::Idle
        {
            self.run_session(query.clone(), None);
        }
    }

    pub fn clear(&self) {
        self.inner.generation.set(self.inner.generation.get() + 1);
        self.abort_inflight("cleared");
        self.ctx().dispatch(SearchAction::Clear);
    }

    /// Advance to the next hit (wrapping) and reveal it.
    pub fn next(&self) -> Result<Option<SearchHit>, Error> {
        self.go_to(self.ctx().state().active_index + 1, None)
    }

    /// Back to the previous hit (wrapping) and reveal it.
    pub fn prev(&self) -> Result<Option<SearchHit>, Error> {
        self.go_to(self.ctx().state().active_index - 1, None)
    }

    /// Jump to a hit index (wrapped modulo the hit count) and reveal it;
    /// `reveal` overrides the plugin's arrival defaults. `Err` only when the
    /// Stage's constructor throws.
    pub fn go_to(
        &self,
        index: isize,
        reveal: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, Error> {
        let hits = self.ctx().state().hits.clone();
        if hits.is_empty() {
            return Ok(None);
        }
        let len = hits.len() as isize;
        let wrapped = ((index % len) + len) % len;
        self.ctx().dispatch(SearchAction::SetActive { index: wrapped });
        let hit = hits[wrapped as usize].clone();
        // Positioned reveal: the hit arrives at the anchor. Per-call over
        // plugin config over the find-bar default; zoom never changes.
        let arrival =
            merge_reveal(merge_reveal(default_reveal(), self.inner.config.reveal), reveal);
        if let Some(stage) = self.ctx().try_get(&STAGE)? {
            stage.reveal(
                hit.page_index as isize,
                Some(RevealOptions {
                    rect: hit.bounds,
                    anchor: arrival.anchor.flatten(),
                    behavior: arrival.behavior.flatten(),
                    zoom: None,
                }),
            );
        }
        Ok(Some(hit))
    }

    /// The session's query: what a search box renders. `None` when idle.
    pub fn query(&self) -> Option<SearchQuery> {
        self.ctx().state().query.clone()
    }

    /// Run a query to completion and return every hit, touching no state.
    /// Scans in natural page order; concurrent calls are independent. With
    /// a signal, aborting it aborts the in-flight slice and rejects with the
    /// signal's reason.
    pub fn find_all(
        &self,
        query: SearchQuery,
        opts: SearchFindAllOptions,
    ) -> Promise<Vec<SearchHit>> {
        let ctx = self.ctx().clone();
        Promise::spawn(async move {
            if ctx.doc().is_none() {
                return Ok(Vec::new());
            }
            let all = Rc::new(RefCell::new(Vec::new()));
            let signal = opts.signal;
            // Its own abort handle, never the session's generation: a
            // findAll neither supersedes the visible search nor is
            // superseded by it.
            let pending: Rc<RefCell<Option<Promise<Value>>>> = Rc::default();
            let listener = signal.as_ref().map(|s| {
                let (pending, s2) = (pending.clone(), s.clone());
                s.on_abort(Rc::new(move |_| {
                    let pending = pending.borrow().clone();
                    if let Some(pending) = pending {
                        pending.abort(s2.reason().map(|r| r.to_canonical()));
                    }
                }))
            });
            let sink = FindAllSink { all: all.clone(), pending, signal: signal.clone() };
            let result = rt::returned(collect(ctx, query, None, opts.mode, sink)).await;
            if let (Some(signal), Some(id)) = (&signal, listener) {
                signal.remove(id);
            }
            result?;
            if let Some(signal) = signal.filter(|s| s.aborted()) {
                return Err(signal.reason().unwrap_or_else(|| Error::new("findAll aborted")));
            }
            Ok(all.take())
        })
    }

    pub fn status(&self) -> SearchStatus {
        self.ctx().state().status
    }

    pub fn hits(&self) -> Rc<Vec<SearchHit>> {
        self.ctx().state().hits.clone()
    }

    pub fn hit_count(&self) -> usize {
        self.ctx().state().hits.len()
    }

    pub fn active_index(&self) -> isize {
        self.ctx().state().active_index
    }

    pub fn active_hit(&self) -> Option<SearchHit> {
        let state = self.ctx().state();
        usize::try_from(state.active_index).ok().and_then(|i| state.hits.get(i)).cloned()
    }

    /// Hits on one page: the layer's input.
    pub fn hits_for_page(&self, pon: PageObjectNumber) -> Vec<SearchHit> {
        let state = self.ctx().state();
        match state.hits_by_page.get(&pon) {
            Some(indices) if !indices.is_empty() => {
                indices.iter().map(|&i| state.hits[i].clone()).collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn progress(&self) -> SearchProgress {
        self.ctx().state().progress
    }

    pub fn error_message(&self) -> Option<String> {
        self.ctx().state().error.clone()
    }
}
