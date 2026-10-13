use std::sync::Arc;

use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, PdfLinkTarget, PluginErrorInfo, ViewerError,
};
use serde::Deserialize;
use serde_json::Value;

use super::{decode, open, plugin_error_info, ref_out, refs_out};
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

#[derive(Deserialize)]
struct CommentDto {
    #[serde(rename = "ref")]
    ref_: epdf_annotation_core::AnnotationRef,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    contents: Option<String>,
    #[serde(default)]
    modified: Option<String>,
}

fn comment(dto: &Value) -> Result<AnnotationComment, epdf_kernel::Error> {
    let dto: CommentDto = decode(dto.clone())?;
    Ok(AnnotationComment {
        annotation_ref: ref_out(dto.ref_)?,
        author: dto.author,
        contents: dto.contents,
        modified_at: dto.modified,
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

impl TryFrom<crate::CommentThread> for AnnotationCommentThread {
    type Error = epdf_kernel::Error;

    fn try_from(t: crate::CommentThread) -> Result<Self, Self::Error> {
        Ok(AnnotationCommentThread {
            root: comment(&t.root)?,
            replies: t.replies.iter().map(comment).collect::<Result<_, _>>()?,
            status: t.review.last_change.map(|s| s.state),
            marked_by: t.review.marked_by,
        })
    }
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
                open(c)?
                    .comments()
                    .list_threads()
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<Result<_, _>>()
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
                open(c)?
                    .comments()
                    .get_thread(&annotation_ref.into())
                    .map(TryInto::try_into)
                    .transpose()
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
                move |c| Ok(open(c)?.comments().reply(annotation_ref.into(), text)),
                |r| ref_out(r).map_err(ViewerError::from),
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
                move |c| Ok(open(c)?.comments().set_text(annotation_ref.into(), text)),
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
                    Ok(open(c)?
                        .comments()
                        .set_status(annotation_ref.into(), status.as_str().into()))
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
                move |c| Ok(open(c)?.comments().set_marked(annotation_ref.into(), marked)),
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
                move |c| Ok(open(c)?.comments().delete(annotation_ref.into())),
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
                move |c| Ok(open(c)?.comments().delete_thread(annotation_ref.into())),
                |r| {
                    Ok::<_, ViewerError>(AnnotationThreadDeleteResult {
                        deleted: refs_out(r.deleted)?,
                        failed: r
                            .failed
                            .into_iter()
                            .map(|f| {
                                Ok(AnnotationThreadDeleteFailure {
                                    annotation_ref: ref_out(f.ref_)?,
                                    error: plugin_error_info(&f.error),
                                })
                            })
                            .collect::<Result<_, epdf_kernel::Error>>()?,
                    })
                },
            )
            .await?
    }

    pub async fn permissions(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<AnnotationCommentPermissions, ViewerError> {
        self.cap
            .call(move |c| {
                let p = open(c)?.comments().get_permissions(&annotation_ref.into());
                Ok(AnnotationCommentPermissions {
                    can_reply: p.can_reply,
                    can_edit_text: p.can_edit_text,
                    can_delete: p.can_delete,
                    can_set_status: p.can_set_status,
                    can_delete_thread: p.can_delete_thread,
                })
            })
            .await
    }

    /// The link attached to an annotation (a shape that acts as a link).
    pub async fn link(
        &self,
        annotation_ref: AnnotationRef,
    ) -> Result<Option<PdfLinkTarget>, ViewerError> {
        self.cap
            .call(move |c| Ok(open(c)?.links().get(&annotation_ref.into()).map(Into::into)))
            .await
    }

    pub async fn set_link(
        &self,
        annotation_ref: AnnotationRef,
        target: PdfLinkTarget,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.links().set(&annotation_ref.into(), target.into())),
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
                move |c| Ok(open(c)?.links().clear(&annotation_ref.into())),
                |()| (),
            )
            .await
    }
}
