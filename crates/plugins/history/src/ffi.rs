//! History submission and confirmation across the kernel-thread boundary.
use crate::{HistoryCapability, HistoryConfig};
use epdf_ffi::{KernelBound, KernelPost, PluginErrorInfo, Subscription, ViewerError};
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum HistoryLabelValue {
    Text { value: String },
    Number { value: f64 },
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HistoryChangeLabel {
    pub key: String,
    pub values: HashMap<String, HistoryLabelValue>,
}
impl HistoryChangeLabel {
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let invalid = || {
            Error::named(
                "InvalidState",
                "history label must contain a key and string/number values",
            )
            .with("code", "invalid-state")
        };
        let object = value.as_object().ok_or_else(invalid)?;
        let key = object.get("key").and_then(Value::as_str).ok_or_else(invalid)?.to_owned();
        let values = object
            .iter()
            .filter(|(key, _)| key.as_str() != "key")
            .map(|(key, value)| {
                let value = match value {
                    Value::String(value) => HistoryLabelValue::Text { value: value.clone() },
                    Value::Number(value) => {
                        HistoryLabelValue::Number { value: value.as_f64().ok_or_else(invalid)? }
                    }
                    _ => return Err(invalid()),
                };
                Ok((key.clone(), value))
            })
            .collect::<Result<_, Error>>()?;
        Ok(Self { key, values })
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HistorySettings {
    pub limit: f64,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct HistorySettingsPatch {
    #[uniffi(default = None)]
    pub limit: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum HistoryUndoFailure {
    Unavailable,
    Refused,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum HistoryEvent {
    Undone {
        label: HistoryChangeLabel,
        redo: bool,
        skipped: u64,
    },
    UndoFailed {
        label: HistoryChangeLabel,
        redo: bool,
        reason: HistoryUndoFailure,
        error: PluginErrorInfo,
    },
    SettingsChanged {
        settings: HistorySettings,
        changed: Vec<String>,
    },
}
#[uniffi::export(with_foreign)]
pub trait HistoryEventListener: Send + Sync {
    fn on_event(&self, event: HistoryEvent);
    fn on_end(&self);
}
fn open(cap: &HistoryCapability) -> Result<&HistoryCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "history",
            "the document is closed",
            None,
            None,
        ));
    }
    Ok(cap)
}
#[derive(uniffi::Object)]
pub struct HistoryHandle {
    cap: KernelBound<HistoryCapability>,
    post: KernelPost,
}
impl HistoryHandle {
    pub fn new(cap: HistoryCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(Self { cap: KernelBound::new(cap, post.clone()), post })
    }
}
#[uniffi::export]
impl HistoryHandle {
    pub async fn can_undo(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.can_undo())).await
    }
    pub async fn can_redo(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.can_redo())).await
    }
    pub async fn undo_label(&self) -> Result<Option<HistoryChangeLabel>, ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.get_undo_label().as_ref().map(HistoryChangeLabel::from_json).transpose()
            })
            .await
    }
    pub async fn redo_label(&self) -> Result<Option<HistoryChangeLabel>, ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.get_redo_label().as_ref().map(HistoryChangeLabel::from_json).transpose()
            })
            .await
    }
    /// Submits undo; confirmation or refusal follows through events. Empty history is a no-op.
    pub async fn undo(&self) -> Result<(), ViewerError> {
        self.cap.call(|cap| open(cap)?.undo()).await
    }
    pub async fn redo(&self) -> Result<(), ViewerError> {
        self.cap.call(|cap| open(cap)?.redo()).await
    }
    pub async fn clear(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.clear();
                Ok(())
            })
            .await
    }
    pub async fn settings(&self) -> Result<HistorySettings, ViewerError> {
        self.cap.call(|cap| Ok(HistorySettings { limit: open(cap)?.get_settings().limit })).await
    }
    pub async fn update_settings(&self, patch: HistorySettingsPatch) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.update_settings(&HistoryConfig { limit: patch.limit });
                Ok(())
            })
            .await
    }
    pub async fn reset_settings(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.reset_settings();
                Ok(())
            })
            .await
    }
    pub fn subscribe(&self, listener: Arc<dyn HistoryEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |cap| {
            let ending = listener.clone();
            slot.fill_until_end(
                cap.lifetime(),
                || {
                    let to = listener.clone();
                    let undone = cap.on_undone(move |event| {
                        to.on_event(HistoryEvent::Undone {
                            label: HistoryChangeLabel::from_json(&event.label)?,
                            redo: event.redo,
                            skipped: event.skipped,
                        });
                        Ok(())
                    });
                    let to = listener.clone();
                    let failed = cap.on_undo_failed(move |event| {
                        to.on_event(HistoryEvent::UndoFailed {
                            label: HistoryChangeLabel::from_json(&event.label)?,
                            redo: event.redo,
                            reason: match event.reason {
                                crate::UndoFailure::Unavailable => HistoryUndoFailure::Unavailable,
                                crate::UndoFailure::Refused => HistoryUndoFailure::Refused,
                            },
                            error: PluginErrorInfo::of("history", &event.error),
                        });
                        Ok(())
                    });
                    let settings = cap.on_settings_changed(move |event| {
                        listener.on_event(HistoryEvent::SettingsChanged {
                            settings: HistorySettings { limit: event.settings.limit },
                            changed: event.changed_keys.clone(),
                        });
                        Ok(())
                    });
                    Unsubscribe::new(move || {
                        for off in [undone, failed, settings] {
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
