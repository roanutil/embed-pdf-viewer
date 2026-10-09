//! `camera/write.ts`: the camera write path. The one clamped write
//! (`set_cam`), the elastic (unclamped) write and its rubber-band curves,
//! the rest detector that gates device snapping, and cursor reconciliation
//! from the camera (manipulation only; navigation sets the cursor as
//! intent).

use crate::prelude::*;

impl StageCapability {
    // ── camera-rest detector ──────────────────────────────────────────────────

    pub(crate) fn arm_rest(&self) {
        if !self.can_animate() || !self.inner.placement_started.get() {
            return;
        }
        if self.st().camera_resting {
            self.dispatch(StageAction::CameraRest { resting: false });
        }
        let rest_raf = self.inner.rest_raf.get();
        if rest_raf != 0 {
            self.caf_call(rest_raf);
        }
        let t0 = Rc::new(Cell::new(0.0));
        let id = self.raf_call(self.rest_tick(t0));
        self.inner.rest_raf.set(id);
    }

    fn rest_tick(&self, t0: Rc<Cell<f64>>) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |ts| {
            this.inner.rest_raf.set(0);
            if falsy(t0.get()) {
                t0.set(ts);
            }
            if ts - t0.get() >= REST_MS {
                this.dispatch(StageAction::CameraRest { resting: true });
                return;
            }
            let id = this.raf_call(this.rest_tick(t0));
            this.inner.rest_raf.set(id);
        })
    }

    /// The one low-level camera write: clamp to `bounds`, dispatch. It
    /// never touches the cursor.
    pub(crate) fn set_cam(&self, next: Camera, bounds: Rect) {
        self.set_cam_kept(next, bounds, false);
    }

    /// `setCam(cam())` with the default bounds: re-clamp in place. On an
    /// unbounded stage `clampCamera` returns its input, so the TypeScript
    /// dispatches the camera object already in state and no camera event
    /// fires.
    pub(crate) fn set_cam_current(&self) -> Result<(), Error> {
        let bounds = self.stay_bounds()?;
        let kept = !self.constraint().bounded;
        self.set_cam_kept(self.cam(), bounds, kept);
        Ok(())
    }

    fn set_cam_kept(&self, next: Camera, bounds: Rect, kept: bool) {
        let clamped = sc::clamp_camera(next, bounds, self.vp(), self.constraint());
        if clamped.zoom != self.cam().zoom {
            if self.inner.gesture_depth.get() > 0 {
                // Mid-gesture: un-rest now, hold the countdown for endGesture.
                self.inner.gesture_zoomed.set(true);
                if self.st().camera_resting {
                    self.dispatch(StageAction::CameraRest { resting: false });
                }
                let rest_raf = self.inner.rest_raf.get();
                if rest_raf != 0 {
                    self.caf_call(rest_raf);
                    self.inner.rest_raf.set(0);
                }
            } else {
                self.arm_rest();
            }
        }
        self.inner.camera_kept.set(kept);
        self.dispatch(StageAction::Camera { camera: clamped });
    }

    /// `setCam(next)` with its default bounds.
    pub(crate) fn set_cam_stay(&self, next: Camera) -> Result<(), Error> {
        let bounds = self.stay_bounds()?;
        self.set_cam(next, bounds);
        Ok(())
    }

    // ── rubber-band (elastic overscroll) ──────────────────────────────────────

    pub(crate) fn axis_travels(&self, origin: f64, content: f64, view: f64, zoom: f64) -> bool {
        !sc::travel_range(origin, content, view, zoom, self.constraint().padding).fits
    }

    /// Unclamped finger-integrated camera to the displayed camera.
    pub(crate) fn rubberize(&self, raw: Camera) -> Result<Camera, Error> {
        let clamped = sc::clamp_camera(raw, self.stay_bounds()?, self.vp(), self.constraint());
        let v = self.vp();
        let b = self.stay_bounds()?;
        let axis = |raw_a: f64, clamped_a: f64, dim: f64, travels: bool| -> f64 {
            if !travels {
                return clamped_a;
            }
            let d_world = raw_a - clamped_a;
            if d_world == 0.0 {
                return clamped_a;
            }
            let out = rubber_out(d_world.abs() * raw.zoom, js::max(1.0, dim));
            clamped_a + (js::sign(d_world) * out) / raw.zoom
        };
        Ok(Camera {
            zoom: raw.zoom,
            x: axis(raw.x, clamped.x, v.width, self.axis_travels(b.x, b.width, v.width, raw.zoom)),
            y: axis(
                raw.y,
                clamped.y,
                v.height,
                self.axis_travels(b.y, b.height, v.height, raw.zoom),
            ),
        })
    }

    /// Displayed camera to the raw position `rubberize` produced it from.
    pub(crate) fn unrubberize(&self, displayed: Camera) -> Result<Camera, Error> {
        let clamped =
            sc::clamp_camera(displayed, self.stay_bounds()?, self.vp(), self.constraint());
        let v = self.vp();
        let b = self.stay_bounds()?;
        let axis = |disp_a: f64, clamped_a: f64, dim: f64, travels: bool| -> f64 {
            if !travels {
                return clamped_a;
            }
            let d_world = disp_a - clamped_a;
            if d_world == 0.0 {
                return clamped_a;
            }
            let d = js::max(1.0, dim);
            let out = js::min(d_world.abs() * displayed.zoom, d - 1.0);
            clamped_a + (js::sign(d_world) * rubber_in(out, d)) / displayed.zoom
        };
        Ok(Camera {
            zoom: displayed.zoom,
            x: axis(
                displayed.x,
                clamped.x,
                v.width,
                self.axis_travels(b.x, b.width, v.width, displayed.zoom),
            ),
            y: axis(
                displayed.y,
                clamped.y,
                v.height,
                self.axis_travels(b.y, b.height, v.height, displayed.zoom),
            ),
        })
    }

    /// The elastic write path: no clamp.
    pub(crate) fn set_cam_raw(&self, c: Camera) {
        self.dispatch(StageAction::Camera { camera: c });
    }

    /// Manipulation derives the cursor from the camera. Paged never syncs.
    /// `Err` where the TypeScript throws: a camera with no item under it
    /// (a NaN coordinate, or a fractional grid column count).
    pub(crate) fn sync_cursor_from_camera(&self) -> Result<(), Error> {
        if self.paged() {
            return Ok(());
        }
        let scene = self.build_scene();
        if scene.item_count == 0 {
            return Ok(());
        }
        let page = sc::anchor_from_camera(self.cam(), &scene, self.vp(), None)
            .map_err(stage_error)?
            .page_index;
        if page != self.st().cursor {
            self.dispatch(StageAction::Cursor { cursor: page });
        }
        Ok(())
    }

    // ── navigation ────────────────────────────────────────────────────────────

    pub(crate) fn mark_cause(&self, cause: MotionCause) {
        if self.st().motion_cause != cause {
            self.dispatch(StageAction::MotionCause { cause });
        }
    }
}
