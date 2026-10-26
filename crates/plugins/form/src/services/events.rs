//! `services/events.ts`: the plugin's events. Fact events fire from the
//! fields mirror (`sync/fields.rs`). Payloads are the TypeScript's event
//! objects, as JSON.

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::{Controller, Error, Unsubscribe};
use serde_json::Value;

pub struct Events {
    pub value_changed: EventHookSource<Value>,
    pub field_created: EventHookSource<Value>,
    pub field_updated: EventHookSource<Value>,
    pub field_deleted: EventHookSource<Value>,
    pub validation_rejected: EventHookSource<Value>,
    pub resynced: EventHookSource<Value>,
}

impl Events {
    /// `createEvents(ctx)`.
    pub fn new(cx: &Controller<Value, Value>) -> Events {
        Events {
            value_changed: cx.event_source(),
            field_created: cx.event_source(),
            field_updated: cx.event_source(),
            field_deleted: cx.event_source(),
            validation_rejected: cx.event_source(),
            resynced: cx.event_source(),
        }
    }
}

macro_rules! hooks {
    ($($(#[$doc:meta])* $name:ident => $source:ident;)*) => {
        impl crate::controller::Core {
            $(
                $(#[$doc])*
                pub fn $name(
                    &self,
                    listener: impl Fn(&Value) -> Result<(), Error> + 'static,
                ) -> Unsubscribe {
                    self.events.$source.on(listener)
                }
            )*
        }
    };
}

hooks! {
    /// `onValueChanged`: a field's value changed, whoever changed it.
    on_value_changed => value_changed;
    /// `onFieldCreated`: a field was added.
    on_field_created => field_created;
    /// `onFieldUpdated`: a field's settings or widgets changed.
    on_field_updated => field_updated;
    /// `onFieldDeleted`: a field was removed.
    on_field_deleted => field_deleted;
    /// `onValidationRejected`: the form's own validation refused a value.
    on_validation_rejected => validation_rejected;
    /// `onResynced`: the whole form was read.
    on_resynced => resynced;
}
