//! The metadata plugin through uniffi: typed records, one handle, one event
//! enum. Every method hops to the kernel thread through `KernelBound`.
//!
//! The plugin keeps the engine's metadata as JSON (`crate::DocumentMetadata`
//! is a `Value`), so the records here mirror the engine's wire shape
//! (`epdf-engine`'s `DocumentMetadata` and `MetadataPatch`) and the handle
//! decodes into them. A value that does not decode is an `invalid-state`
//! error: the record and the engine disagree, and a default would hide it.

use std::collections::HashMap;
use std::sync::Arc;

use epdf_ffi::{CancelToken, KernelBound, KernelPost, ResourceStatus, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::signal::AbortSignal;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::MetadataCapability;

/// `/Trapped`: always present, so a patch can set it but not clear it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum DocumentMetadataTrapped {
    True,
    False,
    Unknown,
}

/// The Info dictionary, as the engine reports it. Dates are ISO 8601.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub producer: Option<String>,
    pub creator: Option<String>,
    pub created: Option<String>,
    pub modified: Option<String>,
    pub trapped: DocumentMetadataTrapped,
    /// Non-reserved Info keys.
    pub custom: HashMap<String, String>,
}

/// A text field of [`DocumentMetadata`], for [`MetadataPatch::clear`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MetadataField {
    Title,
    Author,
    Subject,
    Keywords,
    Producer,
    Creator,
    Created,
    Modified,
}

impl MetadataField {
    fn key(self) -> &'static str {
        match self {
            MetadataField::Title => "title",
            MetadataField::Author => "author",
            MetadataField::Subject => "subject",
            MetadataField::Keywords => "keywords",
            MetadataField::Producer => "producer",
            MetadataField::Creator => "creator",
            MetadataField::Created => "created",
            MetadataField::Modified => "modified",
        }
    }
}

/// A change to the Info dictionary. A `None` field is left alone, a value
/// sets it, and a field named in `clear` is removed (naming one that is also
/// set is refused). `custom` is per key: `None` removes that key.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct MetadataPatch {
    #[uniffi(default = None)]
    pub title: Option<String>,
    #[uniffi(default = None)]
    pub author: Option<String>,
    #[uniffi(default = None)]
    pub subject: Option<String>,
    #[uniffi(default = None)]
    pub keywords: Option<String>,
    #[uniffi(default = None)]
    pub producer: Option<String>,
    #[uniffi(default = None)]
    pub creator: Option<String>,
    #[uniffi(default = None)]
    pub created: Option<String>,
    #[uniffi(default = None)]
    pub modified: Option<String>,
    #[uniffi(default = None)]
    pub trapped: Option<DocumentMetadataTrapped>,
    #[uniffi(default = None)]
    pub custom: Option<HashMap<String, Option<String>>>,
    #[uniffi(default = [])]
    pub clear: Vec<MetadataField>,
}

impl MetadataPatch {
    /// The engine's three-state JSON patch: absent leaves, `null` clears.
    fn to_json(&self) -> Result<Value, Error> {
        let mut patch = Map::new();
        let set = [
            (MetadataField::Title, &self.title),
            (MetadataField::Author, &self.author),
            (MetadataField::Subject, &self.subject),
            (MetadataField::Keywords, &self.keywords),
            (MetadataField::Producer, &self.producer),
            (MetadataField::Creator, &self.creator),
            (MetadataField::Created, &self.created),
            (MetadataField::Modified, &self.modified),
        ];
        for (field, value) in set {
            if let Some(value) = value {
                patch.insert(field.key().into(), Value::String(value.clone()));
            }
        }
        for field in &self.clear {
            if patch.insert(field.key().into(), Value::Null).is_some_and(|v| !v.is_null()) {
                return Err(plugin_error(
                    code::INVALID_INPUT,
                    "metadata",
                    format!("metadata patch both sets and clears \"{}\"", field.key()),
                    None,
                    None,
                ));
            }
        }
        if let Some(trapped) = self.trapped {
            patch.insert("trapped".into(), serde_json::to_value(trapped).expect("an enum"));
        }
        if let Some(custom) = &self.custom {
            let custom = custom
                .iter()
                .map(|(key, value)| (key.clone(), value.clone().map_or(Value::Null, Value::String)))
                .collect();
            patch.insert("custom".into(), Value::Object(custom));
        }
        Ok(Value::Object(patch))
    }
}

/// One confirmed change, whoever caused it. The plugin's event also carries
/// its `ChangeOrigin`, which no foreign caller can act on, so it is dropped.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MetadataUpdatedEvent {
    pub metadata: DocumentMetadata,
    pub previous: Option<DocumentMetadata>,
    pub changed_keys: Vec<String>,
}

impl TryFrom<&crate::MetadataUpdatedEvent> for MetadataUpdatedEvent {
    type Error = Error;

    fn try_from(event: &crate::MetadataUpdatedEvent) -> Result<Self, Error> {
        Ok(MetadataUpdatedEvent {
            metadata: decode(event.metadata.clone())?,
            previous: event.previous.clone().map(decode).transpose()?,
            changed_keys: event.changed_keys.clone(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MetadataEvent {
    Updated { event: MetadataUpdatedEvent },
}

#[uniffi::export(with_foreign)]
pub trait MetadataEventListener: Send + Sync {
    fn on_event(&self, event: MetadataEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

impl From<crate::ResourceStatus> for ResourceStatus {
    fn from(status: crate::ResourceStatus) -> Self {
        match status {
            crate::ResourceStatus::Idle => ResourceStatus::Idle,
            crate::ResourceStatus::Loading => ResourceStatus::Loading,
            crate::ResourceStatus::Ready => ResourceStatus::Ready,
            crate::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
            crate::ResourceStatus::Error => ResourceStatus::Error,
        }
    }
}

/// `MetadataUpdateResult`, as the engine answers an update.
#[derive(Deserialize)]
struct UpdateResult {
    metadata: DocumentMetadata,
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Error> {
    serde_json::from_value(value).map_err(|e| {
        Error::named(
            "InvalidState",
            format!("[metadata] the engine's metadata does not decode: {e}"),
        )
        .with("code", "invalid-state")
    })
}

/// The capability, if its document is still open. The plugin keeps
/// answering from its last state after a close; a foreign handle that
/// outlived the close gets `instance-closed` instead.
fn open(cap: &MetadataCapability) -> Result<&MetadataCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(
            code::INSTANCE_CLOSED,
            "metadata",
            "the document this capability belongs to is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}

#[derive(uniffi::Object)]
pub struct MetadataHandle {
    cap: KernelBound<MetadataCapability>,
    post: KernelPost,
}

impl MetadataHandle {
    /// On the kernel thread.
    pub fn new(cap: MetadataCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(MetadataHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl MetadataHandle {
    /// The current metadata, `None` until the first read lands or while
    /// reading is forbidden (see `status`).
    pub async fn snapshot(&self) -> Result<Option<DocumentMetadata>, ViewerError> {
        self.cap.call(|c| open(c)?.get_snapshot().map(decode).transpose()).await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_status().into())).await
    }

    /// Whether this caller may edit the Info dict (`doc.metadata.modify`).
    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| open(c)?.can_edit()).await
    }

    /// Resolves once the confirmed change has reached the snapshot and every
    /// subscriber. Cancelling stops waiting for that confirmation; an engine
    /// write already sent still lands.
    pub async fn update(
        &self,
        patch: MetadataPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<DocumentMetadata, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let patch = patch.to_json()?;
                    let signal = AbortSignal::new();
                    let aborter = signal.clone();
                    Ok(open(c)?
                        .update(patch, Some(signal))
                        .with_abort(move |reason| aborter.abort(Error::abort(reason))))
                },
                |result| decode::<UpdateResult>(result).map(|r| r.metadata).map_err(Into::into),
            )
            .await?
    }

    /// Re-read from the engine. Shares a read already in flight, so
    /// cancelling only skips a read that has not started.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.refresh()), |()| ()).await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets `on_end` when the
    /// document closes (at once if it already has). An event that does not
    /// decode is reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn MetadataEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    c.on_updated(move |event| {
                        let event = MetadataUpdatedEvent::try_from(event)?;
                        listener.on_event(MetadataEvent::Updated { event });
                        Ok(())
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
