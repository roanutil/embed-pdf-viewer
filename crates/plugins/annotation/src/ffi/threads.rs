use std::sync::Arc;

use epdf_ffi::{AnnotationRef, CancelToken, KernelBound, PluginErrorInfo, ViewerError};
use serde_json::Value;

use super::{open, plugin_error_info_of, ref_json, ref_out, refs_out};
use crate::AnnotationCapability;

/// Comment threads and the links attached to annotations.
#[derive(uniffi::Object)]
pub struct AnnotationThreadsHandle {
    pub(crate) cap: KernelBound<AnnotationCapability>,
}

/// One comment: a thread's root or a reply.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationComment {
    pub annotation_ref: AnnotationRef,
    pub author: Option<String>,
    pub contents: Option<String>,
    pub modified_at: Option<String>,
}

fn comment(dto: &Value) -> Result<AnnotationComment, epdf_kernel::Error> {
    let s = |k: &str| dto.get(k).and_then(Value::as_str).map(str::to_string);
    Ok(AnnotationComment {
        annotation_ref: ref_out(dto.get("ref").unwrap_or(&Value::Null))?,
        author: s("author"),
        contents: s("contents"),
        modified_at: s("modifiedAt"),
    })
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationCommentThread {
    pub root: AnnotationComment,
    pub replies: Vec<AnnotationComment>,
    /// The latest review state anyone set (`Accepted`, `Rejected`, ...).
    pub status: Option<String>,
    pub marked_by: Vec<String>,
}

fn thread(t: &Value) -> Result<AnnotationCommentThread, epdf_kernel::Error> {
    let review = t.get("review").unwrap_or(&Value::Null);
    Ok(AnnotationCommentThread {
        root: comment(t.get("root").unwrap_or(&Value::Null))?,
        replies: t
            .get("replies")
            .and_then(Value::as_array)
            .map(|r| r.iter().map(comment).collect::<Result<_, _>>())
            .transpose()?
            .unwrap_or_default(),
        status: review
            .get("lastChange")
            .and_then(|c| c.get("state"))
            .and_then(Value::as_str)
            .map(str::to_string),
        marked_by: review
            .get("markedBy")
            .and_then(Value::as_array)
            .map(|m| m.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnnotationCommentPermissions {
    pub can_reply: bool,
    pub can_edit_text: bool,
    pub can_delete: bool,
    pub can_set_status: bool,
    pub can_delete_thread: bool,
}

/// The review states `/StateModel Review` defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AnnotationThreadStatus {
    Accepted,
    Rejected,
    Cancelled,
    Completed,
    None,
}

impl AnnotationThreadStatus {
    fn as_str(self) -> &'static str {
        match self {
            AnnotationThreadStatus::Accepted => "Accepted",
            AnnotationThreadStatus::Rejected => "Rejected",
            AnnotationThreadStatus::Cancelled => "Cancelled",
            AnnotationThreadStatus::Completed => "Completed",
            AnnotationThreadStatus::None => "None",
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationThreadDeleteFailure {
    pub annotation_ref: AnnotationRef,
    pub error: PluginErrorInfo,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnnotationThreadDeleteResult {
    pub deleted: Vec<AnnotationRef>,
    pub failed: Vec<AnnotationThreadDeleteFailure>,
}

#[uniffi::export]
impl AnnotationThreadsHandle {
    pub async fn threads(&self) -> Result<Vec<AnnotationCommentThread>, ViewerError> {
        self.cap
            .call(|c| {
                let all = open(c)?.comments_list_threads();
                all.as_array()
                    .map(|t| t.iter().map(thread).collect())
                    .unwrap_or_else(|| Ok(Vec::new()))
            })
            .await
    }

    /// The thread any member of it belongs to.
    pub async fn thread(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<Option<AnnotationCommentThread>, ViewerError> {
        self.cap
            .call(move |c| {
                let t = open(c)?.comments_get_thread(&ref_json(&annotation_ref)?);
                if t.is_null() { Ok(None) } else { thread(&t).map(Some) }
            })
            .await
    }

    pub async fn reply(
        &self,
        annotation_ref: AnnotationRef,
        text: String,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationRef, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments_reply(&ref_json(&annotation_ref)?, &text, None)),
                |made: Value| ref_out(&made["ref"]).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn set_text(
        &self,
        annotation_ref: AnnotationRef,
        text: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments_set_text(&ref_json(&annotation_ref)?, &text, None)),
                |()| (),
            )
            .await
    }

    pub async fn set_status(
        &self,
        annotation_ref: AnnotationRef,
        status: AnnotationThreadStatus,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.comments_set_status(
                        &ref_json(&annotation_ref)?,
                        status.as_str(),
                        None,
                    ))
                },
                |()| (),
            )
            .await
    }

    pub async fn set_marked(
        &self,
        annotation_ref: AnnotationRef,
        marked: bool,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    Ok(open(c)?.comments_set_marked(&ref_json(&annotation_ref)?, marked, None))
                },
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
                move |c| Ok(open(c)?.comments_delete(&ref_json(&annotation_ref)?, None)),
                |()| (),
            )
            .await
    }

    pub async fn delete_thread(
        &self,
        annotation_ref: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<AnnotationThreadDeleteResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.comments_delete_thread(&ref_json(&annotation_ref)?, None)),
                |r: Value| {
                    let failed = r
                        .get("failed")
                        .and_then(Value::as_array)
                        .map(|list| {
                            list.iter()
                                .map(|f| {
                                    Ok(AnnotationThreadDeleteFailure {
                                        annotation_ref: ref_out(
                                            f.get("ref").unwrap_or(&Value::Null),
                                        )?,
                                        error: plugin_error_info_of(
                                            f.get("error").unwrap_or(&Value::Null),
                                        ),
                                    })
                                })
                                .collect::<Result<Vec<_>, epdf_kernel::Error>>()
                        })
                        .transpose()?
                        .unwrap_or_default();
                    Ok::<_, ViewerError>(AnnotationThreadDeleteResult {
                        deleted: refs_out(r.get("deleted").unwrap_or(&Value::Null))?,
                        failed,
                    })
                },
            )
            .await?
    }

    /// What this session may do to a comment (`comments.can*`).
    pub async fn permissions(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<AnnotationCommentPermissions, ViewerError> {
        self.cap
            .call(move |c| {
                let c = open(c)?;
                let r = ref_json(&annotation_ref)?;
                Ok(AnnotationCommentPermissions {
                    can_reply: c.can_create(),
                    can_edit_text: c.comments_can_set_text(&r),
                    can_delete: c.can_delete(&r),
                    can_set_status: c.can_create(),
                    can_delete_thread: c.comments_can_delete_thread(&r),
                })
            })
            .await
    }

    /// `links.get(ref)`: the link attached to an annotation, as the engine's
    /// `LinkTarget` JSON, or `None`.
    pub async fn link(&self, annotation_ref: AnnotationRef) -> Result<Option<String>, ViewerError> {
        self.cap
            .call(move |c| {
                let target = open(c)?.links_get(&ref_json(&annotation_ref)?);
                Ok((!target.is_null()).then(|| target.to_string()))
            })
            .await
    }

    /// `links.set(ref, target)`: `target_json` is the engine's `LinkTarget`.
    pub async fn set_link(
        &self,
        annotation_ref: AnnotationRef,
        target_json: String,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let target: Value = serde_json::from_str(&target_json)
                        .map_err(|e| super::invalid(format!("a link target is not JSON: {e}")))?;
                    Ok(open(c)?.links_set(&ref_json(&annotation_ref)?, target, None))
                },
                |()| (),
            )
            .await
    }

    pub async fn clear_link(
        &self,
        annotation_ref: AnnotationRef,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.links_clear(&ref_json(&annotation_ref)?, None)),
                |()| (),
            )
            .await
    }
}
