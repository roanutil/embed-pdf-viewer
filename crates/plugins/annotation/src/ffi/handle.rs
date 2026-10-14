use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Point, Subscription, ViewerError,
};
use epdf_kernel::Unsubscribe;

use super::{
    Annotation, AnnotationBatchResult, AnnotationChangedEvent, AnnotationCreateBatchResult,
    AnnotationCreateFailure, AnnotationDeletedEvent, AnnotationEvent, AnnotationEventListener,
    AnnotationFilter, AnnotationHydration, AnnotationPatch, AnnotationResyncedPages,
    AnnotationThreadChange, CreateAnnotationInput, annotations_out, open, plugin_error_info,
    ref_out, refs_out,
};
use crate::AnnotationCapability;

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
    ) -> Result<Option<Annotation>, ViewerError> {
        self.cap
            .call(move |c| {
                open(c)?.get(&annotation_ref.into()).map(Annotation::try_from).transpose()
            })
            .await
    }

    pub async fn list(
        &self,
        filter: Option<AnnotationFilter>,
    ) -> Result<Vec<Annotation>, ViewerError> {
        self.cap
            .call(move |c| {
                let filter = filter.map(crate::AnnotationFilter::from);
                annotations_out(open(c)?.list(filter.as_ref()))
            })
            .await
    }

    pub async fn selected(&self) -> Result<Vec<Annotation>, ViewerError> {
        self.cap.call(|c| annotations_out(open(c)?.list_selected())).await
    }

    /// The topmost annotation under a page-space point.
    pub async fn hit_test_at(
        &self,
        page: PageRef,
        point: Point,
    ) -> Result<Option<AnnotationRef>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.hit_test_at(page.into(), point.into()).map(ref_out).transpose())
            .await
    }

    /// `"loading"`, `"ready"`, `"forbidden"` or `"error"`, as the plugin
    /// reports its whole-document state.
    pub async fn status(&self) -> Result<String, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_status().to_string())).await
    }

    pub async fn hydration(&self) -> Result<AnnotationHydration, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_hydration().into())).await
    }

    /// Start hydration if it has not run. Returns before it finishes;
    /// `refresh` waits.
    pub async fn ensure_hydrated(&self) -> Result<(), ViewerError> {
        self.cap.call(|c| Ok(open(c)?.ensure_hydrated())).await
    }

    /// Re-read every annotation from the engine and wait for it.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh()), |()| ()).await
    }

    pub async fn reload_page(
        &self,
        page: PageRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.reload_page(page.into())),
                |()| (),
            )
            .await
    }

    /// Resolves with the created annotation's ref once the engine confirmed it.
    pub async fn create(
        &self,
        input: CreateAnnotationInput,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.create(input.try_into()?)),
                |r| ref_out(r).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn create_many(
        &self,
        inputs: Vec<CreateAnnotationInput>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationCreateBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let inputs = inputs
                        .into_iter()
                        .map(|i| {
                            let input = crate::CreateAnnotationInput::try_from(i)?;
                            Ok(create_input_json(&input))
                        })
                        .collect::<Result<Vec<_>, epdf_kernel::Error>>()?;
                    Ok(open(c)?.create_many(inputs))
                },
                |batch| {
                    let applied =
                        batch.applied.into_iter().map(ref_out).collect::<Result<Vec<_>, _>>()?;
                    let failed = batch
                        .failed
                        .into_iter()
                        .map(|(entry, e)| AnnotationCreateFailure {
                            index: entry
                                .get("index")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0) as u32,
                            error: plugin_error_info(&e),
                        })
                        .collect();
                    Ok::<_, ViewerError>(AnnotationCreateBatchResult { applied, failed })
                },
            )
            .await?
    }

    pub async fn update(
        &self,
        annotation_ref: AnnotationRef,
        patch: AnnotationPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.update(annotation_ref.into(), patch.try_into()?)),
                |()| (),
            )
            .await
    }

    /// One patch applied to every ref, best effort per ref.
    pub async fn update_many(
        &self,
        refs: Vec<AnnotationRef>,
        patch: AnnotationPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let refs = refs.into_iter().map(Into::into).collect();
                    Ok(open(c)?.update_many(refs, patch.try_into()?))
                },
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn set_rotation(
        &self,
        annotation_ref: AnnotationRef,
        degrees: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.set_rotation(annotation_ref.into(), degrees)),
                |()| (),
            )
            .await
    }

    pub async fn rotate_by(
        &self,
        annotation_ref: AnnotationRef,
        delta: f64,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.rotate_by(annotation_ref.into(), delta)),
                |()| (),
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
                move |c| Ok(open(c)?.delete(annotation_ref.into())),
                |()| (),
            )
            .await
    }

    pub async fn delete_many(
        &self,
        refs: Vec<AnnotationRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationBatchResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.delete_many(refs.into_iter().map(Into::into).collect())),
                |b| AnnotationBatchResult::try_from(b).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn can_read(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_read())).await
    }

    pub async fn can_create(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_create())).await
    }

    pub async fn can_edit(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_edit(&annotation_ref.into()))).await
    }

    pub async fn can_delete(&self, annotation_ref: AnnotationRef) -> Result<bool, ViewerError> {
        self.cap.call(move |c| Ok(open(c)?.can_delete(&annotation_ref.into()))).await
    }

    /// Every hook in one registration. An event that does not decode is
    /// reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn AnnotationEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let l = listener.clone();
                    let resynced = c.on_resynced(move |e| {
                        let pages = match &e.pages {
                            crate::ResyncedPages::All => AnnotationResyncedPages::All,
                            crate::ResyncedPages::Pages(p) => AnnotationResyncedPages::Pages {
                                pages: p.iter().map(|p| (*p).into()).collect(),
                            },
                        };
                        l.on_event(AnnotationEvent::Resynced { pages });
                        Ok(())
                    });
                    let l = listener.clone();
                    let created = c.on_created(move |e| {
                        l.on_event(AnnotationEvent::Created {
                            event: AnnotationChangedEvent::try_from(e)?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let updated = c.on_updated(move |e| {
                        l.on_event(AnnotationEvent::Updated {
                            event: AnnotationChangedEvent::try_from(e)?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let deleted = c.on_deleted(move |e| {
                        l.on_event(AnnotationEvent::Deleted {
                            event: AnnotationDeletedEvent {
                                annotation_ref: ref_out(e.ref_.clone())?,
                                page: e.page.into(),
                            },
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let selection = c.on_selection_changed(move |e| {
                        l.on_event(AnnotationEvent::SelectionChanged {
                            refs: refs_out(e.refs.clone())?,
                            previous_refs: refs_out(e.previous_refs.clone())?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let draft = c.on_draft_changed(move |e| {
                        l.on_event(AnnotationEvent::DraftChanged {
                            draft: e.draft.clone().map(Into::into),
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let editing = c.on_editing_changed(move |e| {
                        l.on_event(AnnotationEvent::EditingChanged {
                            annotation_ref: e.ref_.clone().map(ref_out).transpose()?,
                        });
                        Ok(())
                    });
                    let l = listener.clone();
                    let thread = c.comments().on_thread_changed(move |e| {
                        let change = match e.change {
                            crate::ThreadChange::Reply => AnnotationThreadChange::Reply,
                            crate::ThreadChange::Text => AnnotationThreadChange::Text,
                            crate::ThreadChange::Status => AnnotationThreadChange::Status,
                            crate::ThreadChange::Marked => AnnotationThreadChange::Marked,
                            crate::ThreadChange::Deleted => AnnotationThreadChange::Deleted,
                        };
                        l.on_event(AnnotationEvent::ThreadChanged {
                            root_ref: ref_out(e.root_ref.clone())?,
                            change,
                        });
                        Ok(())
                    });
                    let offs =
                        [resynced, created, updated, deleted, selection, draft, editing, thread];
                    Unsubscribe::new(move || offs.iter().for_each(Unsubscribe::call))
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}

/// `createMany` takes JSON inputs (`crud.rs` `create_many(Vec<Value>)`); the
/// input's fields serialize back to the shape `CreateAnnotationInput`
/// deserializes from.
fn create_input_json(input: &crate::CreateAnnotationInput) -> serde_json::Value {
    let mut out = serde_json::json!({ "page": input.page, "subtype": input.subtype });
    let map = out.as_object_mut().expect("an object");
    if let Some(tool) = &input.tool {
        map.insert("tool".into(), tool.clone().into());
    }
    if let Some(props) = &input.props {
        map.insert("props".into(), serde_json::to_value(props).expect("a props patch"));
    }
    if let Some(flags) = &input.flags {
        map.insert("flags".into(), serde_json::to_value(flags).expect("flags"));
    }
    if let Some(select) = input.select {
        map.insert("select".into(), select.into());
    }
    if let Some(bounds) = &input.bounds {
        map.insert("bounds".into(), bounds.clone());
    }
    if let Some(rotation) = input.rotation {
        map.insert("rotation".into(), rotation.into());
    }
    for (key, value) in [
        ("from", input.from.map(|p| serde_json::to_value(p).expect("a point"))),
        ("to", input.to.map(|p| serde_json::to_value(p).expect("a point"))),
        ("vertices", input.vertices.as_ref().map(|v| serde_json::to_value(v).expect("points"))),
        ("strokes", input.strokes.as_ref().map(|s| serde_json::to_value(s).expect("strokes"))),
        ("quads", input.quads.as_ref().map(|q| serde_json::to_value(q).expect("quads"))),
        ("callout", input.callout.map(|c| serde_json::to_value(c).expect("a callout"))),
    ] {
        if let Some(value) = value {
            map.insert(key.into(), value);
        }
    }
    out
}
