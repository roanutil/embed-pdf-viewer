//! `camera/animation.ts`: camera motion over the host's frames
//! (`ctx.clock`). The coordinate tween, the zoom-anchored tween (the focal
//! page-point holds still by construction), and the momentum and edge
//! physics (glide, spring). One frame is waited for at a time:
//! `cancel_animation`, every verb's first act, is also "catch".

use crate::model::CameraObject;
use crate::prelude::*;

/// The default tween duration.
const TWEEN_MS: f64 = 240.0;

fn lerp(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

/// One axis of the momentum integrator, in screen px.
#[derive(Clone, Copy)]
struct AxisState {
    position: f64,
    velocity: f64,
    springing: bool,
    done: bool,
}

/// What runs when a motion reaches its natural end.
pub(crate) type Then = Rc<dyn Fn(&StageCapability)>;

impl StageCapability {
    /// Cancel the frame the running motion waits for.
    pub(crate) fn cancel_animation(&self) {
        let cancel = self.inner.cancel_frame.borrow_mut().take();
        if let Some(cancel) = cancel {
            cancel.call();
        }
    }

    /// A tween or fling reached its natural end.
    fn end_motion(&self) {
        self.inner.cancel_frame.borrow_mut().take();
        self.emit_motion_ended();
    }

    fn emit_motion_ended(&self) {
        let state = self.st();
        self.inner
            .events
            .motion_ended
            .emit(&StageMotionEndedEvent { camera: state.camera, order: state.camera_order() });
    }

    /// Wait for the next frame, as the running motion's.
    fn next_frame(&self, tick: impl FnOnce(&StageCapability, f64) + 'static) {
        let this = self.clone();
        let cancel = self.inner.clock.next_frame(move |now| tick(&this, now));
        *self.inner.cancel_frame.borrow_mut() = Some(cancel);
    }

    /// Tween to `target`, clamping every frame to `bounds`. `then` runs on
    /// natural completion only: a cancelled tween belongs to the verb that
    /// cancelled it.
    pub(crate) fn animate_to(&self, target: CameraObject, bounds: Rect, then: Option<Then>) {
        if !self.inner.clock.has_frames() {
            self.write_camera(target, bounds);
            if let Some(then) = then {
                then(self);
            }
            return;
        }
        self.cancel_animation();
        let from = self.camera();
        let started = Rc::new(Cell::new(None::<f64>));
        self.next_frame(Self::tween_tick(from, target.camera, bounds, started, then));
    }

    fn tween_tick(
        from: Camera,
        target: Camera,
        bounds: Rect,
        started: Rc<Cell<Option<f64>>>,
        then: Option<Then>,
    ) -> impl FnOnce(&StageCapability, f64) + 'static {
        move |this, now| {
            // Anchor to the first real timestamp (works even when it is 0).
            let started_at = match started.get() {
                Some(at) => at,
                None => {
                    started.set(Some(now));
                    now
                }
            };
            let progress = ease_out_cubic(js::min(1.0, (now - started_at) / TWEEN_MS));
            this.write_camera(
                CameraObject::xy_zoom(Camera {
                    x: lerp(from.x, target.x, progress),
                    y: lerp(from.y, target.y, progress),
                    zoom: lerp(from.zoom, target.zoom, progress),
                }),
                bounds,
            );
            if progress < 1.0 {
                this.next_frame(Self::tween_tick(from, target, bounds, started, then));
            } else {
                this.end_motion();
                if let Some(then) = then {
                    then(this);
                }
            }
        }
    }

    /// The zoom-anchored tween: the zoom interpolates geometrically and each
    /// frame's camera derives from the focal anchor at that zoom, then
    /// clamps.
    pub(crate) fn animate_zoom_anchored(
        &self,
        focal: Anchor,
        point: Point,
        target_zoom: f64,
        then: Option<Then>,
    ) -> Result<(), Error> {
        if !self.inner.clock.has_frames() {
            let scene = self.build_scene();
            if scene.item_count > 0 {
                self.write_camera_anchor_at_screen(focal, &scene, point, target_zoom)?;
            }
            if let Some(then) = then {
                then(self);
            }
            return Ok(());
        }
        self.cancel_animation();
        let start_zoom = self.camera().zoom;
        let started = Rc::new(Cell::new(None::<f64>));
        self.next_frame(Self::zoom_tick(focal, point, start_zoom, target_zoom, started, then));
        Ok(())
    }

    /// `writeCamera(cameraForAnchorAtScreen(focal, scene, point, zoom))`.
    pub(crate) fn write_camera_anchor_at_screen(
        &self,
        focal: Anchor,
        scene: &Scene,
        point: Point,
        zoom: f64,
    ) -> Result<(), Error> {
        let camera =
            sc::camera_for_anchor_at_screen(focal, scene, point, zoom).map_err(stage_error)?;
        self.write_camera_stay(CameraObject::zoom_xy(camera))
    }

    fn zoom_tick(
        focal: Anchor,
        point: Point,
        start_zoom: f64,
        target_zoom: f64,
        started: Rc<Cell<Option<f64>>>,
        then: Option<Then>,
    ) -> impl FnOnce(&StageCapability, f64) + 'static {
        move |this, now| {
            let started_at = match started.get() {
                Some(at) => at,
                None => {
                    started.set(Some(now));
                    now
                }
            };
            let progress = ease_out_cubic(js::min(1.0, (now - started_at) / TWEEN_MS));
            let zoom = zoom_lerp(start_zoom, target_zoom, progress);
            let scene = this.build_scene();
            if scene.item_count > 0 {
                // A throw in a frame callback is uncaught in the TypeScript;
                // the motion stops there.
                if this.write_camera_anchor_at_screen(focal, &scene, point, zoom).is_err() {
                    this.inner.cancel_frame.borrow_mut().take();
                    return;
                }
            }
            if progress < 1.0 {
                this.next_frame(Self::zoom_tick(
                    focal,
                    point,
                    start_zoom,
                    target_zoom,
                    started,
                    then,
                ));
            } else {
                this.end_motion();
                if let Some(then) = then {
                    then(this);
                }
            }
        }
    }

    // ── momentum and edge physics ──

    /// The glide, the spring and their conversion, per axis, until every
    /// trajectory ends on the clamp.
    pub(crate) fn start_camera_physics(
        &self,
        finger_velocity_x: f64,
        finger_velocity_y: f64,
    ) -> Result<(), Error> {
        if !self.inner.clock.has_frames() {
            // No host frames: snap straight into bounds.
            return self.write_camera_stay(self.camera_object());
        }
        self.cancel_animation();
        let zoom = self.camera().zoom;
        let start = self.camera();
        let start_clamped =
            sc::clamp_camera(start, self.stay_bounds()?, self.viewport(), self.constraint());
        let start_bounds = self.stay_bounds()?;
        let start_viewport = self.viewport();
        let axis_state =
            |camera_position: f64, clamped_position: f64, finger_velocity: f64| AxisState {
                position: camera_position * zoom,
                velocity: -finger_velocity / 1000.0,
                springing: camera_position != clamped_position,
                done: false,
            };
        // A fitting axis takes no ballistic motion.
        let axis_x = axis_state(
            start.x,
            start_clamped.x,
            if self.axis_travels(start_bounds.x, start_bounds.width, start_viewport.width, zoom) {
                finger_velocity_x
            } else {
                0.0
            },
        );
        let axis_y = axis_state(
            start.y,
            start_clamped.y,
            if self.axis_travels(start_bounds.y, start_bounds.height, start_viewport.height, zoom) {
                finger_velocity_y
            } else {
                0.0
            },
        );
        if !axis_x.springing
            && !axis_y.springing
            && js::hypot(axis_x.velocity, axis_y.velocity) < FLING_STOP
        {
            return Ok(());
        }
        let state = Rc::new(RefCell::new((axis_x, axis_y, None::<f64>)));
        self.next_frame(Self::physics_tick(zoom, state));
        Ok(())
    }

    fn physics_tick(
        zoom: f64,
        state: Rc<RefCell<(AxisState, AxisState, Option<f64>)>>,
    ) -> impl FnOnce(&StageCapability, f64) + 'static {
        move |this, now| {
            this.inner.cancel_frame.borrow_mut().take();
            let (mut axis_x, mut axis_y, last_frame_at) = *state.borrow();
            // The first frame assumes one 60 Hz step.
            let elapsed_ms = match last_frame_at {
                Some(last) => js::min(64.0, js::max(1.0, now - last)),
                None => 16.0,
            };
            // A throw in a frame callback is uncaught in the TypeScript; the
            // motion stops there. The stay bounds exist: the motion started
            // against them, and only paged flow over no pages lacks them.
            let Ok(bounds) = this.stay_bounds() else { return };
            let home = sc::clamp_camera(
                Camera { x: axis_x.position / zoom, y: axis_y.position / zoom, zoom },
                bounds,
                this.viewport(),
                this.constraint(),
            );
            let step = |axis: &mut AxisState, edge_world: f64| {
                if axis.done {
                    return;
                }
                let next = if axis.springing {
                    spring_step(axis.position, axis.velocity, edge_world * zoom, elapsed_ms)
                } else {
                    glide_step(axis.position, axis.velocity, elapsed_ms)
                };
                axis.position = next.position;
                axis.velocity = next.velocity;
                axis.done = next.done;
            };
            step(&mut axis_x, home.x);
            step(&mut axis_y, home.y);
            let Ok(bounds) = this.stay_bounds() else { return };
            let next_home = sc::clamp_camera(
                Camera { x: axis_x.position / zoom, y: axis_y.position / zoom, zoom },
                bounds,
                this.viewport(),
                this.constraint(),
            );
            let convert = |axis: &mut AxisState, clamped_position: f64| {
                if !axis.springing && (clamped_position * zoom - axis.position).abs() > 1e-6 {
                    axis.position = clamped_position * zoom;
                    axis.springing = true;
                    axis.done = false;
                }
            };
            convert(&mut axis_x, next_home.x);
            convert(&mut axis_y, next_home.y);
            *state.borrow_mut() = (axis_x, axis_y, Some(now));
            this.write_camera_unclamped(CameraObject::xy_zoom(Camera {
                x: axis_x.position / zoom,
                y: axis_y.position / zoom,
                zoom,
            }));
            if this.sync_cursor_from_camera().is_err() {
                return;
            }
            if axis_x.done && axis_y.done {
                // The exact landing: clamped, snapped, settled.
                if this.write_camera_stay(this.camera_object()).is_ok() {
                    this.emit_motion_ended();
                }
                return;
            }
            this.next_frame(Self::physics_tick(zoom, state));
        }
    }
}

// ── the area's api ──

impl StageCapability {
    /// Momentum pan from a release velocity in viewport px/s. Without frames
    /// it only snaps into bounds.
    pub fn fling(&self, velocity_x: f64, velocity_y: f64) -> Result<(), Error> {
        self.start_camera_physics(velocity_x, velocity_y)
    }

    /// A tween or fling is running.
    pub fn is_moving(&self) -> bool {
        self.inner.cancel_frame.borrow().is_some()
    }

    /// Halt any running tween or fling where it is.
    pub fn stop_motion(&self) {
        if self.inner.cancel_frame.borrow().is_none() {
            return;
        }
        self.cancel_animation();
        self.emit_motion_ended();
    }
}
