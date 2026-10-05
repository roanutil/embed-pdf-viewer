//! The search plugin through uniffi: typed records, one handle, one event
//! enum. Every method hops to the kernel thread through `KernelBound`.
//!
//! The records the plugin's own types can carry derive in place
//! (`SearchQuery`, `SearchMode`, `SearchStatus`, `SearchProgress`,
//! `SearchSnippet`, `SearchResultStatus`, `SearchCancelReason`, and the
//! started and cancelled events) and are re-exported here. The rest are
//! mirrors, each for a reason uniffi cannot carry: `usize` counts and
//! indices (`SearchHit`, `SearchResult`, the progress, completed and
//! active-hit events), the Stage's reveal types and their two-level options
//! (`SearchRevealOptions`, `SearchConfig`), and `TextSegment`, whose quad and
//! rect are `epdf-geometry` types. A count that does not fit a `u32` is an
//! `invalid-state` error, never a clamp.

use std::sync::Arc;

use epdf_ffi::{
    CancelToken, KernelBound, KernelPost, PageRef, PluginErrorInfo, Rect, Subscription, TextQuad,
    ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_stage::{RevealAnchor, RevealAnchorValue, RevealWord, ScrollBehaviorKind};

use crate::{OperationOptions, SearchCapability, SearchFindAllOptions, SearchOptions};
pub use crate::{
    SearchCancelReason, SearchCancelledEvent, SearchMode, SearchProgress, SearchQuery,
    SearchResultStatus, SearchSnippet, SearchStartedEvent, SearchStatus,
};

/// One merged visual line of a match, in page space.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// The reading direction along the baseline: `1` or `-1`.
    pub advance: f64,
}

impl From<&crate::TextSegment> for TextSegment {
    fn from(segment: &crate::TextSegment) -> Self {
        TextSegment {
            quad: segment.quad.into(),
            rect: segment.rect.into(),
            advance: segment.advance,
        }
    }
}

/// One match. `page` is the durable identity, `page_index` its display index
/// at the time of the match.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchHit {
    pub page: PageRef,
    pub page_index: u32,
    /// Engine text-page character offsets (PDFium character indices, the
    /// space geometry and selection use), not UTF-16 offsets into extracted
    /// text. `snippet`'s offsets are UTF-16.
    pub char_start: f64,
    pub char_count: f64,
    pub segments: Vec<TextSegment>,
    /// The union of the segments; absent when the match has none.
    pub bounds: Option<Rect>,
    /// Context around the match, `SearchMode::Full` only.
    pub snippet: Option<SearchSnippet>,
}

impl TryFrom<&crate::SearchHit> for SearchHit {
    type Error = Error;

    fn try_from(hit: &crate::SearchHit) -> Result<Self, Error> {
        Ok(SearchHit {
            page: hit.page.into(),
            page_index: whole(hit.page_index, "a hit's page index")?,
            char_start: hit.char_start,
            char_count: hit.char_count,
            segments: hit.segments.iter().map(TextSegment::from).collect(),
            bounds: hit.bounds.map(Rect::from),
            snippet: hit.snippet.clone(),
        })
    }
}

fn hits(hits: &[crate::SearchHit]) -> Result<Vec<SearchHit>, Error> {
    hits.iter().map(SearchHit::try_from).collect()
}

/// What a `search` resolved to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SearchResult {
    pub status: SearchResultStatus,
    pub hit_count: u32,
}

impl TryFrom<crate::SearchResult> for SearchResult {
    type Error = Error;

    fn try_from(result: crate::SearchResult) -> Result<Self, Error> {
        Ok(SearchResult {
            status: result.status,
            hit_count: whole(result.hit_count, "a hit count")?,
        })
    }
}

/// One axis of a reveal arrival: a word, or a viewport fraction.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum SearchRevealAxis {
    Keep,
    Start,
    Center,
    End,
    Fraction { value: f64 },
}

impl From<SearchRevealAxis> for RevealAnchorValue {
    fn from(axis: SearchRevealAxis) -> Self {
        match axis {
            SearchRevealAxis::Keep => RevealAnchorValue::Word(RevealWord::Keep),
            SearchRevealAxis::Start => RevealAnchorValue::Word(RevealWord::Start),
            SearchRevealAxis::Center => RevealAnchorValue::Word(RevealWord::Center),
            SearchRevealAxis::End => RevealAnchorValue::Word(RevealWord::End),
            SearchRevealAxis::Fraction { value } => RevealAnchorValue::Fraction(value),
        }
    }
}

/// Where a revealed hit lands in the viewport, per axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchRevealAnchor {
    #[uniffi(default = None)]
    pub x: Option<SearchRevealAxis>,
    #[uniffi(default = None)]
    pub y: Option<SearchRevealAxis>,
}

impl From<SearchRevealAnchor> for RevealAnchor {
    fn from(anchor: SearchRevealAnchor) -> Self {
        RevealAnchor { x: anchor.x.map(Into::into), y: anchor.y.map(Into::into) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SearchScrollBehavior {
    Smooth,
    Instant,
}

impl From<SearchScrollBehavior> for ScrollBehaviorKind {
    fn from(behavior: SearchScrollBehavior) -> Self {
        match behavior {
            SearchScrollBehavior::Smooth => ScrollBehaviorKind::Smooth,
            SearchScrollBehavior::Instant => ScrollBehaviorKind::Instant,
        }
    }
}

/// How a navigated hit arrives, when a Stage is installed. Merged per field
/// over `SearchConfig.reveal` over the find-bar default (the hit at 35% from
/// the top, scrolled smoothly); a `None` field leaves the layer below.
#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchRevealOptions {
    #[uniffi(default = None)]
    pub anchor: Option<SearchRevealAnchor>,
    #[uniffi(default = None)]
    pub behavior: Option<SearchScrollBehavior>,
}

impl From<SearchRevealOptions> for crate::SearchRevealOptions {
    fn from(options: SearchRevealOptions) -> Self {
        crate::SearchRevealOptions {
            anchor: options.anchor.map(|anchor| Some(anchor.into())),
            behavior: options.behavior.map(|behavior| Some(behavior.into())),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, uniffi::Record)]
pub struct SearchConfig {
    /// Arrival defaults for `next_hit`, `previous_hit` and `go_to_hit`.
    #[uniffi(default = None)]
    pub reveal: Option<SearchRevealOptions>,
}

impl From<SearchConfig> for crate::SearchConfig {
    fn from(config: SearchConfig) -> Self {
        crate::SearchConfig { reveal: config.reveal.map(Into::into) }
    }
}

// ── events ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchProgressEvent {
    pub scanned: f64,
    pub total: f64,
    pub hit_count: u32,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SearchCompletedEvent {
    pub hit_count: u32,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SearchFailedEvent {
    pub error: PluginErrorInfo,
    pub operation_id: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SearchActiveHitChangedEvent {
    pub index: u32,
    pub hit: Option<SearchHit>,
}

/// One session event. `Cleared` carries nothing (the plugin's event is an
/// empty object).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SearchEvent {
    Started { event: SearchStartedEvent },
    Progress { event: SearchProgressEvent },
    Completed { event: SearchCompletedEvent },
    Cancelled { event: SearchCancelledEvent },
    Failed { event: SearchFailedEvent },
    ActiveHitChanged { event: SearchActiveHitChangedEvent },
    Cleared,
}

#[uniffi::export(with_foreign)]
pub trait SearchEventListener: Send + Sync {
    fn on_event(&self, event: SearchEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

fn error_info(info: crate::PluginErrorInfo) -> PluginErrorInfo {
    PluginErrorInfo { code: info.code, message: info.message, capability: info.capability }
}

/// A count or index as a `u32`, or `invalid-state`. Shared with the
/// experimental manual bridge (`crates/epdf-manual-bridge`).
#[doc(hidden)]
pub fn whole(n: usize, what: &str) -> Result<u32, Error> {
    u32::try_from(n).map_err(|_| {
        Error::named("InvalidState", format!("[search] {what} ({n}) does not fit a u32"))
            .with("code", "invalid-state")
    })
}

/// The capability, if its document is still open. The plugin keeps
/// answering from its last state after a close; a foreign handle that
/// outlived the close gets `instance-closed` instead. Shared with the
/// experimental manual bridge.
#[doc(hidden)]
pub fn open(cap: &SearchCapability) -> Result<&SearchCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "search",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// A signal for the plugin's options, and the abort to hang on the promise
/// it returns, so a cancelled `CancelToken` reaches the plugin's own
/// cancellation path.
fn abortable() -> (AbortSignal, impl Fn(Option<serde_json::Value>) + 'static) {
    let signal = AbortSignal::new();
    let aborter = signal.clone();
    (signal, move |reason| aborter.abort(Error::abort(reason)))
}

/// `search`'s kernel-thread half: what both bridges start, so they do the
/// same plugin-side work. Shared with the experimental manual bridge.
#[doc(hidden)]
pub fn start_search(
    cap: &SearchCapability,
    query: SearchQuery,
    start_page: Option<PageRef>,
) -> Result<epdf_kernel::Promise<crate::SearchResult>, Error> {
    let (signal, abort) = abortable();
    let options = SearchOptions { start_page: start_page.map(Into::into), signal: Some(signal) };
    Ok(open(cap)?.search(query, options).with_abort(abort))
}

/// `find_all`'s kernel-thread half. Shared with the experimental manual
/// bridge.
#[doc(hidden)]
pub fn start_find_all(
    cap: &SearchCapability,
    query: SearchQuery,
    mode: Option<SearchMode>,
) -> Result<epdf_kernel::Promise<Vec<crate::SearchHit>>, Error> {
    let (signal, abort) = abortable();
    let options = SearchFindAllOptions { signal: Some(signal), mode };
    Ok(open(cap)?.find_all(query, options).with_abort(abort))
}

#[derive(uniffi::Object)]
pub struct SearchHandle {
    cap: KernelBound<SearchCapability>,
    post: KernelPost,
}

impl SearchHandle {
    /// On the kernel thread.
    pub fn new(cap: SearchCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(SearchHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl SearchHandle {
    /// Would a search be served now. `None` or `Rects` needs
    /// `doc.text.search`; `Full` also `doc.text.copy`.
    pub async fn can_search(&self, mode: Option<SearchMode>) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_search(mode))).await
    }

    /// Start a new search, superseding a running one. Hits stream into the
    /// session and the first becomes active. Resolves when the scan
    /// completes, is superseded or is cancelled (cancelling keeps the hits
    /// found so far); rejects only on a real failure. An empty `query.text`
    /// clears the session.
    pub async fn search(
        &self,
        query: SearchQuery,
        start_page: Option<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<SearchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| start_search(c, query, start_page),
                |result| SearchResult::try_from(result).map_err(Into::into),
            )
            .await?
    }

    /// Re-run the current query from scratch. The plugin does this itself
    /// after document mutations.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<SearchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (signal, abort) = abortable();
                    Ok(open(c)?
                        .refresh(OperationOptions { signal: Some(signal) })
                        .with_abort(abort))
                },
                |result| SearchResult::try_from(result).map_err(Into::into),
            )
            .await?
    }

    /// Stop the scan and keep the hits found so far.
    pub async fn cancel(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.cancel();
                Ok(())
            })
            .await
    }

    /// Stop the scan and drop the session.
    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.clear();
                Ok(())
            })
            .await
    }

    /// Step to the next hit, wrapping, and reveal it.
    pub async fn next_hit(
        &self,
        reveal: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .next_hit(reveal.map(Into::into))?
                    .as_ref()
                    .map(SearchHit::try_from)
                    .transpose()
            })
            .await
    }

    /// Step to the previous hit, wrapping, and reveal it.
    pub async fn previous_hit(
        &self,
        reveal: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .previous_hit(reveal.map(Into::into))?
                    .as_ref()
                    .map(SearchHit::try_from)
                    .transpose()
            })
            .await
    }

    /// Jump to a hit by index (wrapped modulo the hit count) and reveal it.
    pub async fn go_to_hit(
        &self,
        index: u32,
        reveal: Option<SearchRevealOptions>,
    ) -> Result<Option<SearchHit>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .go_to_hit(index as isize, reveal.map(Into::into))?
                    .as_ref()
                    .map(SearchHit::try_from)
                    .transpose()
            })
            .await
    }

    /// Bring the active hit back into view without changing it.
    pub async fn reveal_active_hit(
        &self,
        reveal: Option<SearchRevealOptions>,
    ) -> Result<(), ViewerError> {
        self.cap.call(move |c| open(c)?.reveal_active_hit(reveal.map(Into::into))).await
    }

    pub async fn query(&self) -> Result<Option<SearchQuery>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_query())).await
    }

    pub async fn status(&self) -> Result<SearchStatus, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_status())).await
    }

    /// Hits found so far, or those on one page.
    pub async fn hits(&self, page: Option<PageRef>) -> Result<Vec<SearchHit>, ViewerError> {
        self.cap.call(move |c| hits(&open(c)?.list_hits(page.map(Into::into)))).await
    }

    /// How many hits so far, or on one page.
    pub async fn hit_count(&self, page: Option<PageRef>) -> Result<u32, ViewerError> {
        self.cap
            .call(move |c| whole(open(c)?.get_hit_count(page.map(Into::into)), "a hit count"))
            .await
    }

    /// The pages with at least one hit, in document order.
    pub async fn pages_with_hits(&self) -> Result<Vec<PageRef>, ViewerError> {
        self.cap
            .call(|c| Ok(open(c)?.list_pages_with_hits().into_iter().map(PageRef::from).collect()))
            .await
    }

    /// `None` when no hit is active.
    pub async fn active_hit_index(&self) -> Result<Option<u32>, ViewerError> {
        self.cap
            .call(|c| match usize::try_from(open(c)?.get_active_hit_index()) {
                Ok(index) => whole(index, "the active hit index").map(Some),
                Err(_) => Ok(None),
            })
            .await
    }

    pub async fn active_hit(&self) -> Result<Option<SearchHit>, ViewerError> {
        self.cap
            .call(|c| open(c)?.get_active_hit().as_ref().map(SearchHit::try_from).transpose())
            .await
    }

    pub async fn progress(&self) -> Result<SearchProgress, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_progress())).await
    }

    /// The failure that ended the last search, if it failed.
    pub async fn error(&self) -> Result<Option<PluginErrorInfo>, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_error().map(error_info))).await
    }

    /// Run a query to completion and return every hit, in page order,
    /// touching no session state. `mode: None` is `Full`, falling back to
    /// `Rects` when snippets are denied. Cancelling rejects
    /// `operation-cancelled`.
    pub async fn find_all(
        &self,
        query: SearchQuery,
        mode: Option<SearchMode>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<SearchHit>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| start_find_all(c, query, mode),
                |found| hits(&found).map_err(Into::into),
            )
            .await?
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets `on_end` when the
    /// document closes (at once if it already has). An event that does not
    /// convert is reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn SearchEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    macro_rules! relay {
                        ($hook:ident, |$e:ident| $event:expr) => {{
                            let listener = listener.clone();
                            c.$hook(move |$e| {
                                listener.on_event($event);
                                Ok(())
                            })
                        }};
                    }
                    let offs: Vec<Unsubscribe> = vec![
                        relay!(on_started, |e| SearchEvent::Started { event: e.clone() }),
                        relay!(on_progress, |e| SearchEvent::Progress {
                            event: SearchProgressEvent {
                                scanned: e.scanned,
                                total: e.total,
                                hit_count: whole(e.hit_count, "a hit count")?,
                                operation_id: e.operation_id.clone(),
                            },
                        }),
                        relay!(on_completed, |e| SearchEvent::Completed {
                            event: SearchCompletedEvent {
                                hit_count: whole(e.hit_count, "a hit count")?,
                                operation_id: e.operation_id.clone(),
                            },
                        }),
                        relay!(on_cancelled, |e| SearchEvent::Cancelled { event: e.clone() }),
                        relay!(on_failed, |e| SearchEvent::Failed {
                            event: SearchFailedEvent {
                                error: error_info(e.error.clone()),
                                operation_id: e.operation_id.clone(),
                            },
                        }),
                        relay!(on_active_hit_changed, |e| SearchEvent::ActiveHitChanged {
                            event: SearchActiveHitChangedEvent {
                                index: whole(e.index, "the active hit index")?,
                                hit: e.hit.as_ref().map(SearchHit::try_from).transpose()?,
                            },
                        }),
                        relay!(on_cleared, |_e| SearchEvent::Cleared),
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
