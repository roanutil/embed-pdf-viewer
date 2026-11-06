//! Typed outline records and the kernel-thread capability boundary.
use crate::OutlineCapability;
use epdf_ffi::document::DocumentEventOrigin;
use epdf_ffi::{
    CancelToken, KernelBound, KernelPost, PageLinkTarget, ResourceStatus, Subscription, ViewerError,
};
use epdf_kernel::signal::AbortSignal;
use epdf_kernel::{Error, Unsubscribe};
use epdf_plugin_actions::ffi::PdfActionTree;
use epdf_plugin_link::ffi::LinkActivation;
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
        .ok_or_else(|| invalid(format!("outline {field} is not text")))
}
fn flag(value: &Value, field: &str) -> Result<bool, Error> {
    value
        .get(field)
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid(format!("outline {field} is not boolean")))
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OutlineItemRef {
    pub object_number: u32,
}
impl OutlineItemRef {
    pub fn to_json(&self) -> Value {
        json!({"kind":"objectNumber","objectNumber":self.object_number})
    }
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        if value.get("kind").and_then(Value::as_str) != Some("objectNumber") {
            return Err(invalid("outline ref has an unknown kind"));
        }
        let number = value
            .get("objectNumber")
            .and_then(Value::as_f64)
            .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= f64::from(u32::MAX))
            .ok_or_else(|| invalid("outline object number is not a whole UInt32"))?;
        Ok(Self { object_number: number as u32 })
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OutlineItem {
    pub item_ref: OutlineItemRef,
    pub parent: Option<OutlineItemRef>,
    pub title: String,
    pub target: Option<PageLinkTarget>,
    pub action: Option<Arc<PdfActionTree>>,
    pub open: bool,
    pub color: Option<String>,
    pub bold: bool,
    pub italic: bool,
}
impl OutlineItem {
    pub fn from_json(value: &Value) -> Result<Self, Error> {
        let optional = |field: &str| value.get(field).filter(|value| !value.is_null());
        Ok(Self {
            item_ref: OutlineItemRef::from_json(&value["ref"])?,
            parent: optional("parent").map(OutlineItemRef::from_json).transpose()?,
            title: text(value, "title")?,
            target: optional("target").map(PageLinkTarget::from_json).transpose()?,
            action: optional("action")
                .map(|tree| {
                    PdfActionTree::from_json(tree.to_string())
                        .map_err(|error| invalid(error.to_string()))
                })
                .transpose()?,
            open: flag(value, "open")?,
            color: optional("color")
                .map(|color| {
                    color
                        .as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("outline color is not text"))
                })
                .transpose()?,
            bold: flag(value, "bold")?,
            italic: flag(value, "italic")?,
        })
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OutlineRow {
    pub item: OutlineItem,
    pub depth: u32,
    pub has_children: bool,
    pub expanded: bool,
}
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OutlineSnapshot {
    pub items: Vec<OutlineItem>,
    pub incomplete: bool,
}
fn items(values: &[Value]) -> Result<Vec<OutlineItem>, Error> {
    values.iter().map(OutlineItem::from_json).collect()
}
fn snapshot(value: &Value) -> Result<OutlineSnapshot, Error> {
    Ok(OutlineSnapshot {
        items: items(
            value
                .get("items")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("outline snapshot has no items"))?,
        )?,
        incomplete: flag(value, "incomplete")?,
    })
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum OutlinePosition {
    Start,
    End,
    Before { item: OutlineItemRef },
    After { item: OutlineItemRef },
}
impl OutlinePosition {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Start => json!("start"),
            Self::End => json!("end"),
            Self::Before { item } => json!({"before":item.to_json()}),
            Self::After { item } => json!({"after":item.to_json()}),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct OutlineItemInput {
    #[uniffi(default = None)]
    pub title: Option<String>,
    #[uniffi(default = None)]
    pub parent: Option<OutlineItemRef>,
    #[uniffi(default = None)]
    pub position: Option<OutlinePosition>,
    #[uniffi(default = None)]
    pub target: Option<PageLinkTarget>,
    /// Explicitly choose no target; otherwise an omitted target uses the current view.
    #[uniffi(default = false)]
    pub clear_target: bool,
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub bold: Option<bool>,
    #[uniffi(default = None)]
    pub italic: Option<bool>,
}
impl OutlineItemInput {
    pub fn to_json(self) -> Value {
        let mut value = json!({});
        if let Some(title) = self.title {
            value["title"] = title.into();
        }
        if let Some(parent) = self.parent {
            value["parent"] = parent.to_json();
        }
        if let Some(position) = self.position {
            value["position"] = position.to_json();
        }
        if let Some(target) = self.target {
            value["target"] = target.to_json();
        }
        if self.clear_target {
            value["target"] = Value::Null;
        }
        if let Some(color) = self.color {
            value["color"] = color.into();
        }
        if let Some(bold) = self.bold {
            value["bold"] = bold.into();
        }
        if let Some(italic) = self.italic {
            value["italic"] = italic.into();
        }
        value
    }
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum OutlinePatchKey {
    Target,
    Color,
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct OutlineItemPatch {
    #[uniffi(default = None)]
    pub title: Option<String>,
    #[uniffi(default = None)]
    pub target: Option<PageLinkTarget>,
    #[uniffi(default = None)]
    pub color: Option<String>,
    #[uniffi(default = None)]
    pub bold: Option<bool>,
    #[uniffi(default = None)]
    pub italic: Option<bool>,
    #[uniffi(default = None)]
    pub open: Option<bool>,
    #[uniffi(default = [])]
    pub clear: Vec<OutlinePatchKey>,
}
impl OutlineItemPatch {
    pub fn to_json(self) -> Value {
        let mut value = OutlineItemInput {
            title: self.title,
            target: self.target,
            color: self.color,
            bold: self.bold,
            italic: self.italic,
            ..Default::default()
        }
        .to_json();
        if let Some(open) = self.open {
            value["open"] = open.into();
        }
        for field in self.clear {
            value[match field {
                OutlinePatchKey::Target => "target",
                OutlinePatchKey::Color => "color",
            }] = Value::Null;
        }
        value
    }
}
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct OutlineCreateOptions {
    #[uniffi(default = false)]
    pub select: bool,
}
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum OutlineEvent {
    Changed { kind: String, items: Vec<OutlineItemRef>, origin: DocumentEventOrigin },
    Resynced { outline: OutlineSnapshot },
}
#[uniffi::export(with_foreign)]
pub trait OutlineEventListener: Send + Sync {
    fn on_event(&self, event: OutlineEvent);
    fn on_end(&self);
}
fn open(cap: &OutlineCapability) -> Result<&OutlineCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(epdf_kernel::error::plugin_error(
            "instance-closed",
            "outline",
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
fn refs(values: &[Value]) -> Result<Vec<OutlineItemRef>, Error> {
    values.iter().map(OutlineItemRef::from_json).collect()
}
fn ref_values(values: Vec<OutlineItemRef>) -> Vec<Value> {
    values.iter().map(OutlineItemRef::to_json).collect()
}
fn options() -> (crate::OperationOptions, AbortSignal) {
    let signal = AbortSignal::new();
    (crate::OperationOptions { signal: Some(signal.clone()) }, signal)
}
#[derive(uniffi::Object)]
pub struct OutlineHandle {
    cap: KernelBound<OutlineCapability>,
    post: KernelPost,
}
impl OutlineHandle {
    pub fn new(cap: OutlineCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(Self { cap: KernelBound::new(cap, post.clone()), post })
    }
}
#[uniffi::export]
impl OutlineHandle {
    pub async fn list(&self) -> Result<Vec<OutlineItem>, ViewerError> {
        self.cap.call(|cap| items(&open(cap)?.list())).await
    }
    pub async fn item(&self, item: OutlineItemRef) -> Result<Option<OutlineItem>, ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.get(&item.to_json()).as_ref().map(OutlineItem::from_json).transpose()
            })
            .await
    }
    pub async fn children(
        &self,
        parent: Option<OutlineItemRef>,
    ) -> Result<Vec<OutlineItem>, ViewerError> {
        self.cap
            .call(move |cap| {
                items(
                    &open(cap)?.get_children(parent.as_ref().map(OutlineItemRef::to_json).as_ref()),
                )
            })
            .await
    }
    pub async fn rows(&self) -> Result<Vec<OutlineRow>, ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?
                    .list_rows()
                    .iter()
                    .map(|row| {
                        Ok(OutlineRow {
                            item: OutlineItem::from_json(&row.item)?,
                            depth: row.depth,
                            has_children: row.has_children,
                            expanded: row.expanded,
                        })
                    })
                    .collect()
            })
            .await
    }
    pub async fn current_item(&self) -> Result<Option<OutlineItemRef>, ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.get_current_item().as_ref().map(OutlineItemRef::from_json).transpose()
            })
            .await
    }
    pub async fn selected_items(&self) -> Result<Vec<OutlineItemRef>, ViewerError> {
        self.cap.call(|cap| refs(&open(cap)?.get_selected())).await
    }
    pub async fn select(&self, items: Vec<OutlineItemRef>) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.select(&ref_values(items));
                Ok(())
            })
            .await
    }
    pub async fn status(&self) -> Result<ResourceStatus, ViewerError> {
        self.cap.call(|cap| Ok(status(open(cap)?.get_status()))).await
    }
    pub async fn is_incomplete(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.is_incomplete())).await
    }
    pub async fn is_expanded(&self, item: OutlineItemRef) -> Result<bool, ViewerError> {
        self.cap.call(move |cap| Ok(open(cap)?.is_expanded(&item.to_json()))).await
    }
    pub async fn can_edit(&self) -> Result<bool, ViewerError> {
        self.cap.call(|cap| Ok(open(cap)?.can_edit())).await
    }
    pub async fn current_view_target(&self) -> Result<Option<PageLinkTarget>, ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?
                    .get_current_view_target()
                    .as_ref()
                    .map(PageLinkTarget::from_json)
                    .transpose()
            })
            .await
    }
    pub async fn activate(&self, item: OutlineItemRef) -> Result<LinkActivation, ViewerError> {
        self.cap
            .call(move |cap| Ok(LinkActivation::from(&open(cap)?.activate(&item.to_json())?)))
            .await
    }
    pub async fn expand(&self, item: OutlineItemRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.expand(&item.to_json());
                Ok(())
            })
            .await
    }
    pub async fn collapse(&self, item: OutlineItemRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.collapse(&item.to_json());
                Ok(())
            })
            .await
    }
    pub async fn toggle(&self, item: OutlineItemRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.toggle(&item.to_json());
                Ok(())
            })
            .await
    }
    pub async fn reveal(&self, item: OutlineItemRef) -> Result<(), ViewerError> {
        self.cap
            .call(move |cap| {
                open(cap)?.reveal(&item.to_json());
                Ok(())
            })
            .await
    }
    pub async fn expand_all(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.expand_all();
                Ok(())
            })
            .await
    }
    pub async fn collapse_all(&self) -> Result<(), ViewerError> {
        self.cap
            .call(|cap| {
                open(cap)?.collapse_all();
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
    pub async fn create(
        &self,
        input: OutlineItemInput,
        options: OutlineCreateOptions,
        cancel: Arc<CancelToken>,
    ) -> Result<OutlineItem, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let signal = AbortSignal::new();
                    let options = crate::OutlineCreateOptions {
                        select: options.select,
                        signal: Some(signal.clone()),
                    };
                    Ok(open(cap)?
                        .create(input.to_json(), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |result| OutlineItem::from_json(&result["item"]).map_err(ViewerError::from),
            )
            .await?
    }
    pub async fn update(
        &self,
        item: OutlineItemRef,
        patch: OutlineItemPatch,
        cancel: Arc<CancelToken>,
    ) -> Result<OutlineItem, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .update(item.to_json(), patch.to_json(), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |result| OutlineItem::from_json(&result["item"]).map_err(ViewerError::from),
            )
            .await?
    }
    pub async fn move_items(
        &self,
        items: Vec<OutlineItemRef>,
        parent: Option<OutlineItemRef>,
        position: OutlinePosition,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .move_items(
                            ref_values(items),
                            parent.as_ref().map(OutlineItemRef::to_json).unwrap_or(Value::Null),
                            position.to_json(),
                            options,
                        )
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }
    pub async fn delete_items(
        &self,
        items: Vec<OutlineItemRef>,
        cancel: Arc<CancelToken>,
    ) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |cap| {
                    let (options, signal) = options();
                    Ok(open(cap)?
                        .delete(ref_values(items), options)
                        .with_abort(move |reason| signal.abort(Error::abort(reason))))
                },
                |()| (),
            )
            .await
    }
    pub fn subscribe(&self, listener: Arc<dyn OutlineEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |cap| {
            let ending = listener.clone();
            slot.fill_until_end(
                cap.lifetime(),
                || {
                    let to = listener.clone();
                    let changed = cap.on_changed(move |event| {
                        to.on_event(OutlineEvent::Changed {
                            kind: event.kind.into(),
                            items: refs(&event.items)?,
                            origin: DocumentEventOrigin::from_json(&event.origin)?,
                        });
                        Ok(())
                    });
                    let resynced = cap.on_resynced(move |event| {
                        listener.on_event(OutlineEvent::Resynced {
                            outline: snapshot(&event.outline)?,
                        });
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
