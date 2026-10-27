//! `write/calibration.ts`: the calibrate flow. Arm the tool, turn a
//! captured distance draft into a request (page space), and dismiss it.

use epdf_kernel::Error;
use epdf_kernel::geometry::{PageRef, Point};
use serde_json::Value;

use crate::contract::{
    CalibrationDismissedEvent, CalibrationRequest, CalibrationRequestedEvent, SCALE_PERMISSION,
};
use crate::controller::MeasurementCapability;
use crate::model::set_calibration;
use crate::units::measurement_point;

/// A draft's `from` or `to`; a missing coordinate reads `NaN`.
fn point_of(value: Option<&Value>) -> Point {
    let read = |key: &str| {
        value
            .and_then(|p| p.get(key))
            .and_then(epdf_annotation_core::json::number)
            .unwrap_or(f64::NAN)
    };
    Point { x: read("x"), y: read("y") }
}

impl MeasurementCapability {
    /// Arm the calibrate tool (two points, then `on_calibration_requested`).
    /// `permission-denied` without `doc.annotate.modify`.
    pub fn start_calibration(&self) -> Result<(), Error> {
        let core = self.core();
        core.cx.assert_allowed(SCALE_PERMISSION, "measurement.startCalibration")?;
        core.cx.update_state(|s| set_calibration(&s, None));
        core.interaction.activate_tool("calibrate")
    }

    /// Drop the pending request. Fires `on_calibration_dismissed` when there
    /// was one.
    pub fn dismiss_calibration(&self) {
        let core = self.core();
        if core.cx.state().calibration.is_none() {
            return;
        }
        core.cx.update_state(|s| set_calibration(&s, None));
        core.events.calibration_dismissed.emit(&CalibrationDismissedEvent);
    }

    /// `connect()` of the calibrate flow: a distance draft from the
    /// calibrate tool becomes the pending request.
    pub(crate) fn connect_calibration(&self) {
        let core = self.core();
        let this = std::rc::Rc::downgrade(&self.0);
        let off = core.annotation.on_draft_captured(move |draft: &Value| {
            let Some(core) = this.upgrade() else { return Ok(()) };
            if draft.get("tool").and_then(Value::as_str) != Some("calibrate")
                || !core.registry.can_calibrate()
            {
                return Ok(());
            }
            let from = measurement_point(point_of(draft.get("from")));
            let to = measurement_point(point_of(draft.get("to")));
            let user_space_length = epdf_geometry::js::hypot(to.x - from.x, to.y - from.y);
            // `!(userSpaceLength > 0)`: zero, negative and NaN make no request.
            if user_space_length.is_nan() || user_space_length <= 0.0 {
                return Ok(());
            }
            let Some(page) = draft.get("page").and_then(PageRef::from_json) else {
                return Ok(());
            };
            let request = CalibrationRequest { page, from, to, user_space_length };
            core.interaction.activate_tool("pointer")?;
            core.cx.update_state(|s| set_calibration(&s, Some(request.clone())));
            core.events.calibration_requested.emit(&CalibrationRequestedEvent { request });
            Ok(())
        });
        core.cx.listen(off);
    }
}
