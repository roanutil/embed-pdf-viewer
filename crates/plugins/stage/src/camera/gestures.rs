//! `camera/gestures.ts`: direct manipulation. Set, pan, scroll and
//! pinch-zoom the camera, and the gesture bracket. The camera moves freely
//! and the cursor is derived from it.

use crate::prelude::*;

impl StageCapability {
    pub fn set_camera(&self, c: Camera) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        self.set_cam_stay(c)?;
        self.sync_cursor_from_camera();
        Ok(())
    }

    pub fn pan_by(&self, dx: f64, dy: f64) -> Result<(), Error> {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        if self.inner.gesture_depth.get() > 0 && self.inner.gesture_elastic.get() {
            // Elastic: integrate the finger on the unclamped camera.
            let raw = match self.inner.gesture_raw.get() {
                Some(raw) => raw,
                None => self.unrubberize(self.cam())?,
            };
            let raw = sc::pan_by_screen(raw, dx, dy);
            self.inner.gesture_raw.set(Some(raw));
            self.set_cam_raw(self.rubberize(raw)?);
        } else {
            self.set_cam_stay(sc::pan_by_screen(self.cam(), dx, dy))?;
        }
        self.sync_cursor_from_camera();
        Ok(())
    }

    /// `Element.scrollTo` for the camera: absolute, clamped, cursor-synced.
    pub fn scroll_to(&self, opts: StageScrollToOptions) {
        self.mark_cause(MotionCause::User);
        self.cancel_anim();
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return;
        }
        let bounds = self.stay_bounds().expect("a scene with items has bounds");
        let next = sc::camera_from_scroll(
            self.cam(),
            bounds,
            self.vp(),
            self.pad(),
            sc::ScrollTarget { left: opts.left, top: opts.top },
        );
        if opts.behavior == Some(ScrollBehaviorKind::Smooth) {
            let bounds = self.stay_bounds().expect("a scene with items has bounds");
            self.animate_to(next, bounds, Some(Rc::new(|s: &Self| s.sync_cursor_from_camera())));
        } else {
            self.set_cam(next, bounds);
            self.sync_cursor_from_camera();
        }
    }

    /// `Element.scrollBy`: relative offsets over `scroll_to`.
    pub fn scroll_by(&self, opts: StageScrollToOptions) {
        let m = self.scroll_metrics_now();
        self.scroll_to(StageScrollToOptions {
            left: opts.left.map(|left| m.scroll_left + left),
            top: opts.top.map(|top| m.scroll_top + top),
            behavior: opts.behavior,
        });
    }

    pub fn zoom_around(&self, pt: Point, factor: f64) -> Result<(), Error> {
        self.cancel_anim();
        let before = self.build_scene();
        let focal = if before.item_count > 0 {
            Some(
                sc::anchor_at_point(&before, sc::to_world(self.cam(), pt))
                    .expect("a scene with items has an anchor"),
            )
        } else {
            None
        };
        self.set_cam_stay(sc::zoom_around(self.cam(), pt, factor))?;
        if self.inner.gesture_depth.get() == 0 {
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: self.cam().zoom }),
            });
        }
        // A wrapped grid re-wrapped under the camera: re-pin the focal point.
        let after = self.build_scene();
        if let Some(focal) = focal
            && !Rc::ptr_eq(&after, &before)
            && after.item_count > 0
        {
            self.set_cam_anchor_at_screen(focal, &after, pt, self.cam().zoom);
        }
        if self.inner.gesture_depth.get() > 0 && self.inner.gesture_elastic.get() {
            self.inner.gesture_raw.set(Some(self.unrubberize(self.cam())?));
        }
        self.sync_cursor_from_camera();
        Ok(())
    }

    /// Bracket a continuous gesture; re-entrant. Opening one catches any
    /// running tween or fling.
    pub fn begin_gesture(&self, options: Option<GestureOptions>) {
        self.inner.gesture_depth.set(self.inner.gesture_depth.get() + 1);
        if self.inner.gesture_depth.get() == 1 {
            self.cancel_anim();
            self.inner.gesture_zoomed.set(false);
            self.inner.gesture_elastic.set(options.and_then(|o| o.elastic) == Some(true));
            self.inner.gesture_raw.set(None);
        }
    }

    pub fn end_gesture(&self) -> Result<(), Error> {
        if self.inner.gesture_depth.get() == 0 {
            return Ok(());
        }
        self.inner.gesture_depth.set(self.inner.gesture_depth.get() - 1);
        if self.inner.gesture_depth.get() > 0 {
            return Ok(());
        }
        self.inner.gesture_raw.set(None);
        let was_elastic = self.inner.gesture_elastic.get();
        self.inner.gesture_elastic.set(false);
        if self.inner.gesture_zoomed.get() {
            self.inner.gesture_zoomed.set(false);
            self.dispatch(StageAction::Patch {
                patch: PartialSettings::zoom(ZoomSpec::Level { level: self.cam().zoom }),
            });
            self.arm_rest();
        }
        self.sync_cursor_from_camera();
        if was_elastic {
            let c = self.cam();
            let cl = sc::clamp_camera(c, self.stay_bounds()?, self.vp(), self.constraint());
            if cl.x != c.x || cl.y != c.y {
                self.start_camera_physics(0.0, 0.0)?;
            }
        }
        Ok(())
    }
}
