//! Owned attachment bytes and typed document/page identities across UniFFI.
use crate::AttachmentCapability;
use epdf_ffi::document::DocumentEventOrigin;
use epdf_ffi::{
    AnnotationRef, CancelToken, KernelBound, KernelPost, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::bytes::Bytes;
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::{Value, json};
use std::sync::Arc;
fn invalid(message: impl Into<String>) -> Error {
    Error::named("InvalidState", message.into()).with("code", "invalid-state")
}
fn text(value: &Value, field: &str) -> Result<String, Error> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("attachment {field} is not text")))
}
fn optional_text(value: &Value, field: &str) -> Result<Option<String>, Error> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(invalid(format!("attachment {field} is not text"))),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AttachmentRef {
    pub key: String,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AttachmentHome {
    Document { file: AttachmentRef },
    Page { annotation: AnnotationRef },
}
impl AttachmentHome {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Document { file } => {
                json!({"kind":"document","ref":{"kind":"key","key":file.key}})
            }
            Self::Page { annotation } => json!({"kind":"page","ref":annotation.to_json()}),
        }
    }
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        match value.get("kind").and_then(Value::as_str) {
            Some("document") if value["ref"]["kind"] == "key" => {
                Ok(Self::Document { file: AttachmentRef { key: text(&value["ref"], "key")? } })
            }
            Some("page") => Ok(Self::Page { annotation: AnnotationRef::from_json(&value["ref"])? }),
            _ => Err(invalid("attachment home has an unknown kind")),
        }
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AttachedFile {
    pub id: String,
    pub home: AttachmentHome,
    pub name: String,
    pub mime_type: Option<String>,
    pub description: Option<String>,
    pub size: Option<u64>,
    pub checksum: Option<String>,
    pub created_at: Option<String>,
    pub modified_at: Option<String>,
}
impl AttachedFile {
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let size = match value.get("size") {
            None | Some(Value::Null) => None,
            Some(number) => Some(
                number
                    .as_u64()
                    .or_else(|| {
                        number
                            .as_f64()
                            .filter(|n| {
                                n.fract() == 0.0 && *n >= 0.0 && *n <= 9_007_199_254_740_991.0
                            })
                            .map(|n| n as u64)
                    })
                    .ok_or_else(|| invalid("attachment size is not a whole byte count"))?,
            ),
        };
        Ok(Self {
            id: text(value, "id")?,
            home: AttachmentHome::from_json(&value["home"])?,
            name: text(value, "name")?,
            mime_type: optional_text(value, "mimeType")?,
            description: optional_text(value, "description")?,
            size,
            checksum: optional_text(value, "checksum")?,
            created_at: optional_text(value, "createdAt")?,
            modified_at: optional_text(value, "modifiedAt")?,
        })
    }
    pub fn to_json(&self) -> Value {
        json!({"id":self.id,"home":self.home.to_json(),"name":self.name,"mimeType":self.mime_type,"description":self.description,"size":self.size,"checksum":self.checksum,"createdAt":self.created_at,"modifiedAt":self.modified_at})
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AttachmentFileSource {
    pub data: Vec<u8>,
    #[uniffi(default=None)]
    pub name: Option<String>,
    #[uniffi(default=None)]
    pub mime_type: Option<String>,
    #[uniffi(default=None)]
    pub description: Option<String>,
}
impl AttachmentFileSource {
    pub fn to_json(self) -> Value {
        let mut value = json!({"data":Bytes(self.data).to_json()});
        if let Some(name) = self.name {
            value["name"] = name.into();
        }
        if let Some(mime) = self.mime_type {
            value["mimeType"] = mime.into();
        }
        if let Some(description) = self.description {
            value["description"] = description.into();
        }
        value
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AttachmentContent {
    pub data: Vec<u8>,
    pub name: String,
    pub mime_type: Option<String>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AttachmentEvent {
    Changed { kind: String, files: Vec<AttachedFile>, origin: DocumentEventOrigin },
    Resynced { files: Vec<AttachedFile> },
}
#[uniffi::export(with_foreign)]
pub trait AttachmentEventListener: Send + Sync {
    fn on_event(&self, event: AttachmentEvent);
    fn on_end(&self);
}
fn files(values: &[Value]) -> Result<Vec<AttachedFile>, Error> {
    values.iter().map(AttachedFile::from_json).collect()
}
fn open(cap: &AttachmentCapability) -> Result<&AttachmentCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "attachment",
            "the document is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}
fn status(value: epdf_kernel::mirror::ResourceStatus) -> ResourceStatus {
    match value {
        epdf_kernel::mirror::ResourceStatus::Idle => ResourceStatus::Idle,
        epdf_kernel::mirror::ResourceStatus::Loading => ResourceStatus::Loading,
        epdf_kernel::mirror::ResourceStatus::Ready => ResourceStatus::Ready,
        epdf_kernel::mirror::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
        epdf_kernel::mirror::ResourceStatus::Error => ResourceStatus::Error,
    }
}
fn options() -> (crate::OperationOptions, AbortSignal) {
    let signal = AbortSignal::new();
    (crate::OperationOptions { signal: Some(signal.clone()) }, signal)
}
#[derive(uniffi::Object)]
pub struct AttachmentHandle {
    cap: KernelBound<AttachmentCapability>,
    post: KernelPost,
}
impl AttachmentHandle {
    pub fn new(cap: AttachmentCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(Self { cap: KernelBound::new(cap, post.clone()), post })
    }
}
#[uniffi::export]
impl AttachmentHandle {
    pub async fn files(&self) -> Result<Vec<AttachedFile>, ViewerError> {
        self.cap.call(|cap| files(&open(cap)?.list())).await
    }
    pub async fn file(&self, id: String) -> Result<Option<AttachedFile>, ViewerError> {
        self.cap
            .call(move |cap| open(cap)?.get(&id).as_ref().map(AttachedFile::from_json).transpose())
            .await
    }
    pub async fn selected_files(&self) -> Result<Vec<AttachedFile>, ViewerError> {
        self.cap.call(|cap| files(&open(cap)?.get_selected())).await
    }
    pub async fn select(&self, files: Vec<AttachedFile>) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.select(&files.iter().map(AttachedFile::to_json).collect::<Vec<_>>());
                Ok(())
            })
            .await
    }
    pub async fn is_pending(&self, file: AttachedFile) -> Result<bool, ViewerError> {
        self.cap.call(move |cap| Ok(open(cap)?.is_pending(&file.to_json()))).await
    }
    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|cap| Ok(status(open(cap)?.get_status()))).await
    }
    pub async fn can_download(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.can_download())).await
    }
    pub async fn can_add(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.can_add())).await
    }
    pub async fn can_remove(&self, file: AttachedFile) -> Result<bool, ViewerError> {
        self.cap.call(move |cap| Ok(open(cap)?.can_remove(&file.to_json()))).await
    }
    pub async fn reveal(&self, file: AttachedFile) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.reveal(&file.to_json());
                Ok(())
            })
            .await
    }
    pub async fn ensure_loaded(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .ensure_loaded(options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .refresh(options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }
    pub async fn add(
        &self,
        sources: Vec<AttachmentFileSource>,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<AttachedFile>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .add(
                            sources.into_iter().map(AttachmentFileSource::to_json).collect(),
                            options,
                        )
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |result| files(&result).map_err(ViewerError::from),
            )
            .await?
    }
    pub async fn remove(
        &self,
        file: AttachedFile,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .remove(file.to_json(), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }
    pub async fn download(
        &self,
        file: AttachedFile,
        cancel: Arc<CancelToken>,
    ) -> Result<AttachmentContent, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .download(file.to_json(), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |content| {
                    Ok(AttachmentContent {
                        data: content.data().ok_or_else(|| {
                            ViewerError::from(invalid("attachment bytes do not decode"))
                        })?,
                        name: content.name,
                        mime_type: match content.mime_type {
                            Value::Null => None,
                            Value::String(text) => Some(text),
                            _ => {
                                return Err(ViewerError::from(invalid(
                                    "attachment MIME type is not text",
                                )));
                            }
                        },
                    })
                },
            )
            .await?
    }
    pub fn subscribe(&self, listener: Arc<dyn AttachmentEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |cap| {
            let ending = listener.clone();
            slot.fill_until_end(
                cap.lifetime(),
                || {
                    let to = listener.clone();
                    let changed = cap.on_changed(move |event| {
                        to.on_event(AttachmentEvent::Changed {
                            kind: event.kind.into(),
                            files: files(&event.files)?,
                            origin: DocumentEventOrigin::from_json(&event.origin)?,
                        });
                        Ok(())
                    });
                    let resynced = cap.on_resynced(move |event| {
                        listener
                            .on_event(AttachmentEvent::Resynced { files: files(&event.files)? });
                        Ok(())
                    });
                    Unsubscribe::new(move || {
                        changed.call();
                        resynced.call();
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
