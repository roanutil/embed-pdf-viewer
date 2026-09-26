//! `services/events.ts`: the four change hooks, disposed with the plugin.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::event_hook::EventHookSource;

use crate::contract::{
    CalibrationCompletedEvent, CalibrationDismissedEvent, CalibrationRequestedEvent,
    MeasurementHost, MeasurementScaleChangedEvent,
};
use crate::controller::MeasurementContext;

#[derive(Clone)]
pub struct MeasurementEvents {
    pub scale_changed: EventHookSource<MeasurementScaleChangedEvent>,
    pub calibration_requested: EventHookSource<CalibrationRequestedEvent>,
    pub calibration_completed: EventHookSource<CalibrationCompletedEvent>,
    pub calibration_dismissed: EventHookSource<CalibrationDismissedEvent>,
}

/// `createEvents(ctx)`. A throwing listener goes to
/// `console.error('[measurement] listener failed:', error)`.
pub fn create_events(cx: &MeasurementContext, host: &MeasurementHost) -> MeasurementEvents {
    let console_error = host.console_error.clone();
    let report: Rc<dyn Fn(&Error)> = Rc::new(move |error: &Error| {
        if let Some(sink) = &console_error {
            sink("[measurement] listener failed:", error);
        }
    });
    let events = MeasurementEvents {
        scale_changed: EventHookSource::new(Some(report.clone())),
        calibration_requested: EventHookSource::new(Some(report.clone())),
        calibration_completed: EventHookSource::new(Some(report.clone())),
        calibration_dismissed: EventHookSource::new(Some(report)),
    };
    let hooks = events.clone();
    cx.defer(Box::new(move || {
        hooks.scale_changed.dispose();
        hooks.calibration_requested.dispose();
        hooks.calibration_completed.dispose();
        hooks.calibration_dismissed.dispose();
        Ok(None)
    }));
    events
}
