//! The redaction plugin through uniffi: one handle over the two-stage
//! model (mark, then apply), typed records, one event enum. Every method
//! hops to the kernel thread through `KernelBound`.
//!
//! Not compiled until `epdf-ffi` builds again (Task 13.33). Since Task 13.23
//! the plugin carries annotation refs and apply results as the engine's
//! records (JSON), so the records here decode them; a value that does not
//! decode is an `invalid-state` error. Refs map onto `epdf_ffi::AnnotationRef`
//! as the annotation handle maps them (`objectNumber`, `baseIndex` as
//! `Index` with no revision); 13.33 may retype them against the renamed
//! mirror. An apply result's pages are `applied` or `unchanged`: the
//! engine's apply is all or nothing, so the retired `failed` and `skipped`
//! statuses and the per-page error are gone.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Rect, Subscription, ViewerError,
    to_u32,
};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_search::SearchQuery;
use serde_json::{Value, json};

use crate::{OperationOptions, PageTarget, RedactionCapability};

fn invalid(message: impl Into<String>) -> Error {
    Error::named("InvalidState", format!("[redaction] {}", message.into()))
        .with("code", "invalid-state")
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

/// The cancel token carries the cancel; the plugin call takes none.
fn none() -> OperationOptions {
    OperationOptions::default()
}

fn whole(value: Option<&Value>, what: &str) -> Result<u32, Error> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= u32::MAX as f64)
        .map(|n| n as u32)
        .ok_or_else(|| invalid(format!("{what} is not a whole number")))
}

fn page_in(page: PageRef) -> epdf_kernel::geometry::PageRef {
    epdf_kernel::geometry::PageRef::new(page.page_object_number)
}

fn page_out(page: &Value) -> Result<PageRef, Error> {
    Ok(PageRef { page_object_number: whole(page.get("objectNumber"), "a page's object number")? })
}

fn page_json(page: PageRef) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": page.page_object_number })
}

/// An `AnnotationRef` as the engine writes it: an object number or a base
/// index on its page.
fn ref_json(reference: &AnnotationRef) -> Result<Value, Error> {
    match reference {
        AnnotationRef::ObjectNumber { page, annot_object_number } => Ok(json!({
            "kind": "objectNumber", "page": page_json(*page), "objectNumber": annot_object_number
        })),
        AnnotationRef::Index { page, index } => Ok(json!({
            "kind": "baseIndex", "page": page_json(*page), "baseIndex": index
        })),
    }
}

fn refs_json(refs: &[AnnotationRef]) -> Result<Vec<Value>, Error> {
    refs.iter().map(ref_json).collect()
}

fn ref_out(reference: &Value) -> Result<AnnotationRef, Error> {
    let page = page_out(reference.get("page").unwrap_or(&Value::Null))?;
    match reference.get("kind").and_then(Value::as_str) {
        Some("objectNumber") => Ok(AnnotationRef::ObjectNumber {
            page,
            annot_object_number: whole(
                reference.get("objectNumber"),
                "an annotation's object number",
            )?,
        }),
        Some("baseIndex") => Ok(AnnotationRef::Index {
            page,
            index: whole(reference.get("baseIndex"), "an annotation's base index")?,
        }),
        _ => Err(invalid("a ref of a kind the engine does not write")),
    }
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

/// The overlay settings, any of them, over the defaults (black fill, white
/// Helvetica label fitted to the area). Every mark takes its look from
/// them.
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
            (c.fill.is_some() || c.overlay_text.is_some()).then(|| crate::RedactionOverlayConfig {
                fill: c.fill,
                text: c.overlay_text.map(|t| crate::RedactionOverlayTextConfig {
                    color: t.color,
                    font_family: t.font_family,
                    font_size: t.font_size,
                }),
            });
        crate::RedactionConfig { overlay }
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
    /// Whether the label tiles the area.
    pub repeat: bool,
}

impl TryFrom<&crate::RedactionMark> for RedactionMark {
    type Error = Error;

    fn try_from(m: &crate::RedactionMark) -> Result<Self, Error> {
        Ok(RedactionMark {
            annotation_ref: ref_out(&m.ref_)?,
            page: PageRef { page_object_number: m.page.page_object_number },
            page_index: u32::try_from(m.page_index).ok(),
            kind: match m.kind {
                crate::RedactionMarkKind::Area => RedactionMarkKind::Area,
                crate::RedactionMarkKind::Text => RedactionMarkKind::Text,
            },
            bounds: Rect {
                x: m.bounds.x,
                y: m.bounds.y,
                width: m.bounds.width,
                height: m.bounds.height,
            },
            overlay_text: m.overlay_text.clone(),
            repeat: m.repeat,
        })
    }
}

fn marks_out(list: &[crate::RedactionMark]) -> Result<Vec<RedactionMark>, Error> {
    list.iter().map(RedactionMark::try_from).collect()
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
            (Some(text), false) => Some(Some(text)),
            (None, true) => Some(None),
            (None, false) => None,
        };
        Ok(crate::RedactionLabelPatch { overlay_text, repeat: p.repeat })
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionSkipped {
    pub annotation_ref: AnnotationRef,
    pub reason: String,
}

/// A mark `unmark` could not remove, with the `PluginErrorInfo`'s members.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkFailure {
    pub annotation_ref: AnnotationRef,
    pub code: String,
    pub message: String,
    pub capability: String,
    pub permission: Option<String>,
}

/// What `unmark` and `clear_pending` did: marks removed, refs that weren't
/// pending marks, and marks that could not be removed.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionUnmarkResult {
    pub applied: Vec<AnnotationRef>,
    pub skipped: Vec<RedactionSkipped>,
    pub failed: Vec<RedactionUnmarkFailure>,
}

impl TryFrom<crate::BatchResult> for RedactionUnmarkResult {
    type Error = Error;

    fn try_from(batch: crate::BatchResult) -> Result<Self, Error> {
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
        Ok(RedactionUnmarkResult {
            applied: batch.applied.iter().map(ref_out).collect::<Result<_, _>>()?,
            skipped: batch
                .skipped
                .iter()
                .map(|s| {
                    Ok(RedactionSkipped {
                        annotation_ref: ref_out(&s.ref_)?,
                        reason: s.reason.clone(),
                    })
                })
                .collect::<Result<_, Error>>()?,
            failed: batch
                .failed
                .iter()
                .map(|f| {
                    Ok(RedactionUnmarkFailure {
                        annotation_ref: ref_out(&f.ref_)?,
                        code: text(&f.error, "code").unwrap_or_default(),
                        message: text(&f.error, "message").unwrap_or_default(),
                        capability: text(&f.error, "capability").unwrap_or_default(),
                        permission: text(&f.error, "permission"),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RedactionPageStatus {
    Applied,
    /// The page had no marks, which is not a failure.
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionPageResult {
    pub page: PageRef,
    pub status: RedactionPageStatus,
    /// Annotations removed alongside the marks (popups, detached widgets),
    /// not the marks themselves.
    pub removed_annotation_count: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RedactionApplyResult {
    pub scope: RedactionApplyScope,
    pub results: Vec<RedactionPageResult>,
    pub removed_annotation_count: u32,
    /// The mutation's operation id (`meta.opId`).
    pub op_id: Option<String>,
}

impl RedactionApplyResult {
    /// engine-core's `RedactionApplyResult`: `{scope, results,
    /// removedAnnotationCount, meta}`, the scope `{pages}` or
    /// `{annotations}`.
    pub(crate) fn from_json(value: &Value) -> Result<Self, Error> {
        let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
        let scope = value.get("scope").unwrap_or(&Value::Null);
        let scope = if let Some(pages) = scope.get("pages") {
            RedactionApplyScope::Pages {
                pages: list(Some(pages)).iter().map(page_out).collect::<Result<_, _>>()?,
            }
        } else if let Some(refs) = scope.get("annotations") {
            RedactionApplyScope::Annotations {
                refs: list(Some(refs)).iter().map(ref_out).collect::<Result<_, _>>()?,
            }
        } else {
            return Err(invalid("an apply scope names neither pages nor annotations"));
        };
        let results = list(value.get("results"))
            .iter()
            .map(|item| {
                Ok(RedactionPageResult {
                    page: page_out(item.get("page").unwrap_or(&Value::Null))?,
                    status: match item.get("status").and_then(Value::as_str) {
                        Some("applied") => RedactionPageStatus::Applied,
                        Some("unchanged") => RedactionPageStatus::Unchanged,
                        _ => return Err(invalid("a page status the engine does not write")),
                    },
                    removed_annotation_count: whole(
                        item.get("removedAnnotationCount"),
                        "a removed-annotation count",
                    )?,
                })
            })
            .collect::<Result<_, Error>>()?;
        Ok(RedactionApplyResult {
            scope,
            results,
            removed_annotation_count: whole(
                value.get("removedAnnotationCount"),
                "a removed-annotation count",
            )?,
            op_id: value
                .get("meta")
                .and_then(|m| m.get("opId"))
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }
}

fn applied(value: Rc<Value>) -> Result<RedactionApplyResult, ViewerError> {
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

fn page_target(page: Option<PageRef>) -> Option<PageTarget> {
    page.map(|p| PageTarget::Ref(page_in(p)))
}

#[uniffi::export]
impl RedactionHandle {
    /// Pending marks, or those on one page.
    pub async fn pending(&self, page: Option<PageRef>) -> Result<Vec<RedactionMark>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = crate::RedactionMarkFilter { page: page_target(page) };
                marks_out(&open(c)?.list_pending(Some(filter)))
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
                    .get_pending(&ref_json(&annotation_ref)?)
                    .as_ref()
                    .map(RedactionMark::try_from)
                    .transpose()
            })
            .await
    }

    pub async fn pending_count(&self, page: Option<PageRef>) -> Result<u32, ViewerError> {
        self.cap
            .call(move |c| to_u32(open(c)?.get_pending_count(page_target(page)), "a pending count"))
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
                let refs = refs.as_deref().map(refs_json).transpose()?;
                let estimate = open(c)?.estimate_collateral(refs.as_deref());
                Ok(RedactionCollateral {
                    count: to_u32(estimate.count, "a collateral count")?,
                    refs: estimate.refs.iter().map(ref_out).collect::<Result<_, _>>()?,
                })
            })
            .await
    }

    /// Mark the current text selection, one mark per page, and clear it.
    /// Empty without a selection; `unsupported` without the selection plugin.
    pub async fn mark_selection(
        &self,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<RedactionMark>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.mark_selection(none())),
                |r| marks_out(&r.marks).map_err(ViewerError::from),
            )
            .await?
    }

    /// Mark a page-space rectangle.
    pub async fn mark_area(
        &self,
        page: PageRef,
        bounds: Rect,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionMark, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let bounds = epdf_geometry::Rect {
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    };
                    Ok(open(c)?.mark_area(PageTarget::Ref(page_in(page)), bounds, none()))
                },
                |r| RedactionMark::try_from(&r.mark).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn mark_page(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionMark, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.mark_page(PageTarget::Ref(page_in(page)), none())),
                |r| RedactionMark::try_from(&r.mark).map_err(ViewerError::from),
            )
            .await?
    }

    /// Mark every hit of `query`, or only those on `pages`.
    pub async fn mark_matches(
        &self,
        query: SearchQuery,
        pages: Option<Vec<PageRef>>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<RedactionMark>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let pages =
                        pages.map(|p| p.into_iter().map(|p| PageTarget::Ref(page_in(p))).collect());
                    Ok(open(c)?.mark_matches(query, pages, none()))
                },
                |r| marks_out(&r.marks).map_err(ViewerError::from),
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
                move |c| Ok(open(c)?.unmark(refs_json(&refs)?, none())),
                |batch| RedactionUnmarkResult::try_from(batch).map_err(ViewerError::from),
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
                |c| Ok(open(c)?.clear_pending(none())),
                |batch| RedactionUnmarkResult::try_from(batch).map_err(ViewerError::from),
            )
            .await?
    }

    /// Edit a mark's label. The mark keeps its current styling.
    pub async fn update_label(
        &self,
        annotation_ref: AnnotationRef,
        patch: RedactionLabelPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionMark, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.update_label(ref_json(&annotation_ref)?, patch.try_into()?, none()))
                },
                |r| RedactionMark::try_from(&r.mark).map_err(ViewerError::from),
            )
            .await?
    }

    /// Apply these pending marks. Irreversible.
    pub async fn apply(
        &self,
        refs: Vec<AnnotationRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<RedactionApplyResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.apply(&refs_json(&refs)?, none())),
                applied,
            )
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
                    let pages: Vec<PageTarget> =
                        pages.into_iter().map(|p| PageTarget::Ref(page_in(p))).collect();
                    Ok(open(c)?.apply_pages(&pages, none()))
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
        self.cap
            .call_promise_cancellable(cancel, |c| Ok(open(c)?.apply_all(none())), applied)
            .await?
    }

    pub async fn can_mark(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_mark())).await
    }

    pub async fn can_unmark(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_unmark(&ref_json(&annotation_ref)?))).await
    }

    pub async fn can_update_label(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_update_label(&ref_json(&annotation_ref)?))).await
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
                open(c)?.get_last_result().map(|r| RedactionApplyResult::from_json(&r)).transpose()
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
                            pages: e.pages.iter().map(page_out).collect::<Result<_, _>>()?,
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
