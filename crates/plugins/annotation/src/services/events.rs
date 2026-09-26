//! `services/events.ts`: the confirmed-change events, one emit per
//! confirmed fact. A failing listener is reported through `console.error`
//! (`[annotation] event listener failed:`), never to its siblings.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, Error};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::DeferFn;
use crate::host_contract::CapturedAnnotationDraft;
use crate::model::{AnnotationAction, AnnotationState};

/// `ORIGIN_API`: a change this session made through the public API.
pub fn origin_api() -> Value {
    json!({ "locality": "local", "trigger": "api", "sessionId": null, "actorId": null })
}

/// `ORIGIN_UNKNOWN`: a change this session made through a gesture or an
/// internal path.
pub fn origin_unknown() -> Value {
    json!({ "locality": "local", "trigger": "unknown", "sessionId": null, "actorId": null })
}

/// `originOf(event)`. `sessionId` is the event's own member, so a missing
/// one stays missing, as the TypeScript's object literal leaves it.
pub fn origin_of(event: &Value) -> Value {
    let origin = event.get("origin").cloned().unwrap_or(Value::Null);
    let remote = origin.get("kind").and_then(Value::as_str) == Some("remote");
    let mut out = serde_json::Map::new();
    out.insert("locality".into(), json!(if remote { "remote" } else { "local" }));
    out.insert("trigger".into(), json!("unknown"));
    if let Some(session) = origin.get("sessionId").filter(|s| !epdf_kernel::is_undefined(s)) {
        out.insert("sessionId".into(), session.clone());
    }
    out.insert(
        "actorId".into(),
        origin
            .get("sub")
            .filter(|s| !s.is_null() && !epdf_kernel::is_undefined(s))
            .cloned()
            .unwrap_or(Value::Null),
    );
    Value::Object(out)
}

/// `createAnnotationEvents(ctx)`.
pub(crate) struct AnnotationEvents {
    pub created: EventHookSource<AnnotationCreatedEvent>,
    pub updated: EventHookSource<AnnotationUpdatedEvent>,
    pub deleted: EventHookSource<AnnotationDeletedEvent>,
    pub resynced: EventHookSource<AnnotationResyncedEvent>,
    pub selection_changed: EventHookSource<AnnotationSelectionChangedEvent>,
    pub draft_changed: EventHookSource<AnnotationDraftChangedEvent>,
    pub editing_changed: EventHookSource<AnnotationEditingChangedEvent>,
    pub thread_changed: EventHookSource<CommentThreadChangedEvent>,
    pub draft_captured: EventHookSource<CapturedAnnotationDraft>,
}

impl AnnotationEvents {
    pub fn new(
        cx: &Controller<AnnotationState, AnnotationAction>,
        console_error: Option<Rc<dyn Fn(&str, Option<&Error>)>>,
    ) -> Self {
        let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
            if let Some(sink) = &console_error {
                sink("[annotation] event listener failed:", Some(error));
            }
        });
        let events = AnnotationEvents {
            created: EventHookSource::new(Some(report.clone())),
            updated: EventHookSource::new(Some(report.clone())),
            deleted: EventHookSource::new(Some(report.clone())),
            resynced: EventHookSource::new(Some(report.clone())),
            selection_changed: EventHookSource::new(Some(report.clone())),
            draft_changed: EventHookSource::new(Some(report.clone())),
            editing_changed: EventHookSource::new(Some(report.clone())),
            thread_changed: EventHookSource::new(Some(report.clone())),
            draft_captured: EventHookSource::new(Some(report)),
        };
        let all = (
            events.created.clone(),
            events.updated.clone(),
            events.deleted.clone(),
            events.resynced.clone(),
            events.selection_changed.clone(),
            events.draft_changed.clone(),
            events.editing_changed.clone(),
            events.thread_changed.clone(),
            events.draft_captured.clone(),
        );
        cx.defer_fn(move || {
            all.0.dispose();
            all.1.dispose();
            all.2.dispose();
            all.3.dispose();
            all.4.dispose();
            all.5.dispose();
            all.6.dispose();
            all.7.dispose();
            all.8.dispose();
        });
        events
    }
}

impl crate::controller::AnnotationCapability {
    /// `onCreated(listener)`: a confirmed creation, whoever caused it.
    pub fn on_created(
        &self,
        listener: impl Fn(&AnnotationCreatedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.created.on(listener)
    }

    /// `onUpdated(listener)`.
    pub fn on_updated(
        &self,
        listener: impl Fn(&AnnotationUpdatedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.updated.on(listener)
    }

    /// `onDeleted(listener)`.
    pub fn on_deleted(
        &self,
        listener: impl Fn(&AnnotationDeletedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.deleted.on(listener)
    }

    /// `onSelectionChanged(listener)`.
    pub fn on_selection_changed(
        &self,
        listener: impl Fn(&AnnotationSelectionChangedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.selection_changed.on(listener)
    }

    /// `onDraftChanged(listener)`.
    pub fn on_draft_changed(
        &self,
        listener: impl Fn(&AnnotationDraftChangedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.draft_changed.on(listener)
    }

    /// `onEditingChanged(listener)`.
    pub fn on_editing_changed(
        &self,
        listener: impl Fn(&AnnotationEditingChangedEvent) -> Result<(), Error> + 'static,
    ) -> epdf_kernel::Unsubscribe {
        self.0.events.editing_changed.on(listener)
    }
}
