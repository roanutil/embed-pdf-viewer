//! `services/events.ts`: the capability's two event sources, disposed with
//! the instance. A throwing listener goes to the kernel's report and the
//! next listener still runs.

use epdf_kernel::ControllerContext;
use epdf_kernel::event_hook::EventHookSource;

use crate::contract::{RedactionAppliedEvent, RedactionPendingChangedEvent};

pub struct RedactionEvents {
    pub applied: EventHookSource<RedactionAppliedEvent>,
    pub pending_changed: EventHookSource<RedactionPendingChangedEvent>,
}

/// `createEvents(ctx)`.
pub fn create_events(cx: &ControllerContext) -> RedactionEvents {
    RedactionEvents { applied: cx.event_source(), pending_changed: cx.event_source() }
}
