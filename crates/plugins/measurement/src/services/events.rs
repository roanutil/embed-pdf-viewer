//! `services/events.ts`: the capability's four event sources, disposed with
//! the instance.

use epdf_kernel::ControllerContext;
use epdf_kernel::event_hook::EventHookSource;

use crate::contract::{
    CalibrationCompletedEvent, CalibrationDismissedEvent, CalibrationRequestedEvent,
    MeasurementScaleChangedEvent,
};

pub struct MeasurementEvents {
    pub scale_changed: EventHookSource<MeasurementScaleChangedEvent>,
    pub calibration_requested: EventHookSource<CalibrationRequestedEvent>,
    pub calibration_completed: EventHookSource<CalibrationCompletedEvent>,
    pub calibration_dismissed: EventHookSource<CalibrationDismissedEvent>,
}

/// `createEvents(ctx)`.
pub fn create_events(cx: &ControllerContext) -> MeasurementEvents {
    MeasurementEvents {
        scale_changed: cx.event_source(),
        calibration_requested: cx.event_source(),
        calibration_completed: cx.event_source(),
        calibration_dismissed: cx.event_source(),
    }
}
