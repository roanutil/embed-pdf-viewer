//! `camera/gestures.ts`: direct manipulation. Set, pan, scroll and
//! pinch-zoom the camera, and the gesture bracket. The camera moves freely
//! and the cursor is derived from it.

use crate::model::{CameraObject, patch_settings};
use crate::prelude::*;

impl StageCapability {
    /// Write the camera directly; marks the cause as user motion.
    pub fn set_camera(&self, next: Camera) -> Result<(), Error> {
        self.set_camera_object(CameraObject::xy_zoom(next))
    }

    /// `setCamera` with the camera object the caller built (its member
    /// order survives an unbounded write).
    pub fn set_camera_object(&self, next: CameraObject) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_animation();
        self.write_camera_stay(next)?;
        self.sync_cursor_from_camera()
    }

    /// Pan by a viewport delta.
    pub fn pan_by(&self, dx: f64, dy: f64) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_animation();
        let gesture = &self.inner.gesture;
        if gesture.depth.get() > 0 && gesture.elastic.get() {
            // Elastic: integrate the finger on the unclamped camera and
            // display it through the resistance curve.
            let raw = match gesture.raw.get() {
                Some(raw) => raw,
                None => self.unrubberize(self.camera())?,
            };
            let raw = sc::pan_by_screen(raw, dx, dy);
            gesture.raw.set(Some(raw));
            self.write_camera_unclamped(CameraObject::zoom_xy(self.rubberize(raw)?));
        } else {
            self.write_camera_stay(CameraObject::xy_zoom(sc::pan_by_screen(
                self.camera(),
                dx,
                dy,
            )))?;
        }
        self.sync_cursor_from_camera()
    }

    /// `Element.scrollTo` for the camera: absolute, clamped, cursor-synced.
    pub fn scroll_to(&self, options: StageScrollToOptions) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_animation();
        if self.build_scene().item_count == 0 {
            return Ok(());
        }
        let bounds = self.stay_bounds()?;
        let next = CameraObject::zoom_xy(sc::camera_from_scroll(
            self.camera(),
            bounds,
            self.viewport(),
            self.padding(),
            sc::ScrollTarget { left: options.left, top: options.top },
        ));
        if options.behavior == Some(ScrollBehaviorKind::Smooth) {
            let bounds = self.stay_bounds()?;
            // A throw in a frame callback is uncaught in the TypeScript.
            self.animate_to(
                next,
                bounds,
                Some(Rc::new(|stage: &StageCapability| {
                    let _ = stage.sync_cursor_from_camera();
                })),
            );
            Ok(())
        } else {
            self.write_camera_stay(next)?;
            self.sync_cursor_from_camera()
        }
    }

    /// `Element.scrollBy`: relative offsets over `scroll_to`.
    pub fn scroll_by(&self, options: StageScrollToOptions) -> Result<(), Error> {
        let metrics = self.scroll_metrics_now();
        self.scroll_to(StageScrollToOptions {
            left: options.left.map(|left| metrics.scroll_left + left),
            top: options.top.map(|top| metrics.scroll_top + top),
            behavior: options.behavior,
        })
    }

    /// Zoom by a factor holding a viewport point fixed: the pinch
    /// primitive.
    pub fn zoom_around(&self, point: ViewportPoint, factor: f64) -> Result<(), Error> {
        self.cancel_animation();
        let before = self.build_scene();
        // The page-relative focal point: what is under the pointer.
        let focal = if before.item_count > 0 {
            Some(
                sc::anchor_at_point(&before, sc::to_world(self.camera(), point))
                    .map_err(stage_error)?,
            )
        } else {
            None
        };
        self.write_camera_stay(CameraObject::xy_zoom(sc::zoom_around(
            self.camera(),
            point,
            factor,
        )))?;
        // Record the resulting level as the zoom intent; inside a gesture
        // bracket it is deferred to endGesture.
        if self.inner.gesture.depth.get() == 0 {
            let level = self.camera().zoom;
            self.update(|s| {
                patch_settings(s, &StageSettingsPatch::zoom(ZoomSpec::Level { level }))
            });
        }
        // A wrapped grid may have re-wrapped under the camera: re-pin the
        // same page-point under the pointer against the new geometry.
        let after = self.build_scene();
        if let Some(focal) = focal
            && !Rc::ptr_eq(&after, &before)
            && after.item_count > 0
        {
            self.write_camera_anchor_at_screen(focal, &after, point, self.camera().zoom)?;
        }
        // A zoom write in an elastic gesture re-bases the pan integrator.
        let gesture = &self.inner.gesture;
        if gesture.depth.get() > 0 && gesture.elastic.get() {
            gesture.raw.set(Some(self.unrubberize(self.camera())?));
        }
        self.sync_cursor_from_camera()
    }

    /// Bracket a direct-manipulation gesture (touch pan, pinch); re-entrant.
    /// `elastic` enables rubber-band. Opening one catches any running tween
    /// or fling.
    pub fn begin_gesture(&self, options: Option<GestureOptions>) {
        let gesture = &self.inner.gesture;
        gesture.depth.set(gesture.depth.get() + 1);
        if gesture.depth.get() == 1 {
            self.cancel_animation();
            gesture.zoomed.set(false);
            gesture.elastic.set(options.and_then(|o| o.elastic) == Some(true));
            gesture.raw.set(None);
        }
    }

    pub fn end_gesture(&self) -> Result<(), Error> {
        let gesture = &self.inner.gesture;
        if gesture.depth.get() == 0 {
            return Ok(());
        }
        gesture.depth.set(gesture.depth.get() - 1);
        if gesture.depth.get() > 0 {
            return Ok(());
        }
        gesture.raw.set(None);
        let was_elastic = gesture.elastic.get();
        gesture.elastic.set(false);
        if gesture.zoomed.get() {
            gesture.zoomed.set(false);
            // The deferred zoom intent: one patch for the whole gesture.
            let level = self.camera().zoom;
            self.update(|s| {
                patch_settings(s, &StageSettingsPatch::zoom(ZoomSpec::Level { level }))
            });
            self.arm_rest();
        }
        self.sync_cursor_from_camera()?;
        if was_elastic {
            // Released while stretched: spring home.
            let current = self.camera();
            let clamped =
                sc::clamp_camera(current, self.stay_bounds()?, self.viewport(), self.constraint());
            if clamped.x != current.x || clamped.y != current.y {
                self.start_camera_physics(0.0, 0.0)?;
            }
        }
        Ok(())
    }
}
