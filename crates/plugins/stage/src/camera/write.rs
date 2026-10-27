//! `camera/write.ts`: the camera write path. The one low-level clamped write
//! (`write_camera`), the elastic unclamped write and its rubber-band
//! curves, the rest detector that gates device snapping, the gesture
//! bracket state, and cursor reconciliation from the camera (manipulation
//! only; navigation sets the cursor as intent).

use crate::model::{CameraObject, set_camera, set_camera_resting, set_cursor, set_motion_cause};
use crate::prelude::*;

/// How long the zoom must hold still before the camera counts as resting.
const REST_MS: f64 = 150.0;

impl StageCapability {
    /// The camera object the state holds.
    pub(crate) fn camera_object(&self) -> CameraObject {
        self.st().camera_object()
    }

    // ── the camera-rest detector ──

    fn stop_rest_countdown(&self) {
        let cancel = self.inner.cancel_rest.borrow_mut().take();
        if let Some(cancel) = cancel {
            cancel.call();
        }
    }

    /// Initial placement snaps immediately, and a host without frames keeps
    /// snapping always on: rest-gating refines live gestures only.
    pub(crate) fn arm_rest(&self) {
        if !self.inner.clock.has_frames() || !self.inner.placement.started.get() {
            return;
        }
        self.update(|s| set_camera_resting(s, false));
        self.stop_rest_countdown();
        let this = self.clone();
        let cancel = self.inner.clock.after(REST_MS, move || {
            this.inner.cancel_rest.borrow_mut().take();
            this.update(|s| set_camera_resting(s, true));
        });
        *self.inner.cancel_rest.borrow_mut() = Some(cancel);
    }

    /// `clampCamera(next, bounds, ...)` as an object: unbounded, the input
    /// object itself; bounded, a new `{zoom, x, y}`.
    pub(crate) fn clamp_object(&self, next: CameraObject, bounds: Rect) -> CameraObject {
        let constraint = self.constraint();
        if !constraint.bounded {
            return next;
        }
        CameraObject::zoom_xy(sc::clamp_camera(next.camera, bounds, self.viewport(), constraint))
    }

    /// The one low-level camera write: clamp to `bounds` and store. It never
    /// touches the cursor.
    pub(crate) fn write_camera(&self, next: CameraObject, bounds: Rect) {
        let clamped = self.clamp_object(next, bounds);
        if clamped.camera.zoom != self.camera().zoom {
            let gesture = &self.inner.gesture;
            if gesture.depth.get() > 0 {
                // Mid-gesture: un-rest at once but hold the countdown; rest
                // is declared at endGesture, not at a pinch hesitation.
                gesture.zoomed.set(true);
                self.update(|s| set_camera_resting(s, false));
                self.stop_rest_countdown();
            } else {
                self.arm_rest();
            }
        }
        self.update(|s| set_camera(s, clamped));
    }

    /// `writeCamera(next)` with its default bounds (`stayBounds()`), which
    /// throw in paged flow over a document with no pages.
    pub(crate) fn write_camera_stay(&self, next: CameraObject) -> Result<(), Error> {
        let bounds = self.stay_bounds()?;
        self.write_camera(next, bounds);
        Ok(())
    }

    // ── rubber-band (elastic overscroll) ──

    pub(crate) fn axis_travels(&self, origin: f64, content: f64, view: f64, zoom: f64) -> bool {
        !sc::travel_range(origin, content, view, zoom, self.constraint().padding).fits
    }

    /// The unclamped finger-integrated camera to the displayed camera:
    /// clamp, then re-apply the overshoot through the resistance curve, per
    /// travelling axis.
    pub(crate) fn rubberize(&self, raw: Camera) -> Result<Camera, Error> {
        let clamped =
            sc::clamp_camera(raw, self.stay_bounds()?, self.viewport(), self.constraint());
        let size = self.viewport();
        let bounds = self.stay_bounds()?;
        let axis = |raw_position: f64, clamped_position: f64, dimension: f64, travels: bool| {
            if !travels {
                return clamped_position;
            }
            let overshoot_world = raw_position - clamped_position;
            if overshoot_world == 0.0 {
                return clamped_position;
            }
            let stretch = rubber_out(overshoot_world.abs() * raw.zoom, js::max(1.0, dimension));
            clamped_position + (js::sign(overshoot_world) * stretch) / raw.zoom
        };
        Ok(Camera {
            zoom: raw.zoom,
            x: axis(
                raw.x,
                clamped.x,
                size.width,
                self.axis_travels(bounds.x, bounds.width, size.width, raw.zoom),
            ),
            y: axis(
                raw.y,
                clamped.y,
                size.height,
                self.axis_travels(bounds.y, bounds.height, size.height, raw.zoom),
            ),
        })
    }

    /// The displayed camera to the raw position `rubberize` would have
    /// produced it from, the overshoot capped just under the asymptote.
    pub(crate) fn unrubberize(&self, displayed: Camera) -> Result<Camera, Error> {
        let clamped =
            sc::clamp_camera(displayed, self.stay_bounds()?, self.viewport(), self.constraint());
        let size = self.viewport();
        let bounds = self.stay_bounds()?;
        let axis =
            |displayed_position: f64, clamped_position: f64, dimension: f64, travels: bool| {
                if !travels {
                    return clamped_position;
                }
                let overshoot_world = displayed_position - clamped_position;
                if overshoot_world == 0.0 {
                    return clamped_position;
                }
                let span = js::max(1.0, dimension);
                let stretch = js::min(overshoot_world.abs() * displayed.zoom, span - 1.0);
                clamped_position
                    + (js::sign(overshoot_world) * rubber_in(stretch, span)) / displayed.zoom
            };
        Ok(Camera {
            zoom: displayed.zoom,
            x: axis(
                displayed.x,
                clamped.x,
                size.width,
                self.axis_travels(bounds.x, bounds.width, size.width, displayed.zoom),
            ),
            y: axis(
                displayed.y,
                clamped.y,
                size.height,
                self.axis_travels(bounds.y, bounds.height, size.height, displayed.zoom),
            ),
        })
    }

    /// The elastic write path, with no clamp.
    pub(crate) fn write_camera_unclamped(&self, next: CameraObject) {
        self.update(|s| set_camera(s, next));
    }

    /// Manipulation derives the cursor from the camera; navigation never
    /// calls this, and paged flow never syncs. `Err` where the TypeScript
    /// throws: a camera with no item under it.
    pub(crate) fn sync_cursor_from_camera(&self) -> Result<(), Error> {
        if self.paged() {
            return Ok(());
        }
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return Ok(());
        }
        let page = sc::anchor_from_camera(self.camera(), &scene, self.viewport(), None)
            .map_err(stage_error)?
            .page_index;
        self.update(|s| set_cursor(s, page));
        Ok(())
    }

    /// Tag what drives the next camera or cursor change.
    pub(crate) fn mark_cause(&self, cause: MotionCause) {
        self.update(|s| set_motion_cause(s, cause));
    }
}
