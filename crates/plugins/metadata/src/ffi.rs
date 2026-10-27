//! The metadata plugin through uniffi: typed records, one handle, one event
//! enum. Every method hops to the kernel thread through `KernelBound`.
//!
//! Migrated by hand to the current plugin (Task 13.26); compiled and tested
//! (`tests/ffi.rs`) since Task 13.33.
//!
//! The plugin keeps the engine's metadata as JSON, so the records here
//! mirror the engine's wire shape (`epdf-engine`'s `DocumentMetadata`,
//! `MetadataPatch` and `CustomMetadataPatch`) and the handle decodes into
//! them. A value that does not decode is an `invalid-state` error: the
//! record and the engine disagree, and a default would hide it.

use std::collections::HashMap;
use std::sync::Arc;

use epdf_ffi::{CancelToken, KernelBound, KernelPost, ResourceStatus, Subscription, ViewerError};
use epdf_kernel::error::{code, plugin_error};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{MetadataCapability, OperationOptions};

/// `/Trapped`: always present, so a patch can set it but not clear it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum DocumentMetadataTrapped {
    True,
    False,
    Unknown,
}

/// The Info dict's standard fields, as the engine reports them. Dates are
/// ISO 8601. The other keys are [`CustomMetadata`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub producer: Option<String>,
    pub creator: Option<String>,
    /// `/CreationDate`.
    pub created_at: Option<String>,
    /// `/ModDate`.
    pub modified_at: Option<String>,
    pub trapped: DocumentMetadataTrapped,
}

/// The Info dict's other keys, `{ key: value }`.
pub type CustomMetadata = HashMap<String, String>;

/// A text field of [`DocumentMetadata`], for [`MetadataPatch::clear`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MetadataField {
    Title,
    Author,
    Subject,
    Keywords,
    Producer,
    Creator,
    CreatedAt,
    ModifiedAt,
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
            MetadataField::CreatedAt => "createdAt",
            MetadataField::ModifiedAt => "modifiedAt",
        }
    }
}

/// A change to the standard fields. A `None` field is left alone, a value
/// sets it, and a field named in `clear` is removed (naming one that is also
/// set is refused). The custom keys change through
/// [`MetadataHandle::update_custom`].
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
    pub created_at: Option<String>,
    #[uniffi(default = None)]
    pub modified_at: Option<String>,
    #[uniffi(default = None)]
    pub trapped: Option<DocumentMetadataTrapped>,
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
            (MetadataField::CreatedAt, &self.created_at),
            (MetadataField::ModifiedAt, &self.modified_at),
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
        Ok(Value::Object(patch))
    }
}

/// The custom-key patch as the engine takes it: per key, `None` removes it.
fn custom_patch_json(patch: HashMap<String, Option<String>>) -> Value {
    Value::Object(
        patch
            .into_iter()
            .map(|(key, value)| (key, value.map_or(Value::Null, Value::String)))
            .collect(),
    )
}

/// One confirmed change of the standard fields, whoever caused it. The
/// plugin's event also carries its `EventOrigin`, which no foreign caller
/// can act on, so it is dropped.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MetadataUpdatedEvent {
    pub metadata: DocumentMetadata,
    pub previous: Option<DocumentMetadata>,
    pub changed_keys: Vec<String>,
}

/// One confirmed change of the custom keys; `changed_keys` names the keys
/// added, changed or removed.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CustomMetadataUpdatedEvent {
    pub custom: CustomMetadata,
    pub previous: Option<CustomMetadata>,
    pub changed_keys: Vec<String>,
}

// One event per call; boxing a variant would change the foreign enum.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MetadataEvent {
    Updated { event: MetadataUpdatedEvent },
    Resynced { metadata: DocumentMetadata },
    CustomUpdated { event: CustomMetadataUpdatedEvent },
    CustomResynced { custom: CustomMetadata },
}

#[uniffi::export(with_foreign)]
pub trait MetadataEventListener: Send + Sync {
    fn on_event(&self, event: MetadataEvent);
    /// The document closed: no event follows. Called once, and at once for a
    /// subscription made after the close.
    fn on_end(&self);
}

fn status(status: crate::ResourceStatus) -> ResourceStatus {
    match status {
        crate::ResourceStatus::Idle => ResourceStatus::Idle,
        crate::ResourceStatus::Loading => ResourceStatus::Loading,
        crate::ResourceStatus::Ready => ResourceStatus::Ready,
        crate::ResourceStatus::Forbidden => ResourceStatus::Forbidden,
        crate::ResourceStatus::Error => ResourceStatus::Error,
    }
}

/// `{ metadata }`, as `update` resolves.
#[derive(Deserialize)]
struct UpdateResult {
    metadata: DocumentMetadata,
}

/// `{ custom }`, as `custom.update` resolves.
#[derive(Deserialize)]
struct CustomUpdateResult {
    custom: CustomMetadata,
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

/// The options a cancellable call passes: a fresh signal, aborted with the
/// token.
fn signalled() -> (OperationOptions, AbortSignal) {
    let signal = AbortSignal::new();
    (OperationOptions { signal: Some(signal.clone()) }, signal)
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
    /// The standard fields, `None` until the first read lands or while
    /// reading is forbidden (see `status`). Nothing reads them until
    /// `ensure_loaded` or `refresh` asks.
    pub async fn snapshot(&self) -> Result<Option<DocumentMetadata>, ViewerError> {
        self.cap
            .call(|c| open(c)?.get_snapshot().map(|value| decode((*value).clone())).transpose())
            .await
    }

    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|c| Ok(status(open(c)?.get_status()))).await
    }

    /// The custom keys, `None` until the first read lands.
    pub async fn custom_snapshot(&self) -> Result<Option<CustomMetadata>, ViewerError> {
        self.cap
            .call(|c| {
                open(c)?.custom().get_snapshot().map(|value| decode((*value).clone())).transpose()
            })
            .await
    }

    pub async fn custom_status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|c| Ok(status(open(c)?.custom().get_status()))).await
    }

    /// Read the standard fields and the custom keys, unless loaded or
    /// loading. Cancelling rejects this call; the shared reads still land.
    pub async fn ensure_loaded(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (options, signal) = signalled();
                    Ok(open(c)?
                        .ensure_loaded(options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }

    /// Whether this caller may change the Info dict (`doc.metadata.modify`),
    /// standard fields and custom keys alike.
    pub async fn can_update(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_update())).await
    }

    /// Resolves once the confirmed change has reached the snapshot and every
    /// subscriber. Cancelling before the write starts leaves the document
    /// alone; a write already sent still lands.
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
                    let (options, signal) = signalled();
                    Ok(open(c)?
                        .update(patch, options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |result| decode::<UpdateResult>(result).map(|r| r.metadata).map_err(Into::into),
            )
            .await?
    }

    /// Set (`Some`) and remove (`None`) custom keys; a key left out stays.
    pub async fn update_custom(
        &self,
        patch: HashMap<String, Option<String>>,
        cancel: Arc<CancelToken>,
    ) -> Result<CustomMetadata, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let (options, signal) = signalled();
                    Ok(open(c)?
                        .custom()
                        .update(custom_patch_json(patch), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |result| decode::<CustomUpdateResult>(result).map(|r| r.custom).map_err(Into::into),
            )
            .await?
    }

    /// Re-read the standard fields. During a read in flight this joins it
    /// and queues one more read.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (options, signal) = signalled();
                    Ok(open(c)?
                        .refresh(options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }

    /// Re-read the custom keys.
    pub async fn refresh_custom(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| {
                    let (options, signal) = signalled();
                    Ok(open(c)?
                        .custom()
                        .refresh(options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }

    /// The listener is registered on the kernel thread after this returns;
    /// a cancel before then still takes effect. It gets every updated and
    /// resynced event of both halves, then `on_end` when the document closes
    /// (at once if it already has). An event that does not decode is
    /// reported through the kernel's report sink, not delivered.
    pub fn subscribe(&self, listener: Arc<dyn MetadataEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(
                c.lifetime(),
                || {
                    let custom = c.custom();
                    let to = listener.clone();
                    let updated = c.on_updated(move |event| {
                        let event = MetadataUpdatedEvent {
                            metadata: decode((*event.metadata).clone())?,
                            previous: event
                                .previous
                                .as_ref()
                                .map(|p| decode((**p).clone()))
                                .transpose()?,
                            changed_keys: event.changed_keys.clone(),
                        };
                        to.on_event(MetadataEvent::Updated { event });
                        Ok(())
                    });
                    let to = listener.clone();
                    let resynced = c.on_resynced(move |event| {
                        let metadata = decode((*event.metadata).clone())?;
                        to.on_event(MetadataEvent::Resynced { metadata });
                        Ok(())
                    });
                    let to = listener.clone();
                    let custom_updated = custom.on_updated(move |event| {
                        let event = CustomMetadataUpdatedEvent {
                            custom: decode((*event.custom).clone())?,
                            previous: event
                                .previous
                                .as_ref()
                                .map(|p| decode((**p).clone()))
                                .transpose()?,
                            changed_keys: event.changed_keys.clone(),
                        };
                        to.on_event(MetadataEvent::CustomUpdated { event });
                        Ok(())
                    });
                    let to = listener.clone();
                    let custom_resynced = custom.on_resynced(move |event| {
                        let custom = decode((*event.custom).clone())?;
                        to.on_event(MetadataEvent::CustomResynced { custom });
                        Ok(())
                    });
                    Unsubscribe::new(move || {
                        for off in [updated, resynced, custom_updated, custom_resynced] {
                            off.call();
                        }
                    })
                },
                move || ending.on_end(),
            );
        });
        subscription
    }
}
