//! `services/events.ts`: the nine change hooks, disposed with the plugin.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::{Value, json};

use crate::contract::*;
use crate::controller::SignatureCapability;
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

macro_rules! hook {
    ($(#[$doc:meta])* $name:ident, $field:ident, $event:ty) => {
        $(#[$doc])*
        pub fn $name(
            &self,
            listener: impl Fn(&$event) -> Result<(), Error> + 'static,
        ) -> Unsubscribe {
            self.0.events.$field.on(listener)
        }
    };
}

impl SignatureCapability {
    hook!(
        /// `onSigned(listener)`: a field was signed.
        on_signed, signed, SignatureSignedEvent
    );
    hook!(
        /// `onFilled(listener)`: a visual mark was drawn into a field.
        on_filled, filled, SignatureFieldEvent
    );
    hook!(
        /// `onCleared(listener)`: a field's visual mark was removed.
        on_cleared, cleared, SignatureFieldEvent
    );
    hook!(
        /// `onValidated(listener)`.
        on_validated, validated, SignatureValidatedEvent
    );
    hook!(
        /// `onProtectionChanged(listener)`.
        on_protection_changed, protection_changed, SignatureProtectionChangedEvent
    );
    hook!(
        /// `onInvalidating(listener)`: an unsaved edit just turned a
        /// signature that held into one a save would invalidate.
        on_invalidating, invalidating, SignatureInvalidatingEvent
    );
    hook!(
        /// `onTargetChanged(listener)`.
        on_target_changed, target_changed, SignatureTargetChangedEvent
    );
    hook!(
        /// `onSignRequested(listener)`: mode `ask`, a mark met a field and
        /// the host decides (opens its dialog, then signs or fills).
        on_sign_requested, sign_requested, SignatureSignRequestedEvent
    );
    hook!(
        /// `onInspectionRequested(listener)`: a signed field was activated.
        on_inspection_requested, inspection_requested, SignatureInspectionRequestedEvent
    );
}
