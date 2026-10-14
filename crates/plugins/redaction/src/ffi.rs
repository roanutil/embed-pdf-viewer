//! The redaction plugin through uniffi: one handle over the two-stage
//! model (mark, then apply), typed records, one event enum. Every method
//! hops to the kernel thread through `KernelBound`.

use std::sync::Arc;

use epdf_annotation_core as core;
use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Rect, Subscription, ViewerError,
    to_u32,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_search::SearchQuery;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::RedactionCapability;

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named("InvalidState", format!("[redaction] a value does not decode: {e}"))
            .with("code", "invalid-state")
    })
}

fn open(cap: &RedactionCapability) -> Result<&RedactionCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "redaction",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

fn ref_out(r: core::AnnotationRef) -> Result<AnnotationRef, Error> {
    AnnotationRef::try_from(r)
}

fn refs_out(refs: Vec<core::AnnotationRef>) -> Result<Vec<AnnotationRef>, Error> {
    refs.into_iter().map(ref_out).collect()
}

fn refs_in(refs: Vec<AnnotationRef>) -> Vec<core::AnnotationRef> {
    refs.into_iter().map(Into::into).collect()
}

fn json_ref(value: &Value) -> Result<AnnotationRef, Error> {
    ref_out(decode(value.clone())?)
}

// ── config ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionOverlayText {
    /// CSS hex colour of the label. White when `None`.
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub font_family: Option<String>,
    /// Points; 0 fits the label to the region.
    #[uniffi(default = None)]
    pub font_size: Option<f64>,
}

/// Defaults for marks this plugin creates.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionConfig {
    /// CSS hex colour painted over the region on apply. Black when `None`.
    #[uniffi(default = None)]
    pub fill: Option<String>,
    #[uniffi(default = None)]
    pub overlay_text: Option<RedactionOverlayText>,
}

impl From<RedactionConfig> for crate::RedactionConfig {
    fn from(c: RedactionConfig) -> Self {
        let overlay =
            (c.fill.is_some() || c.overlay_text.is_some()).then(|| crate::RedactionOverlay {
                fill: c.fill,
                text: c.overlay_text.map(|t| crate::RedactionOverlayText {
                    color: t.color,
                    font_family: t.font_family,
                    font_size: t.font_size,
                }),
            });
        crate::RedactionConfig { overlay, ..Default::default() }
    }
}

// ── marks ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RedactionMarkKind {
    /// A rectangle (a marquee, a whole page).
    Area,
    /// Per-line quads over text (a selection, a search hit).
    Text,
}

/// One pending mark: a live projection of a `redact` annotation. Delete the
/// annotation and the mark is gone.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionMark {
    pub annotation_ref: AnnotationRef,
    pub page: PageRef,
    /// Display index; `None` when the page registry lacks the page.
    pub page_index: Option<u32>,
    pub kind: RedactionMarkKind,
    /// Page space.
    pub bounds: Rect,
    /// `/OverlayText`, when set.
    pub overlay_text: Option<String>,
}

impl TryFrom<crate::RedactionMark> for RedactionMark {
    type Error = Error;

    fn try_from(m: crate::RedactionMark) -> Result<Self, Error> {
        Ok(RedactionMark {
            annotation_ref: ref_out(m.ref_)?,
            page: m.page.into(),
            page_index: u32::try_from(m.page_index).ok(),
            kind: match m.kind {
                crate::RedactionMarkKind::Area => RedactionMarkKind::Area,
                crate::RedactionMarkKind::Text => RedactionMarkKind::Text,
            },
            bounds: m.bounds.into(),
            overlay_text: m.overlay_text.as_ref().and_then(Value::as_str).map(str::to_string),
        })
    }
}

fn marks_out(list: Vec<crate::RedactionMark>) -> Result<Vec<RedactionMark>, Error> {
    list.into_iter().map(RedactionMark::try_from).collect()
}

/// What an apply would destroy beyond the marks: a client-side estimate.
/// The apply result is authoritative.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionCollateral {
    pub count: u32,
    pub refs: Vec<AnnotationRef>,
}

/// A label edit. `overlay_text` sets the label, `clear_overlay_text` removes
/// it (setting both is refused), and `repeat` tiles it across the region.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct RedactionLabelPatch {
    #[uniffi(default = None)]
    pub overlay_text: Option<String>,
    #[uniffi(default = false)]
    pub clear_overlay_text: bool,
    #[uniffi(default = None)]
    pub repeat: Option<bool>,
}

impl TryFrom<RedactionLabelPatch> for crate::RedactionLabelPatch {
    type Error = Error;

    fn try_from(p: RedactionLabelPatch) -> Result<Self, Error> {
        use core::js::Nullable;
        let overlay_text = match (p.overlay_text, p.clear_overlay_text) {
            (Some(_), true) => {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    "redaction",
                    "a label patch both sets and clears the overlay text",
                    None,
                    None,
                ));
            }
            (Some(text), false) => Nullable::Value(text),
            (None, true) => Nullable::Null,
            (None, false) => Nullable::Undefined,
        };
        Ok(crate::RedactionLabelPatch { overlay_text, repeat: p.repeat })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionSkipped {
    pub annotation_ref: AnnotationRef,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkFailure {
    pub annotation_ref: AnnotationRef,
    pub message: String,
}

/// What `unmark` and `clear_pending` did: marks removed, refs that weren't
/// pending marks, and deletes that failed.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkResult {
    pub applied: Vec<AnnotationRef>,
    pub skipped: Vec<RedactionSkipped>,
    pub failed: Vec<RedactionUnmarkFailure>,
}

impl RedactionUnmarkResult {
    fn from_json(value: &Value) -> Result<Self, Error> {
        let list =
            |key: &str| value.get(key).and_then(Value::as_array).cloned().unwrap_or_default();
        Ok(RedactionUnmarkResult {
            applied: list("applied").iter().map(json_ref).collect::<Result<_, _>>()?,
            skipped: list("skipped")
                .iter()
                .map(|s| {
                    Ok(RedactionSkipped {
                        annotation_ref: json_ref(&s["ref"])?,
                        reason: s.get("reason").and_then(Value::as_str).unwrap_or_default().into(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            failed: list("failed")
                .iter()
                .map(|f| {
                    Ok(RedactionUnmarkFailure {
                        annotation_ref: json_ref(&f["ref"])?,
                        message: f["error"]
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                    })
                })
                .collect::<Result<_, Error>>()?,
        })
    }
}

// ── apply ────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RedactionApplyScope {
    /// Every mark on each page.
    Pages { pages: Vec<PageRef> },
    /// Exactly these marks.
    Annotations { refs: Vec<AnnotationRef> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "camelCase")]
pub enum RedactionPageStatus {
    Applied,
    /// The page had no marks, which is not a failure.
    Unchanged,
    Failed,
    /// Not attempted: an earlier page failed.
    Skipped,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionPageResult {
    pub page: PageRef,
    pub status: RedactionPageStatus,
    /// Annotations removed alongside the marks (popups, detached widgets),
    /// not the marks themselves.
    pub removed_annotation_count: u32,
    /// Why the page failed: the engine error's message.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionApplyResult {
    pub scope: RedactionApplyScope,
    pub results: Vec<RedactionPageResult>,
    pub removed_annotation_count: u32,
}

impl RedactionApplyResult {
    /// The kernel's spelling (`engine-core`'s `RedactionApplyResult`), which
    /// `translate.rs` turns the host's answer into. A remote apply hands the
    /// plugin the whole `redaction.applied` event, which spreads the result's
    /// members beside `type` and `origin`, so it decodes the same way.
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "camelCase")]
        enum Scope {
            Pages { pages: Vec<core::PageRef> },
            Annotations { refs: Vec<core::AnnotationRef> },
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Item {
            page: core::PageRef,
            status: RedactionPageStatus,
            removed_annotation_count: u32,
            /// engine-core's `SerializedEngineError`, or the native host's
            /// bare message.
            #[serde(default)]
            error: Option<Value>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Dto {
            scope: Scope,
            results: Vec<Item>,
            removed_annotation_count: u32,
        }
        let dto: Dto = decode(value.clone())?;
        Ok(RedactionApplyResult {
            scope: match dto.scope {
                Scope::Pages { pages } => RedactionApplyScope::Pages {
                    pages: pages.into_iter().map(Into::into).collect(),
                },
                Scope::Annotations { refs } => {
                    RedactionApplyScope::Annotations { refs: refs_out(refs)? }
                }
            },
            results: dto
                .results
                .into_iter()
                .map(|i| RedactionPageResult {
                    page: i.page.into(),
                    status: i.status,
                    removed_annotation_count: i.removed_annotation_count,
                    error: i.error.and_then(|e| match e {
                        Value::String(message) => Some(message),
                        Value::Null => None,
                        other => Some(
                            other
                                .get("message")
                                .and_then(Value::as_str)
                                .map_or_else(|| other.to_string(), str::to_string),
                        ),
                    }),
                })
                .collect(),
            removed_annotation_count: dto.removed_annotation_count,
        })
    }
}

fn applied(value: Value) -> Result<RedactionApplyResult, ViewerError> {
    RedactionApplyResult::from_json(&value).map_err(Into::into)
}

// ── events ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum RedactionEvent {
    /// A confirmed apply, this session's or a remote one.
    Applied { result: RedactionApplyResult },
    /// Marks were created, changed or removed on these pages.
    PendingChanged { pages: Vec<PageRef> },
}

#[uniffi::export(with_foreign)]
pub trait RedactionEventListener: Send + Sync {
    fn on_event(&self, event: RedactionEvent);
    /// The document closed: no event follows. Called once, and at once for
    /// a subscription made after the close.
    fn on_end(&self);
}

// ── the handle ───────────────────────────────────────────────────────────────

#[derive(uniffi::Object)]
pub struct RedactionHandle {
    cap: KernelBound<RedactionCapability>,
    post: KernelPost,
}

impl RedactionHandle {
    /// On the kernel thread.
    pub fn new(cap: RedactionCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(RedactionHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl RedactionHandle {
    /// Pending marks, or those on one page.
    pub async fn pending(&self, page: Option<PageRef>) -> Result<Vec<RedactionMark>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = crate::RedactionMarkFilter { page: page.map(Into::into) };
                marks_out(open(c)?.list_pending(Some(&filter)))
            })
            .await
    }

    pub async fn pending_item(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<Option<RedactionMark>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?
                    .get_pending(&annotation_ref.into())
                    .map(RedactionMark::try_from)
                    .transpose()
            })
            .await
    }

    pub async fn pending_count(&self, page: Option<PageRef>) -> Result<u32, ViewerError> {
        self.cap
            .call(move |c| {
                let page = page.map(core::PageRef::from);
                to_u32(open(c)?.get_pending_count(page.as_ref()), "a pending count")
            })
            .await
    }

    /// What applying `refs` (every pending mark when `None`) would also
    /// remove.
    pub async fn estimate_collateral(
        &self,
        refs: Option<Vec<AnnotationRef>>,
    ) -> Result<RedactionCollateral, ViewerError> {
        self.cap
            .call(move |c| {
                let refs = refs.map(refs_in);
                let estimate = open(c)?.estimate_collateral(refs.as_deref());
                Ok(RedactionCollateral {
                    count: to_u32(estimate.count, "a collateral count")?,
                    refs: estimate.refs.iter().map(json_ref).collect::<Result<_, _>>()?,
                })
            })
            .await
    }

    /// Mark the current text selection, one mark per page, and clear it.
    /// Empty without a selection; `unsupported` without the selection plugin.
    pub async fn mark_selection(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.mark_selection()),
                |refs| refs_out(refs).map_err(ViewerError::from),
            )
            .await?
    }

    /// Mark a page-space rectangle.
    pub async fn mark_area(
        &self,
        page: PageRef,
        bounds: Rect,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let bounds = json!({ "x": bounds.x, "y": bounds.y, "width": bounds.width, "height": bounds.height });
                    open(c)?.mark_area(&page.into(), bounds)
                },
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn mark_page(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| open(c)?.mark_page(&page.into()),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    /// Mark every hit of `query`, or only those on `pages`.
    pub async fn mark_matches(
        &self,
        query: SearchQuery,
        pages: Option<Vec<PageRef>>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<AnnotationRef>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.mark_matches(
                        query,
                        pages.map(|p| p.into_iter().map(Into::into).collect()),
                    ))
                },
                |refs| refs_out(refs).map_err(ViewerError::from),
            )
            .await?
    }

    /// Remove marks. A ref that isn't a pending mark is skipped, not failed.
    pub async fn unmark(
        &self,
        refs: Vec<AnnotationRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionUnmarkResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.unmark(refs_in(refs))),
                |v| RedactionUnmarkResult::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn clear_pending(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionUnmarkResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.clear_pending()),
                |v| RedactionUnmarkResult::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Edit a mark's label. The mark keeps its current styling.
    pub async fn update_label(
        &self,
        annotation_ref: AnnotationRef,
        patch: RedactionLabelPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update_label(annotation_ref.into(), patch.try_into()?)),
                |()| (),
            )
            .await
    }

    /// Apply these pending marks. Irreversible.
    pub async fn apply(
        &self,
        refs: Vec<AnnotationRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionApplyResult, ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.apply(&refs_in(refs))), applied)
            .await?
    }

    /// Apply every mark on these pages. Irreversible.
    pub async fn apply_pages(
        &self,
        pages: Vec<PageRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionApplyResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let pages: Vec<core::PageRef> = pages.into_iter().map(Into::into).collect();
                    Ok(open(c)?.apply_pages(&pages))
                },
                applied,
            )
            .await?
    }

    /// Apply every redaction in the document, including marks on pages this
    /// session never loaded. Irreversible.
    pub async fn apply_all(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionApplyResult, ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.apply_all()), applied).await?
    }

    pub async fn can_mark(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_mark())).await
    }

    /// Whether the engine can apply and the session holds every capability
    /// the apply asserts.
    pub async fn can_apply(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_apply())).await
    }

    pub async fn is_applying(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_applying())).await
    }

    pub async fn last_result(&self) -> Result<Option<RedactionApplyResult>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.get_last_result().as_ref().map(RedactionApplyResult::from_json).transpose()
            })
            .await
    }

    pub fn subscribe(&self, listener: Arc<dyn RedactionEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let applied = c.on_applied(move |e| {
                        l.on_event(RedactionEvent::Applied {
                            result: RedactionApplyResult::from_json(&e.result)?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let pending = c.on_pending_changed(move |e| {
                        l.on_event(RedactionEvent::PendingChanged {
                            pages: e.pages.iter().map(|p| (*p).into()).collect(),
                        });
                        Ok(())
                    });
                    Unsubscribe::new(move || {
                        applied.call();
                        pending.call();
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
