//! `services/events.ts`: the capability's events, minted on the instance so
//! they are disposed with it. The state-change events (`onTargetChanged`,
//! `onInvalidationPredicted`) are derived here, in one place, from each
//! committed state change.

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Error, Unsubscribe};
use serde_json::Value;

use crate::contract::*;
use crate::controller::{SignatureCapability, SignatureContext};
use crate::model::newly_invalidated;

pub(crate) struct SignatureEvents {
    pub signed: EventHookSource<SignatureSignedEvent>,
    pub filled: EventHookSource<SignatureFilledEvent>,
    pub cleared: EventHookSource<SignatureClearedEvent>,
    pub validated: EventHookSource<SignatureValidatedEvent>,
    pub protection_changed: EventHookSource<SignatureProtectionChangedEvent>,
    pub invalidation_predicted: EventHookSource<SignatureInvalidationPredictedEvent>,
    pub target_changed: EventHookSource<SignatureTargetChangedEvent>,
    pub sign_requested: EventHookSource<SignatureSignRequestedEvent>,
    pub inspection_requested: EventHookSource<SignatureInspectionRequestedEvent>,
}

/// `createEvents(ctx)`.
pub(crate) fn create_events(cx: &SignatureContext) -> SignatureEvents {
    let events = SignatureEvents {
        signed: cx.event_source(),
        filled: cx.event_source(),
        cleared: cx.event_source(),
        validated: cx.event_source(),
        protection_changed: cx.event_source(),
        invalidation_predicted: cx.event_source(),
        target_changed: cx.event_source(),
        sign_requested: cx.event_source(),
        inspection_requested: cx.event_source(),
    };
    let (target_changed, invalidation_predicted) =
        (events.target_changed.clone(), events.invalidation_predicted.clone());
    cx.on_state_change(move |next, previous| {
        if !same(&previous.target, &next.target) {
            let field = next.target.as_ref().map_or(Value::Null, |t| (**t).clone());
            target_changed.emit(&SignatureTargetChangedEvent { field });
        }
        if !same(&previous.verdicts, &next.verdicts)
            && let Some(verdicts) = &next.verdicts
        {
            // Acrobat's warning, after the fact and only on the edge: a
            // signature that held (or was never judged) now reads invalid
            // because of unsaved edits.
            for verdict in newly_invalidated(previous.verdicts.as_deref(), verdicts) {
                let field = verdict
                    .get("signature")
                    .and_then(|s| s.get("field"))
                    .cloned()
                    .unwrap_or_else(epdf_kernel::undefined);
                let detail = verdict
                    .get("modifications")
                    .and_then(|m| m.get("detail"))
                    .filter(|d| !d.is_null() && !epdf_kernel::is_undefined(d))
                    .map(|d| crate::js::template(Some(d)))
                    .unwrap_or_default();
                invalidation_predicted.emit(&SignatureInvalidationPredictedEvent { field, detail });
            }
        }
        Ok(())
    });
    events
}

fn same(a: &Option<std::rc::Rc<Value>>, b: &Option<std::rc::Rc<Value>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => std::rc::Rc::ptr_eq(a, b),
        _ => false,
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
        /// `onSigned(listener)`: a field was signed, whoever signed it.
        on_signed, signed, SignatureSignedEvent
    );
    hook!(
        /// `onFilled(listener)`: a mark was drawn into a field.
        on_filled, filled, SignatureFilledEvent
    );
    hook!(
        /// `onCleared(listener)`: a drawn mark was taken out of a field.
        on_cleared, cleared, SignatureClearedEvent
    );
    hook!(
        /// `onValidated(listener)`: the signatures were checked.
        on_validated, validated, SignatureValidatedEvent
    );
    hook!(
        /// `onProtectionChanged(listener)`: what the signatures allow
        /// changed.
        on_protection_changed, protection_changed, SignatureProtectionChangedEvent
    );
    hook!(
        /// `onInvalidationPredicted(listener)`: a change would break a
        /// signature once saved.
        on_invalidation_predicted, invalidation_predicted, SignatureInvalidationPredictedEvent
    );
    hook!(
        /// `onTargetChanged(listener)`.
        on_target_changed, target_changed, SignatureTargetChangedEvent
    );
    hook!(
        /// `onSignRequested(listener)`: in `ask` mode, a mark was dropped on
        /// a field.
        on_sign_requested, sign_requested, SignatureSignRequestedEvent
    );
    hook!(
        /// `onInspectionRequested(listener)`: a signed field was clicked.
        on_inspection_requested, inspection_requested, SignatureInspectionRequestedEvent
    );
}
