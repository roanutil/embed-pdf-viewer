//! The search plugin over the C ABI: the members Task 11 measures
//! (`hit_count`, `hits`, `search`, `find_all`, `subscribe`), each starting
//! the same plugin work as `epdf_plugin_search::ffi::SearchHandle`.
//!
//! A hit list crosses as flat arrays the caller reads in place: one
//! `EpdfSearchHit` per hit, every hit's segments in one `EpdfSegment` array
//! (`segment_offset`, `segment_count`), and every snippet's UTF-8 in one
//! byte pool. The arrays live in a boxed owner until `epdf_hits_free`.

use std::ffi::{CString, c_char, c_void};
use std::rc::Rc;

use epdf_ffi::{KernelBound, KernelPost, Subscription};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_search::ffi::{open, start_find_all, start_search, whole};
use epdf_plugin_search::{SearchCancelReason, SearchCapability, SearchQuery};

use crate::{Call, Completion, EpdfComplete, SendPtr, arg, registry};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EpdfPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EpdfRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EpdfQuad {
    pub upper_start: EpdfPoint,
    pub upper_end: EpdfPoint,
    pub lower_start: EpdfPoint,
    pub lower_end: EpdfPoint,
}

/// `TextSegment`: one merged visual line of a match, in page space.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EpdfSegment {
    pub quad: EpdfQuad,
    pub rect: EpdfRect,
    pub advance: f64,
}

/// `SearchHit`, flat. `bounds` is meaningful when `has_bounds`; the
/// snippet fields when `has_snippet`, and `snippet_text` is
/// `snippet_len` bytes of UTF-8 with no terminator.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct EpdfSearchHit {
    pub page_object_number: u32,
    pub page_index: u32,
    pub char_start: f64,
    pub char_count: f64,
    pub bounds: EpdfRect,
    pub segment_offset: u32,
    pub segment_count: u32,
    pub snippet_text: *const u8,
    pub snippet_len: usize,
    pub snippet_match_start: f64,
    pub snippet_match_length: f64,
    pub has_bounds: u8,
    pub has_snippet: u8,
}

#[repr(C)]
pub struct EpdfHits {
    pub hits: *const EpdfSearchHit,
    pub hit_count: usize,
    pub segments: *const EpdfSegment,
    pub segment_count: usize,
    /// Free with `epdf_hits_free`. Null in an event payload, which the
    /// bridge frees itself after the callback.
    pub owner: *mut c_void,
}

/// The arrays an `EpdfHits` points into.
struct HitsOwner {
    hits: Vec<EpdfSearchHit>,
    segments: Vec<EpdfSegment>,
    _text: Vec<u8>,
}

impl HitsOwner {
    fn build<H: std::borrow::Borrow<epdf_plugin_search::SearchHit>>(hits: &[H]) -> Result<Box<HitsOwner>, Error> {
        let mut text = Vec::new();
        for hit in hits.iter().map(H::borrow) {
            if let Some(snippet) = &hit.snippet {
                text.extend_from_slice(snippet.text.as_bytes());
            }
        }
        let base = text.as_ptr();
        let mut at = 0usize;
        let mut segments = Vec::with_capacity(hits.len());
        let mut flat = Vec::with_capacity(hits.len());
        for hit in hits.iter().map(H::borrow) {
            let segment_offset = whole(segments.len(), "a segment offset")?;
            segments.extend(hit.segments.iter().map(|s| {
                let q = epdf_ffi::TextQuad::from(s.quad);
                let point = |p: epdf_ffi::Point| EpdfPoint { x: p.x, y: p.y };
                EpdfSegment {
                    quad: EpdfQuad {
                        upper_start: point(q.upper_start),
                        upper_end: point(q.upper_end),
                        lower_start: point(q.lower_start),
                        lower_end: point(q.lower_end),
                    },
                    rect: rect(epdf_ffi::Rect::from(s.rect)),
                    advance: s.advance,
                }
            }));
            let (snippet_text, snippet_len, match_start, match_length) = match &hit.snippet {
                Some(snippet) => {
                    let len = snippet.text.len();
                    // `text` is complete and never grows again.
                    let ptr = unsafe { base.add(at) };
                    at += len;
                    (ptr, len, snippet.match_start, snippet.match_length)
                }
                None => (std::ptr::null(), 0, 0.0, 0.0),
            };
            flat.push(EpdfSearchHit {
                page_object_number: hit.page.page_object_number,
                page_index: whole(hit.page_index, "a hit's page index")?,
                char_start: hit.char_start,
                char_count: hit.char_count,
                bounds: hit.bounds.map(|b| rect(b.into())).unwrap_or_default(),
                segment_offset,
                segment_count: whole(hit.segments.len(), "a hit's segment count")?,
                snippet_text,
                snippet_len,
                snippet_match_start: match_start,
                snippet_match_length: match_length,
                has_bounds: hit.bounds.is_some() as u8,
                has_snippet: hit.snippet.is_some() as u8,
            });
        }
        Ok(Box::new(HitsOwner { hits: flat, segments, _text: text }))
    }

    fn view(&self, owner: *mut c_void) -> EpdfHits {
        EpdfHits {
            hits: self.hits.as_ptr(),
            hit_count: self.hits.len(),
            segments: self.segments.as_ptr(),
            segment_count: self.segments.len(),
            owner,
        }
    }
}

fn rect(r: epdf_ffi::Rect) -> EpdfRect {
    EpdfRect { x: r.x, y: r.y, width: r.width, height: r.height }
}

/// Hand a hit list to `completion`; the caller owns it from here.
fn answer_hits<H: std::borrow::Borrow<epdf_plugin_search::SearchHit>>(completion: Completion, hits: Result<&[H], Error>) {
    match hits.and_then(HitsOwner::build) {
        Ok(owner) => {
            let raw = Box::into_raw(owner);
            let view = unsafe { &*raw }.view(raw as *mut c_void);
            completion.ok(&view);
        }
        Err(error) => completion.fail(error),
    }
}

/// # Safety
/// `owner` came from an `EpdfHits` and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_hits_free(owner: *mut c_void) {
    if !owner.is_null() {
        drop(unsafe { Box::from_raw(owner as *mut HitsOwner) });
    }
}

/// A document's search capability, held on the kernel thread.
pub struct EpdfSearch {
    bound: KernelBound<SearchCapability>,
    post: KernelPost,
}

/// Open `document_id`'s search capability. `result` is the `EpdfSearch*`,
/// which the caller frees with `epdf_search_free`.
///
/// # Safety
/// `document_id` is NUL-terminated; `done` is called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_open(
    kernel: u64,
    document_id: *const c_char,
    ctx: *mut c_void,
    done: EpdfComplete,
) {
    let completion = Completion::new(ctx, done);
    let id = match arg(document_id, "the document id") {
        Ok(id) => id,
        Err(error) => return completion.fail(error),
    };
    let Some(registered) = registry::lookup(kernel) else { return };
    let post = registered.post.clone();
    (registered.run)(Box::new(move |kernel| {
        match kernel.capability(&epdf_plugin_search::SEARCH, Some(&id)) {
            Ok(cap) => {
                let search =
                    Box::new(EpdfSearch { bound: KernelBound::new(cap, post.clone()), post });
                completion.ok_ptr(Box::into_raw(search) as *const c_void);
            }
            Err(error) => completion.fail(error),
        }
    }));
}

/// # Safety
/// `search` came from `epdf_search_open` and is freed once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_free(search: *mut EpdfSearch) {
    if !search.is_null() {
        drop(unsafe { Box::from_raw(search) });
    }
}

/// How many hits the session holds. `result` is a `const uint32_t*`.
///
/// # Safety
/// `search` is live; `done` is called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_hit_count(
    search: *mut EpdfSearch,
    ctx: *mut c_void,
    done: EpdfComplete,
) {
    let completion = Completion::new(ctx, done);
    let search = unsafe { &*search };
    search.bound.fire(move |cap| {
        match open(cap).and_then(|c| whole(c.get_hit_count(None), "a hit count")) {
            Ok(n) => completion.ok(&n),
            Err(error) => completion.fail(error),
        }
    });
}

/// The session's hits. `result` is a `const EpdfHits*`; free its `owner`.
///
/// # Safety
/// `search` is live; `done` is called once with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_hits(
    search: *mut EpdfSearch,
    ctx: *mut c_void,
    done: EpdfComplete,
) {
    let completion = Completion::new(ctx, done);
    let search = unsafe { &*search };
    search.bound.fire(move |cap| match open(cap) {
        Ok(cap) => answer_hits(completion, Ok(cap.list_hits(None).as_slice())),
        Err(error) => completion.fail(error),
    });
}

/// Start a session search for `text`, superseding a running one. Answers
/// the call id for `epdf_cancel`; `result` is a `const uint32_t*` hit
/// count. Cancelling keeps the hits found so far and still answers.
///
/// # Safety
/// `search` is live, `text` NUL-terminated; `done` is called once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_search(
    search: *mut EpdfSearch,
    text: *const c_char,
    ctx: *mut c_void,
    done: EpdfComplete,
) -> u64 {
    let completion = Completion::new(ctx, done);
    let text = match arg(text, "the query") {
        Ok(text) => text,
        Err(error) => {
            completion.fail(error);
            return 0;
        }
    };
    let search = unsafe { &*search };
    let call = Call::begin(&search.post);
    let id = call.id();
    search.bound.fire(move |cap| {
        let query = SearchQuery { text, ..SearchQuery::default() };
        match start_search(cap, query, None) {
            Ok(promise) => call.settle(promise, move |result| match result {
                Ok(result) => match whole(result.hit_count, "a hit count") {
                    Ok(n) => completion.ok(&n),
                    Err(error) => completion.fail(error),
                },
                Err(error) => completion.fail(error.clone()),
            }),
            Err(error) => completion.fail(error),
        }
    });
    id
}

/// Run `text` to completion and answer every hit, touching no session
/// state. `mode` is the plugin's default (`Full`, falling back to `Rects`).
/// `result` is a `const EpdfHits*`; free its `owner`. Cancelling answers
/// `operation-cancelled`.
///
/// # Safety
/// `search` is live, `text` NUL-terminated; `done` is called once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_find_all(
    search: *mut EpdfSearch,
    text: *const c_char,
    ctx: *mut c_void,
    done: EpdfComplete,
) -> u64 {
    let completion = Completion::new(ctx, done);
    let text = match arg(text, "the query") {
        Ok(text) => text,
        Err(error) => {
            completion.fail(error);
            return 0;
        }
    };
    let search = unsafe { &*search };
    let call = Call::begin(&search.post);
    let id = call.id();
    search.bound.fire(move |cap| {
        let query = SearchQuery { text, ..SearchQuery::default() };
        match start_find_all(cap, query, None) {
            Ok(promise) => call.settle(promise, move |result| match result {
                Ok(hits) => answer_hits(completion, Ok(hits)),
                Err(error) => completion.fail(error.clone()),
            }),
            Err(error) => completion.fail(error),
        }
    });
    id
}

// ── events ────────────────────────────────────────────────────────────────────

pub const EPDF_SEARCH_EVENT_STARTED: i32 = 0;
pub const EPDF_SEARCH_EVENT_PROGRESS: i32 = 1;
pub const EPDF_SEARCH_EVENT_COMPLETED: i32 = 2;
pub const EPDF_SEARCH_EVENT_CANCELLED: i32 = 3;
pub const EPDF_SEARCH_EVENT_FAILED: i32 = 4;
pub const EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED: i32 = 5;
pub const EPDF_SEARCH_EVENT_CLEARED: i32 = 6;
/// The document closed: no event follows. The payload is null.
pub const EPDF_SEARCH_EVENT_END: i32 = 7;

#[repr(C)]
pub struct EpdfSearchStarted {
    pub text: *const c_char,
    pub operation_id: *const c_char,
}

#[repr(C)]
pub struct EpdfSearchProgress {
    pub scanned: f64,
    pub total: f64,
    pub hit_count: u32,
    pub operation_id: *const c_char,
}

#[repr(C)]
pub struct EpdfSearchCompleted {
    pub hit_count: u32,
    pub operation_id: *const c_char,
}

/// `reason`: 0 superseded, 1 cancelled, 2 cleared.
#[repr(C)]
pub struct EpdfSearchCancelled {
    pub operation_id: *const c_char,
    pub reason: i32,
}

#[repr(C)]
pub struct EpdfSearchFailed {
    pub code: *const c_char,
    pub message: *const c_char,
    pub operation_id: *const c_char,
}

/// `hits` holds the one active hit, or is null when none is active.
#[repr(C)]
pub struct EpdfSearchActiveHitChanged {
    pub index: u32,
    pub hits: *const EpdfHits,
}

/// `payload` points at the kind's struct (null for cleared and end) and
/// lives for the call only.
pub type EpdfSearchEvent = extern "C" fn(ctx: *mut c_void, kind: i32, payload: *const c_void);
/// Called once, on the kernel thread, when no event can follow.
pub type EpdfRelease = extern "C" fn(ctx: *mut c_void);

/// A live listener; freeing it cancels.
pub struct EpdfSubscription(#[allow(dead_code)] std::sync::Arc<Subscription>);

struct Listener {
    ctx: SendPtr<c_void>,
    on_event: EpdfSearchEvent,
    release: Option<EpdfRelease>,
}

impl Listener {
    fn emit<T>(&self, kind: i32, payload: &T) {
        (self.on_event)(self.ctx.0, kind, payload as *const T as *const c_void);
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        if let Some(release) = self.release {
            release(self.ctx.0);
        }
    }
}

fn c(s: &str) -> CString {
    CString::new(s.split('\0').next().unwrap_or_default()).unwrap_or_default()
}

/// Register for every session event. `release` (may be null) runs once
/// when the listener is gone: after `epdf_subscription_free` takes effect,
/// or with the end of the document.
///
/// # Safety
/// `search` is live; `ctx` stays valid until `release`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_search_subscribe(
    search: *mut EpdfSearch,
    ctx: *mut c_void,
    on_event: EpdfSearchEvent,
    release: Option<EpdfRelease>,
) -> *mut EpdfSubscription {
    let search = unsafe { &*search };
    let (subscription, slot) = Subscription::pending(search.post.clone());
    let listener = Listener { ctx: SendPtr(ctx), on_event, release };
    search.bound.fire(move |cap| {
        let listener = Rc::new(listener);
        let ending = listener.clone();
        slot.fill_until_end(
            cap.lifetime(),
            || {
                macro_rules! relay {
                    ($hook:ident, |$l:ident, $e:ident| $body:expr) => {{
                        let $l = listener.clone();
                        cap.$hook(move |$e| {
                            $body;
                            Ok(())
                        })
                    }};
                }
                let offs: Vec<Unsubscribe> = vec![
                    relay!(on_started, |l, e| {
                        let (text, op) = (c(&e.query.text), c(&e.operation_id));
                        l.emit(
                            EPDF_SEARCH_EVENT_STARTED,
                            &EpdfSearchStarted { text: text.as_ptr(), operation_id: op.as_ptr() },
                        )
                    }),
                    relay!(on_progress, |l, e| {
                        let op = c(&e.operation_id);
                        l.emit(
                            EPDF_SEARCH_EVENT_PROGRESS,
                            &EpdfSearchProgress {
                                scanned: e.scanned,
                                total: e.total,
                                hit_count: whole(e.hit_count, "a hit count")?,
                                operation_id: op.as_ptr(),
                            },
                        )
                    }),
                    relay!(on_completed, |l, e| {
                        let op = c(&e.operation_id);
                        l.emit(
                            EPDF_SEARCH_EVENT_COMPLETED,
                            &EpdfSearchCompleted {
                                hit_count: whole(e.hit_count, "a hit count")?,
                                operation_id: op.as_ptr(),
                            },
                        )
                    }),
                    relay!(on_cancelled, |l, e| {
                        let op = c(&e.operation_id);
                        let reason = match e.reason {
                            SearchCancelReason::Superseded => 0,
                            SearchCancelReason::Cancelled => 1,
                            SearchCancelReason::Cleared => 2,
                        };
                        l.emit(
                            EPDF_SEARCH_EVENT_CANCELLED,
                            &EpdfSearchCancelled { operation_id: op.as_ptr(), reason },
                        )
                    }),
                    relay!(on_failed, |l, e| {
                        let (code, message, op) =
                            (c(&e.error.code), c(&e.error.message), c(&e.operation_id));
                        l.emit(
                            EPDF_SEARCH_EVENT_FAILED,
                            &EpdfSearchFailed {
                                code: code.as_ptr(),
                                message: message.as_ptr(),
                                operation_id: op.as_ptr(),
                            },
                        )
                    }),
                    relay!(on_active_hit_changed, |l, e| {
                        let index = whole(e.index, "the active hit index")?;
                        let owner = match &e.hit {
                            Some(hit) => Some(HitsOwner::build(std::slice::from_ref(hit))?),
                            None => None,
                        };
                        let view = owner.as_ref().map(|o| o.view(std::ptr::null_mut()));
                        let hits = view.as_ref().map_or(std::ptr::null(), |v| v as *const _);
                        l.emit(
                            EPDF_SEARCH_EVENT_ACTIVE_HIT_CHANGED,
                            &EpdfSearchActiveHitChanged { index, hits },
                        )
                    }),
                    relay!(on_cleared, |l, _e| {
                        (l.on_event)(l.ctx.0, EPDF_SEARCH_EVENT_CLEARED, std::ptr::null())
                    }),
                ];
                Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
            },
            move || (ending.on_event)(ending.ctx.0, EPDF_SEARCH_EVENT_END, std::ptr::null()),
        );
    });
    Box::into_raw(Box::new(EpdfSubscription(subscription)))
}

/// Cancel and free. Idempotent on the kernel side; call it once.
///
/// # Safety
/// `subscription` came from `epdf_search_subscribe`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn epdf_subscription_free(subscription: *mut EpdfSubscription) {
    if !subscription.is_null() {
        drop(unsafe { Box::from_raw(subscription) });
    }
}
