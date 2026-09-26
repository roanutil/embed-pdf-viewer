//! `services/events.ts`: the two change hooks, disposed with the plugin. A
//! throwing listener goes to `console.error('[redaction] listener failed:',
//! error)` and the next listener still runs.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, Error};
use serde_json::{Value, json};

use crate::contract::{RedactionAppliedEvent, RedactionHost, RedactionPendingChangedEvent};
use crate::model::{RedactionAction, RedactionState};

/// A change this session made through the public API.
pub fn origin_api() -> Value {
    json!({ "locality": "local", "trigger": "api", "sessionId": null, "actorId": null })
}

pub(crate) struct RedactionEvents {
    pub applied: EventHookSource<RedactionAppliedEvent>,
    pub pending_changed: EventHookSource<RedactionPendingChangedEvent>,
}

/// `createEvents(ctx)`.
pub(crate) fn create_events(
    cx: &Controller<RedactionState, RedactionAction>,
    host: &RedactionHost,
) -> RedactionEvents {
    let console_error = host.console_error.clone();
    let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
        if let Some(sink) = &console_error {
            sink("[redaction] listener failed:", error);
        }
    });
    let events = RedactionEvents {
        applied: EventHookSource::new(Some(report.clone())),
        pending_changed: EventHookSource::new(Some(report)),
    };
    let (applied, pending_changed) = (events.applied.clone(), events.pending_changed.clone());
    cx.defer(Box::new(move || {
        applied.dispose();
        pending_changed.dispose();
        Ok(None)
    }));
    events
}
