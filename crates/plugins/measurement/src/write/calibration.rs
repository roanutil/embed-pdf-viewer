//! `write/calibration.ts`: the calibrate flow. Arm the tool, turn a
//! captured distance draft into a request (page space), and dismiss it.

use epdf_kernel::Error;

use crate::contract::*;
use crate::controller::MeasurementCapability;
use crate::model::MeasurementAction;
use crate::units::measurement_point;

impl MeasurementCapability {
    /// Arm the calibrate tool (two points, then `on_calibration_requested`).
    /// Nothing without authority. `Err` is the hub refusing the tool.
    pub fn start_calibration(&self) -> Result<(), Error> {
        let s = self.s();
        if !s.can_calibrate() {
            return Ok(());
        }
        s.cx.dispatch(MeasurementAction::Calibration { request: None });
        s.interaction.activate_tool("calibrate", None)
    }

    /// Drop the pending request.
    pub fn dismiss_calibration(&self) {
        let s = self.s();
        if s.state().calibration.is_none() {
            return;
        }
        s.cx.dispatch(MeasurementAction::Calibration { request: None });
        s.events.calibration_dismissed.emit(&CalibrationDismissedEvent {});
    }

    /// A distance draft from the calibrate tool becomes the pending request.
    pub(crate) fn connect_calibration(&self) {
        let s = self.s();
        let Ok(doc) = s.cx.doc() else { return };
        let this = self.clone();
        let off = s.annotation.on_draft_captured(move |draft| {
            let s = this.s();
            if draft.tool != "calibrate" || !doc.security().allows("doc.annotate.modify") {
                return Ok(());
            }
            let a = measurement_point(epdf_annotation_core::PdfPoint {
                x: draft.from.x,
                y: draft.from.y,
            });
            let b =
                measurement_point(epdf_annotation_core::PdfPoint { x: draft.to.x, y: draft.to.y });
            let user_space_length = epdf_geometry::js::hypot(b.x - a.x, b.y - a.y);
            // `!(userSpaceLength > 0)`: NaN makes no request either.
            if user_space_length.is_nan() || user_space_length <= 0.0 {
                return Ok(());
            }
            // The page left the registry mid-drag.
            let (Ok(from), Ok(to)) = (s.to_page(&draft.page, a), s.to_page(&draft.page, b)) else {
                return Ok(());
            };
            let request = CalibrationRequest { page: draft.page, from, to, user_space_length };
            s.interaction.activate_tool("pointer", None)?;
            s.cx.dispatch(MeasurementAction::Calibration { request: Some(request.clone()) });
            s.events.calibration_requested.emit(&CalibrationRequestedEvent { request });
            Ok(())
        });
        s.cx.defer(Box::new(move || {
            off.call();
            Ok(None)
        }));
    }
}
