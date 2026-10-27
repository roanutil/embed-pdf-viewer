use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, ResourceStatus,
    Subscription, ViewerError,
};
use epdf_kernel::Unsubscribe;
use serde_json::Value;

use super::{
    AnnotationChangedEvent, AnnotationDeletedEvent, AnnotationEvent, AnnotationEventListener,
    AnnotationFilter, AnnotationRecord, AnnotationResyncedPages, AnnotationThreadChange, draft_out,
    geometry_point, object_of, open, page_json, page_out, plugin_error_info_of, record_out,
    records_out, ref_json, ref_out, refs_json, refs_out,
};
use crate::AnnotationCapability;
use crate::write::crud::CreateOptions;

#[derive(uniffi::Object)]
pub struct AnnotationHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
    pub(crate) post: KernelPost,
}

impl AnnotationHandle {
    /// On the kernel thread.
    pub fn new(cap: AnnotationCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(AnnotationHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl AnnotationHandle {
    /// Install the host's file picker, or `None` to remove it. Without one,
    /// a tool that prompts for a file declines the click.
    pub async fn set_file_picker(
        &self,
        picker: Option<Arc<dyn super::AnnotationFilePicker>>,
    ) -> Result<(), ViewerError> {
        let post = self.post.clone();
        self.cap
            .call(move |c| {
                let provider = picker.map(|p| super::picker::provider(p, post));
                // The remover is dropped: the slot is last-write-wins, and
                // `None` clears it.
                let _ = open(c)?.set_file_picker_provider(provider);
                Ok(())
            })
            .await
    }

    pub fn drafts(&self) -> Arc<super::AnnotationDraftsHandle> {
        Arc::new(super::AnnotationDraftsHandle { cap: self.cap.clone() })
    }

    pub fn threads(&self) -> Arc<super::AnnotationThreadsHandle> {
        Arc::new(super::AnnotationThreadsHandle { cap: self.cap.clone() })
    }

    pub fn selection(&self) -> Arc<super::AnnotationSelectionHandle> {
        Arc::new(super::AnnotationSelectionHandle { cap: self.cap.clone() })
    }

    pub fn tools(&self) -> Arc<super::AnnotationToolsHandle> {
        Arc::new(super::AnnotationToolsHandle { cap: self.cap.clone() })
    }

    pub async fn get(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<Option<AnnotationRecord>, ViewerError> {
        self.cap
            .call(move |c| {
                let c = open(c)?;
                c.get(&ref_json(&annotation_ref)?).map(|dto| record_out(c, &dto)).transpose()
            })
            .await
    }

    pub async fn list(
        &self,
        filter: Option<AnnotationFilter>,
    ) -> Result<Vec<AnnotationRecord>, ViewerError> {
        self.cap
            .call(move |c| {
                let c = open(c)?;
                let filter = filter.map(crate::read::annotations::AnnotationFilter::from);
                records_out(c, &c.list(filter.as_ref()))
            })
            .await
    }

    pub async fn selected(&self) -> Result<Vec<AnnotationRecord>, ViewerError> {
        self.cap
            .call(|c| {
                let c = open(c)?;
                records_out(c, &c.list_selected())
            })
            .await
    }

    /// `getAt(page, point)`: the topmost annotation under a page-space point.
    pub async fn hit_test_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<AnnotationRecord>, ViewerError> {
        self.cap
            .call(move |c| {
                let c = open(c)?;
                c.annotation_at(&page_json(page), geometry_point(point))
                    .map(|dto| record_out(c, &dto))
                    .transpose()
            })
            .await
    }

    /// Whether a change of this annotation is still on its way to the engine.
    pub async fn is_pending(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.is_pending(&ref_json(&annotation_ref)?))).await
    }

    /// `getStatus()`: the records mirror's status, as metadata, form and
    /// signature report it.
    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap
            .call(|c| {
                Ok(match open(c)?.records.get_status().as_str() {
                    "ready" => ResourceStatus::Ready,
                    "forbidden" => ResourceStatus::Forbidden,
                    "error" => ResourceStatus::Error,
                    "idle" => ResourceStatus::Idle,
                    _ => ResourceStatus::Loading,
                })
            })
            .await
    }

    /// Re-read every annotation from the engine and wait for it.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.records.refresh()), |()| ()).await
    }

    /// `whenSynced()`: every load and page re-read in flight has settled.
    pub async fn when_synced(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.records.settled()), |()| ()).await
    }

    /// `create(page, fields, resources, { tool, select })`: resolves with the
    /// created annotation's ref once the engine confirmed it. `fields_json`
    /// is the engine's field bag for the subtype (`{"subtype": "square",
    /// "box": …}`).
    pub async fn create(
        &self,
        page: PageRef,
        fields_json: String,
        tool: Option<String>,
        select: Option<bool>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let fields = object_of(&fields_json)?;
                    let options =
                        CreateOptions { signal: None, tool, select: select.unwrap_or(false) };
                    Ok(open(c)?.create(&page_json(page), &fields, None, options))
                },
                |created: Value| ref_out(&created["annotation"]["ref"]).map_err(ViewerError::from),
            )
            .await?
    }

    /// `update(ref, patch)`: `patch_json` is the engine's field bag.
    pub async fn update(
        &self,
        annotation_ref: AnnotationRef,
        patch_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let patch = object_of(&patch_json)?;
                    Ok(open(c)?.update(&ref_json(&annotation_ref)?, &patch, None, None))
                },
                |_: Value| (),
            )
            .await
    }

    pub async fn delete(
        &self,
        annotation_ref: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete(&ref_json(&annotation_ref)?, None)),
                |()| (),
            )
            .await
    }

    /// `reorder(refs, position)`: `position_json` is `"front"`, `"back"`,
    /// `"forward"`, `"backward"` or `{ "above": ref }` / `{ "below": ref }`.
    pub async fn reorder(
        &self,
        refs: Vec<AnnotationRef>,
        position_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let position: Value = serde_json::from_str(&position_json)
                        .map_err(|e| super::invalid(format!("a position is not JSON: {e}")))?;
                    Ok(open(c)?.reorder(&refs_json(&refs)?, &position, None))
                },
                |()| (),
            )
            .await
    }

    pub async fn can_read(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read())).await
    }

    pub async fn can_create(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_create())).await
    }

    pub async fn can_update(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_update(&ref_json(&annotation_ref)?))).await
    }

    pub async fn can_delete(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_delete(&ref_json(&annotation_ref)?))).await
    }

    /// Every hook in one registration. An event that does not decode is
    /// reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn AnnotationEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            let cap = c.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let events = &cap.events;
                    let l = listener.clone();
                    let resynced = events.resynced.on(move |e| {
                        let pages = match e.get("pages") {
                            Some(Value::Array(pages)) => AnnotationResyncedPages::Pages {
                                pages: pages.iter().map(page_out).collect::<Result<_, _>>()?,
                            },
                            _ => AnnotationResyncedPages::All,
                        };
                        l.on_event(AnnotationEvent::Resynced { pages });
                        Ok(())
                    });
                    let (l, k) = (listener.clone(), cap.clone());
                    let created = events.created.on(move |e| {
                        let annotation =
                            record_out(&k, e.get("annotation").unwrap_or(&Value::Null))?;
                        l.on_event(AnnotationEvent::Created {
                            event: AnnotationChangedEvent { annotation },
                        });
                        Ok(())
                    });
                    let (l, k) = (listener.clone(), cap.clone());
                    let updated = events.updated.on(move |e| {
                        let annotation =
                            record_out(&k, e.get("annotation").unwrap_or(&Value::Null))?;
                        l.on_event(AnnotationEvent::Updated {
                            event: AnnotationChangedEvent { annotation },
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = events.deleted.on(move |e| {
                        l.on_event(AnnotationEvent::Deleted {
                            event: AnnotationDeletedEvent {
                                refs: refs_out(e.get("refs").unwrap_or(&Value::Null))?,
                                page: page_out(e.get("page").unwrap_or(&Value::Null))?,
                            },
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let reordered = events.reordered.on(move |e| {
                        l.on_event(AnnotationEvent::Reordered {
                            page: page_out(e.get("page").unwrap_or(&Value::Null))?,
                            order: refs_out(e.get("order").unwrap_or(&Value::Null))?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let selection = events.selection_changed.on(move |e| {
                        l.on_event(AnnotationEvent::SelectionChanged {
                            refs: refs_out(e.get("refs").unwrap_or(&Value::Null))?,
                            previous_refs: refs_out(e.get("previousRefs").unwrap_or(&Value::Null))?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let draft = events.draft_changed.on(move |e| {
                        l.on_event(AnnotationEvent::DraftChanged {
                            draft: draft_out(e.get("draft").unwrap_or(&Value::Null))?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let editing = events.editing_changed.on(move |e| {
                        let annotation_ref = match e.get("ref") {
                            None | Some(Value::Null) => None,
                            Some(r) => Some(ref_out(r)?),
                        };
                        l.on_event(AnnotationEvent::EditingChanged { annotation_ref });
                        Ok(())
                    });
                    let l = listener.clone();
                    let hover = events.hover_changed.on(move |e| {
                        let annotation_ref = match e.get("ref") {
                            None | Some(Value::Null) => None,
                            Some(r) => Some(ref_out(r)?),
                        };
                        l.on_event(AnnotationEvent::HoverChanged { annotation_ref });
                        Ok(())
                    });
                    let l = listener.clone();
                    let thread = events.thread_changed.on(move |e| {
                        let change = match e.get("change").and_then(Value::as_str) {
                            Some("reply") => AnnotationThreadChange::Reply,
                            Some("text") => AnnotationThreadChange::Text,
                            Some("status") => AnnotationThreadChange::Status,
                            Some("marked") => AnnotationThreadChange::Marked,
                            _ => AnnotationThreadChange::Deleted,
                        };
                        l.on_event(AnnotationEvent::ThreadChanged {
                            root_ref: ref_out(e.get("rootRef").unwrap_or(&Value::Null))?,
                            change,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let failed = events.write_failed.on(move |e| {
                        l.on_event(AnnotationEvent::WriteFailed {
                            refs: refs_out(e.get("refs").unwrap_or(&Value::Null))?,
                            error: plugin_error_info_of(e.get("error").unwrap_or(&Value::Null)),
                        });
                        Ok(())
                    });
                    let offs = [
                        resynced, created, updated, deleted, reordered, selection, draft, editing,
                        hover, thread, failed,
                    ];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
