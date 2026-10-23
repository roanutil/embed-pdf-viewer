//! The selection plugin through uniffi: one handle over the selection, its
//! geometry and text extraction, and one event enum.
//!
//! Every record carries a kernel `PageRef` or an `epdf-geometry` quad, so
//! each is a mirror here. Character indices stay `i64`: they are PDFium
//! character indices (the space search hits address), not UTF-16 offsets
//! into extracted text. Selection is session state, so its events carry no
//! origin.
//!
//! Every method hops to the kernel thread through `KernelBound`.
//!
//! Not compiled until `epdf-ffi` builds again (Task 13.33): its `TextQuad`
//! still names `upper_start`..`lower_end`, so [`quad`] maps the named
//! corners onto it; 13.33 renames the mirror.

use std::sync::Arc;

use epdf_ffi::{
    CancelToken, KernelBound, KernelPost, PageRef, Point, Rect, Subscription, TextQuad, ViewerError,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::geometry::PageRef as KernelPageRef;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};

use crate::{OperationOptions, SelectionCapability};

/// A position in a page's character space.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextPosition {
    pub page: PageRef,
    /// A PDFium character index, not a UTF-16 offset.
    pub index: i64,
}

impl From<crate::TextPosition> for SelectionTextPosition {
    fn from(p: crate::TextPosition) -> Self {
        SelectionTextPosition { page: p.page.into(), index: p.index }
    }
}

impl From<SelectionTextPosition> for crate::TextPosition {
    fn from(p: SelectionTextPosition) -> Self {
        crate::TextPosition { page: p.page.into(), index: p.index }
    }
}

/// A half-open range in character space, in document order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionTextRange {
    pub start: SelectionTextPosition,
    pub end: SelectionTextPosition,
}

impl From<crate::TextRange> for SelectionTextRange {
    fn from(r: crate::TextRange) -> Self {
        SelectionTextRange { start: r.start.into(), end: r.end.into() }
    }
}

impl From<SelectionTextRange> for crate::TextRange {
    fn from(r: SelectionTextRange) -> Self {
        crate::TextRange { start: r.start.into(), end: r.end.into() }
    }
}

/// What `select` takes: a cross-page range, or a span on one page (a
/// search hit's shape: `char_start` and `char_count`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionInput {
    Range { range: SelectionTextRange },
    Span { page: PageRef, start: i64, count: i64 },
}

impl From<SelectionInput> for crate::SelectionRangeInput {
    fn from(i: SelectionInput) -> Self {
        match i {
            SelectionInput::Range { range } => crate::SelectionRangeInput::Range(range.into()),
            SelectionInput::Span { page, start, count } => {
                crate::SelectionRangeInput::Span { page: page.into(), start, count }
            }
        }
    }
}

/// One merged visual line of a selection, in page space.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSegment {
    pub quad: TextQuad,
    pub rect: Rect,
    /// Reading direction along the baseline: 1 or -1.
    pub advance: i32,
}

/// A quad's named corners on the mirror's.
fn quad(q: epdf_geometry::Quad) -> TextQuad {
    TextQuad {
        upper_start: q.upper_left.into(),
        upper_end: q.upper_right.into(),
        lower_start: q.lower_left.into(),
        lower_end: q.lower_right.into(),
    }
}

impl From<&crate::SelectionSegment> for SelectionSegment {
    fn from(s: &crate::SelectionSegment) -> Self {
        SelectionSegment { quad: quad(s.quad), rect: s.rect.into(), advance: s.advance }
    }
}

/// A selection boundary on its boundary glyph's own cell.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionEndpoint {
    pub page: PageRef,
    pub glyph_quad: TextQuad,
    pub advance: i32,
    pub rect: Rect,
}

impl From<&crate::SelectionEndpoint> for SelectionEndpoint {
    fn from(e: &crate::SelectionEndpoint) -> Self {
        SelectionEndpoint {
            page: e.page.into(),
            glyph_quad: quad(e.glyph_quad),
            advance: e.advance,
            rect: e.rect.into(),
        }
    }
}

/// Where selection-scoped floating UI attaches.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SelectionAnchor {
    pub page: PageRef,
    pub bounds: Rect,
}

impl From<crate::SelectionAnchor> for SelectionAnchor {
    fn from(a: crate::SelectionAnchor) -> Self {
        SelectionAnchor { page: a.page.into(), bounds: a.bounds.into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionDirection {
    Forward,
    Backward,
}

impl From<crate::SelectionDirection> for SelectionDirection {
    fn from(d: crate::SelectionDirection) -> Self {
        match d {
            crate::SelectionDirection::Forward => SelectionDirection::Forward,
            crate::SelectionDirection::Backward => SelectionDirection::Backward,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshotPage {
    pub page: PageRef,
    pub segments: Vec<SelectionSegment>,
}

/// Everything a selection overlay draws.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionSnapshot {
    pub pages: Vec<SelectionSnapshotPage>,
    pub start: Option<SelectionEndpoint>,
    pub end: Option<SelectionEndpoint>,
    pub direction: SelectionDirection,
    /// A valid `select` input; `None` with nothing selected.
    pub range: Option<SelectionTextRange>,
}

impl From<&crate::SelectionSnapshot> for SelectionSnapshot {
    fn from(s: &crate::SelectionSnapshot) -> Self {
        SelectionSnapshot {
            pages: s
                .pages
                .iter()
                .map(|p| SelectionSnapshotPage {
                    page: p.page.into(),
                    segments: p.segments.iter().map(Into::into).collect(),
                })
                .collect(),
            start: s.start.as_ref().map(Into::into),
            end: s.end.as_ref().map(Into::into),
            direction: s.direction.into(),
            range: s.range.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionChangedEvent {
    pub range: Option<SelectionTextRange>,
    /// The pages that carry a segment now.
    pub pages: Vec<PageRef>,
}

/// A selection gesture ended with a selection in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SelectionCommittedEvent {
    pub range: SelectionTextRange,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SelectionEvent {
    Changed { event: SelectionChangedEvent },
    Committed { event: SelectionCommittedEvent },
    Cleared,
}

#[uniffi::export(with_foreign)]
pub trait SelectionEventListener: Send + Sync {
    fn on_event(&self, event: SelectionEvent);
    /// The document closed: no event follows.
    fn on_end(&self);
}

fn open(cap: &SelectionCapability) -> Result<&SelectionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "selection",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

/// A signal for the plugin's options, and the abort to hang on the promise
/// it returns, so a cancelled `CancelToken` reaches the plugin.
fn abortable() -> (AbortSignal, impl Fn(Option<serde_json::Value>) + 'static) {
    let signal = AbortSignal::new();
    let aborter = signal.clone();
    (signal, move |reason| aborter.abort(Error::abort(reason)))
}

#[derive(uniffi::Object)]
pub struct SelectionHandle {
    cap: KernelBound<SelectionCapability>,
    post: KernelPost,
}

impl SelectionHandle {
    /// On the kernel thread.
    pub fn new(cap: SelectionCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(SelectionHandle { cap: KernelBound::new(cap, post.clone()), post })
    }

    async fn on<T: Send + 'static>(
        &self,
        f: impl FnOnce(&SelectionCapability) -> Result<T, Error> + Send + 'static,
    ) -> Result<T, ViewerError> {
        self.cap.call(move |c| f(open(c)?)).await
    }
}

#[uniffi::export]
impl SelectionHandle {
    /// Whether selection geometry is served (`doc.text.select`).
    pub async fn can_select(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.can_select())).await
    }

    /// Whether text extraction is served (`doc.text.copy`).
    pub async fn can_copy(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.can_copy())).await
    }

    pub async fn select(&self, input: SelectionInput) -> Result<(), ViewerError> {
        self.on(move |c| c.select(input.into())).await
    }

    pub async fn select_all(&self) -> Result<(), ViewerError> {
        self.on(|c| c.select_all()).await
    }

    pub async fn select_page(&self, page: PageRef) -> Result<(), ViewerError> {
        self.on(move |c| c.select_page(KernelPageRef::from(page))).await
    }

    /// False when the point has no selectable text.
    pub async fn select_word_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| c.select_word_at(KernelPageRef::from(page), point.into())).await
    }

    pub async fn select_line_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| c.select_line_at(KernelPageRef::from(page), point.into())).await
    }

    pub async fn extend_to(&self, page: PageRef, point: Point) -> Result<(), ViewerError> {
        self.on(move |c| c.extend_to(KernelPageRef::from(page), point.into())).await
    }

    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.clear();
            Ok(())
        })
        .await
    }

    pub async fn has_selection(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.has_selection())).await
    }

    pub async fn snapshot(&self) -> Result<SelectionSnapshot, ViewerError> {
        self.on(|c| Ok((&c.get_snapshot()).into())).await
    }

    pub async fn range(&self) -> Result<Option<SelectionTextRange>, ViewerError> {
        self.on(|c| Ok(c.get_range().map(Into::into))).await
    }

    pub async fn selected_pages(&self) -> Result<Vec<PageRef>, ViewerError> {
        self.on(|c| Ok(c.list_selected_pages().into_iter().map(Into::into).collect())).await
    }

    pub async fn segments(&self, page: PageRef) -> Result<Vec<SelectionSegment>, ViewerError> {
        self.on(move |c| {
            Ok(c.list_segments(KernelPageRef::from(page)).iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn rects(&self, page: PageRef) -> Result<Vec<Rect>, ViewerError> {
        self.on(move |c| {
            Ok(c.list_rects(KernelPageRef::from(page)).into_iter().map(Into::into).collect())
        })
        .await
    }

    pub async fn anchor(&self) -> Result<Option<SelectionAnchor>, ViewerError> {
        self.on(|c| Ok(c.get_anchor().map(Into::into))).await
    }

    /// The selected text, pages joined with `\n`; `""` with nothing
    /// selected. Rejects `permission-denied` without `doc.text.copy`.
    pub async fn read_text(&self, cancel: Arc<CancelToken>) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (signal, abort) = abortable();
                    Ok(open(c)?
                        .read_text(OperationOptions { signal: Some(signal) })
                        .with_abort(abort))
                },
                |text| text,
            )
            .await
    }

    /// Text for any range, without touching the selection.
    pub async fn read_text_in_range(
        &self,
        range: SelectionTextRange,
        cancel: Arc<CancelToken>,
    ) -> Result<String, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let (signal, abort) = abortable();
                    Ok(open(c)?
                        .read_text_in_range(range.into(), OperationOptions { signal: Some(signal) })
                        .with_abort(abort))
                },
                |text| text,
            )
            .await
    }

    /// Warm a page's text geometry.
    pub async fn ensure_loaded(&self, page: PageRef) -> Result<(), ViewerError> {
        self.cap.call_promise(move |c| Ok(open(c)?.ensure_loaded(page.into())), |()| ()).await
    }

    pub async fn is_loaded(&self, page: PageRef) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_loaded(page.into()))).await
    }

    pub async fn is_over_text(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.is_over_text(page.into(), point.into()))).await
    }

    /// A pointer gesture opened: `is_selecting` until `end_gesture`.
    pub async fn begin_gesture(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.begin_gesture();
            Ok(())
        })
        .await
    }

    /// Start a drag-select anchored at a point; false off text.
    pub async fn begin_gesture_at(&self, page: PageRef, point: Point) -> Result<bool, ViewerError> {
        self.on(move |c| Ok(c.begin_gesture_at(page.into(), point.into()))).await
    }

    pub async fn end_gesture(&self) -> Result<(), ViewerError> {
        self.on(|c| {
            c.end_gesture();
            Ok(())
        })
        .await
    }

    /// Whether a selecting gesture is in flight.
    pub async fn is_selecting(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_selecting())).await
    }

    pub async fn set_highlight_visible(&self, visible: bool) -> Result<(), ViewerError> {
        self.on(move |c| {
            c.set_highlight_visible(visible);
            Ok(())
        })
        .await
    }

    pub async fn is_highlight_visible(&self) -> Result<bool, ViewerError> {
        self.on(|c| Ok(c.is_highlight_visible())).await
    }

    /// The listener is registered on the kernel thread after this returns.
    /// It gets `on_end` when the document closes (at once if it already has).
    pub fn subscribe(&self, listener: Arc<dyn SelectionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let changed = {
                        let listener = listener.clone();
                        c.on_changed(move |e| {
                            listener.on_event(SelectionEvent::Changed {
                                event: SelectionChangedEvent {
                                    range: e.range.map(Into::into),
                                    pages: e.pages.iter().map(|p| (*p).into()).collect(),
                                },
                            });
                            Ok(())
                        })
                    };
                    let committed = {
                        let listener = listener.clone();
                        c.on_committed(move |e| {
                            listener.on_event(SelectionEvent::Committed {
                                event: SelectionCommittedEvent { range: e.range.into() },
                            });
                            Ok(())
                        })
                    };
                    let cleared = {
                        let listener = listener.clone();
                        c.on_cleared(move |_| {
                            listener.on_event(SelectionEvent::Cleared);
                            Ok(())
                        })
                    };
                    let offs: Vec<Unsubscribe> = vec![changed, committed, cleared];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
