//! `services/events.ts`: the nine change hooks, disposed with the plugin.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, Error};
use serde_json::{Value, json};

use crate::contract::*;
use crate::model::{SignatureAction, SignatureState};

/// `ORIGIN_API`: a change this session made through the public API.
pub fn origin_api() -> Value {
    json!({ "locality": "local", "trigger": "api", "sessionId": null, "actorId": null })
}

pub(crate) struct SignatureEvents {
    pub signed: EventHookSource<SignatureSignedEvent>,
    pub filled: EventHookSource<SignatureFieldEvent>,
    pub cleared: EventHookSource<SignatureFieldEvent>,
    pub validated: EventHookSource<SignatureValidatedEvent>,
    pub protection_changed: EventHookSource<SignatureProtectionChangedEvent>,
    pub invalidating: EventHookSource<SignatureInvalidatingEvent>,
    pub target_changed: EventHookSource<SignatureTargetChangedEvent>,
    pub sign_requested: EventHookSource<SignatureSignRequestedEvent>,
    pub inspection_requested: EventHookSource<SignatureInspectionRequestedEvent>,
}

impl SignatureEvents {
    /// `createEvents(ctx)`: a failing listener is reported to the console.
    pub(crate) fn new(
        cx: &Controller<SignatureState, SignatureAction>,
        console_error: Option<Rc<dyn Fn(&str, &Error)>>,
    ) -> Self {
        let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
            if let Some(sink) = &console_error {
                sink("[signature] listener failed:", error);
            }
        });
        let events = SignatureEvents {
            signed: EventHookSource::new(Some(report.clone())),
            filled: EventHookSource::new(Some(report.clone())),
            cleared: EventHookSource::new(Some(report.clone())),
            validated: EventHookSource::new(Some(report.clone())),
            protection_changed: EventHookSource::new(Some(report.clone())),
            invalidating: EventHookSource::new(Some(report.clone())),
            target_changed: EventHookSource::new(Some(report.clone())),
            sign_requested: EventHookSource::new(Some(report.clone())),
            inspection_requested: EventHookSource::new(Some(report)),
        };
        let (a, b, c, d, e, f, g, h, i) = (
            events.signed.clone(),
            events.filled.clone(),
            events.cleared.clone(),
            events.validated.clone(),
            events.protection_changed.clone(),
            events.invalidating.clone(),
            events.target_changed.clone(),
            events.sign_requested.clone(),
            events.inspection_requested.clone(),
        );
        cx.defer(Box::new(move || {
            a.dispose();
            b.dispose();
            c.dispose();
            d.dispose();
            e.dispose();
            f.dispose();
            g.dispose();
            h.dispose();
            i.dispose();
            Ok(None)
        }));
        events
    }
}
