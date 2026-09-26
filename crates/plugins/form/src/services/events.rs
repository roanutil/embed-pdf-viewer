//! `services/events.ts`: the six confirmed-change hooks, disposed with the
//! plugin. A failing listener is reported through `console.error`
//! (`[form] event listener failed:`), never to its siblings.

use std::rc::Rc;

use epdf_kernel::event_hook::EventHookSource;
use epdf_kernel::scope::Teardown;
use epdf_kernel::{Controller, Error, Unsubscribe};

use crate::contract::*;
use crate::controller::FormCapability;
use crate::model::{FormAction, FormState};

pub(crate) struct FormEvents {
    pub value_changed: EventHookSource<FormValueChangedEvent>,
    pub field_created: EventHookSource<FormFieldChangedEvent>,
    pub field_updated: EventHookSource<FormFieldChangedEvent>,
    pub field_deleted: EventHookSource<FormFieldChangedEvent>,
    pub validation_rejected: EventHookSource<FormValidationRejectedEvent>,
    pub resynced: EventHookSource<FormResyncedEvent>,
}

impl FormEvents {
    pub fn new(
        cx: &Controller<FormState, FormAction>,
        console_error: Option<Rc<dyn Fn(&str, &Error)>>,
    ) -> Self {
        let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
            if let Some(sink) = &console_error {
                sink("[form] event listener failed:", error);
            }
        });
        let events = FormEvents {
            value_changed: EventHookSource::new(Some(report.clone())),
            field_created: EventHookSource::new(Some(report.clone())),
            field_updated: EventHookSource::new(Some(report.clone())),
            field_deleted: EventHookSource::new(Some(report.clone())),
            validation_rejected: EventHookSource::new(Some(report.clone())),
            resynced: EventHookSource::new(Some(report)),
        };
        let all = (
            events.value_changed.clone(),
            events.field_created.clone(),
            events.field_updated.clone(),
            events.field_deleted.clone(),
            events.validation_rejected.clone(),
            events.resynced.clone(),
        );
        let teardown: Teardown = Box::new(move || {
            all.0.dispose();
            all.1.dispose();
            all.2.dispose();
            all.3.dispose();
            all.4.dispose();
            all.5.dispose();
            Ok(None)
        });
        cx.defer(teardown);
        events
    }
}

impl FormCapability {
    /// `onValueChanged(listener)`: a confirmed value change, programmatic,
    /// script and remote writes alike.
    pub fn on_value_changed(
        &self,
        listener: impl Fn(&FormValueChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.value_changed.on(listener)
    }

    /// `onFieldCreated(listener)`.
    pub fn on_field_created(
        &self,
        listener: impl Fn(&FormFieldChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.field_created.on(listener)
    }

    /// `onFieldUpdated(listener)`: an update, or a widget attached or
    /// detached.
    pub fn on_field_updated(
        &self,
        listener: impl Fn(&FormFieldChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.field_updated.on(listener)
    }

    /// `onFieldDeleted(listener)`: `field` is null.
    pub fn on_field_deleted(
        &self,
        listener: impl Fn(&FormFieldChangedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.field_deleted.on(listener)
    }

    /// `onValidationRejected(listener)`: the document's scripts refused a
    /// validated write.
    pub fn on_validation_rejected(
        &self,
        listener: impl Fn(&FormValidationRejectedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.validation_rejected.on(listener)
    }

    /// `onResynced(listener)`: the snapshot was replaced wholesale.
    pub fn on_resynced(
        &self,
        listener: impl Fn(&FormResyncedEvent) -> Result<(), Error> + 'static,
    ) -> Unsubscribe {
        self.0.events.resynced.on(listener)
    }
}
