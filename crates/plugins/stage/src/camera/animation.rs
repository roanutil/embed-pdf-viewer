//! `camera/animation.ts`: camera motion over the scheduler seam. The
//! coordinate tween, the zoom-anchored tween (the focal page-point holds
//! still by construction), and the momentum and edge physics (glide,
//! spring). One frame handle: every verb's first act, `cancel_anim`, is
//! also a catch.

use crate::prelude::*;

/// One axis of the momentum integrator, in camera-space screen px.
#[derive(Clone, Copy)]
struct AxisState {
    p: f64,
    v: f64,
    springing: bool,
    done: bool,
}

impl StageCapability {
    // ── camera tween ──────────────────────────────────────────────────────────

    pub(crate) fn cancel_anim(&self) {
        let raf = self.inner.raf.get();
        if raf != 0 {
            self.caf_call(raf);
            self.inner.raf.set(0);
        }
    }

    /// A tween or fling reached its natural end.
    fn end_motion(&self) {
        self.inner.raf.set(0);
        self.inner.events.motion_ended.emit(&StageMotionEndedEvent { camera: self.cam() });
    }

    /// Tween to `target`, clamping every frame to `bounds`. `then` runs on
    /// natural completion only.
    pub(crate) fn animate_to(&self, target: Camera, bounds: Rect, then: Option<Rc<dyn Fn(&Self)>>) {
        if !self.can_animate() {
            self.set_cam(target, bounds);
            if let Some(then) = then {
                then(self);
            }
            return;
        }
        self.cancel_anim();
        let from = self.cam();
        let t0 = Rc::new(Cell::new(None::<f64>));
        let id = self.raf_call(self.tween_tick(from, target, bounds, t0, then));
        self.inner.raf.set(id);
    }

    fn tween_tick(
        &self,
        from: Camera,
        target: Camera,
        bounds: Rect,
        t0: Rc<Cell<Option<f64>>>,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            let start = match t0.get() {
                Some(t) => t,
                None => {
                    t0.set(Some(now));
                    now
                }
            };
            let k = ease_out_cubic(js::min(1.0, (now - start) / TWEEN_MS));
            let lerp = |a: f64, b: f64| a + (b - a) * k;
            this.set_cam(
                Camera {
                    x: lerp(from.x, target.x),
                    y: lerp(from.y, target.y),
                    zoom: lerp(from.zoom, target.zoom),
                },
                bounds,
            );
            if k < 1.0 {
                let id = this.raf_call(this.tween_tick(from, target, bounds, t0, then));
                this.inner.raf.set(id);
            } else {
                this.end_motion();
                if let Some(then) = then {
                    then(&this);
                }
            }
        })
    }

    /// Zoom-anchored tween: the zoom interpolates geometrically and each
    /// frame's camera derives from the focal anchor at that zoom.
    pub(crate) fn animate_zoom_anchored(
        &self,
        focal: Anchor,
        pt: Point,
        target_zoom: f64,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) {
        if !self.can_animate() {
            let scene = self.build_scene();
            if scene.item_count > 0 {
                self.set_cam_anchor_at_screen(focal, &scene, pt, target_zoom);
            }
            if let Some(then) = then {
                then(self);
            }
            return;
        }
        self.cancel_anim();
        let z0 = self.cam().zoom;
        let t0 = Rc::new(Cell::new(None::<f64>));
        let id = self.raf_call(self.zoom_tick(focal, pt, z0, target_zoom, t0, then));
        self.inner.raf.set(id);
    }

    /// `setCam(S.cameraForAnchorAtScreen(focal, scene, pt, zoom))`, whose
    /// default bounds exist whenever the scene has items.
    pub(crate) fn set_cam_anchor_at_screen(
        &self,
        focal: Anchor,
        scene: &Scene,
        pt: Point,
        zoom: f64,
    ) {
        let camera = sc::camera_for_anchor_at_screen(focal, scene, pt, zoom)
            .expect("a scene with items places an anchor");
        let bounds = self.stay_bounds().expect("a scene with items has bounds");
        self.set_cam(camera, bounds);
    }

    fn zoom_tick(
        &self,
        focal: Anchor,
        pt: Point,
        z0: f64,
        target_zoom: f64,
        t0: Rc<Cell<Option<f64>>>,
        then: Option<Rc<dyn Fn(&Self)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            let start = match t0.get() {
                Some(t) => t,
                None => {
                    t0.set(Some(now));
                    now
                }
            };
            let k = ease_out_cubic(js::min(1.0, (now - start) / TWEEN_MS));
            let z = zoom_lerp(z0, target_zoom, k);
            let scene = this.build_scene();
            if scene.item_count > 0 {
                this.set_cam_anchor_at_screen(focal, &scene, pt, z);
            }
            if k < 1.0 {
                let id = this.raf_call(this.zoom_tick(focal, pt, z0, target_zoom, t0, then));
                this.inner.raf.set(id);
            } else {
                this.end_motion();
                if let Some(then) = then {
                    then(&this);
                }
            }
        })
    }

    // ── momentum and edge physics ─────────────────────────────────────────────

    pub(crate) fn start_camera_physics(&self, vx_finger: f64, vy_finger: f64) -> Result<(), Error> {
        if !self.can_animate() {
            return self.set_cam_current(); // no host frames: snap into bounds
        }
        self.cancel_anim();
        let zoom = self.cam().zoom;
        let c0 = self.cam();
        let cl0 = sc::clamp_camera(c0, self.stay_bounds()?, self.vp(), self.constraint());
        let b0 = self.stay_bounds()?;
        let v0 = self.vp();
        let mk = |cam_a: f64, clamped_a: f64, finger_v: f64| AxisState {
            p: cam_a * zoom,
            v: -finger_v / 1000.0,
            springing: cam_a != clamped_a,
            done: false,
        };
        // A fitting axis takes no ballistic motion.
        let ax = mk(
            c0.x,
            cl0.x,
            if self.axis_travels(b0.x, b0.width, v0.width, zoom) { vx_finger } else { 0.0 },
        );
        let ay = mk(
            c0.y,
            cl0.y,
            if self.axis_travels(b0.y, b0.height, v0.height, zoom) { vy_finger } else { 0.0 },
        );
        if !ax.springing && !ay.springing && js::hypot(ax.v, ay.v) < FLING_STOP {
            return Ok(());
        }
        let state = Rc::new(RefCell::new((ax, ay, false, 0.0)));
        let id = self.raf_call(self.physics_tick(zoom, state));
        self.inner.raf.set(id);
        Ok(())
    }

    fn physics_tick(
        &self,
        zoom: f64,
        state: Rc<RefCell<(AxisState, AxisState, bool, f64)>>,
    ) -> Box<dyn FnOnce(f64)> {
        let this = self.clone();
        Box::new(move |now| {
            this.inner.raf.set(0);
            let (mut ax, mut ay, started, last) = *state.borrow();
            // The first frame assumes one 60 Hz step.
            let dt = if started { js::min(64.0, js::max(1.0, now - last)) } else { 16.0 };
            // The stay bounds exist: the motion started against them.
            let Ok(bounds) = this.stay_bounds() else { return };
            let cl = sc::clamp_camera(
                Camera { x: ax.p / zoom, y: ay.p / zoom, zoom },
                bounds,
                this.vp(),
                this.constraint(),
            );
            let step = |a: &mut AxisState, edge_world: f64| {
                if a.done {
                    return;
                }
                let r = if a.springing {
                    spring_step(a.p, a.v, edge_world * zoom, dt)
                } else {
                    glide_step(a.p, a.v, dt)
                };
                a.p = r.p;
                a.v = r.v;
                a.done = r.done;
            };
            step(&mut ax, cl.x);
            step(&mut ay, cl.y);
            let bounds = this.stay_bounds().unwrap_or(bounds);
            let ncl = sc::clamp_camera(
                Camera { x: ax.p / zoom, y: ay.p / zoom, zoom },
                bounds,
                this.vp(),
                this.constraint(),
            );
            let convert = |a: &mut AxisState, clamped_a: f64| {
                if !a.springing && (clamped_a * zoom - a.p).abs() > 1e-6 {
                    a.p = clamped_a * zoom;
                    a.springing = true;
                    a.done = false;
                }
            };
            convert(&mut ax, ncl.x);
            convert(&mut ay, ncl.y);
            *state.borrow_mut() = (ax, ay, true, now);
            this.set_cam_raw(Camera { x: ax.p / zoom, y: ay.p / zoom, zoom });
            // A throw in a frame callback is uncaught in the TypeScript.
            let _ = this.sync_cursor_from_camera();
            if ax.done && ay.done {
                // The exact landing: clamped, snapped, settled.
                if this.set_cam_current().is_ok() {
                    this.inner
                        .events
                        .motion_ended
                        .emit(&StageMotionEndedEvent { camera: this.cam() });
                }
                return;
            }
            let id = this.raf_call(this.physics_tick(zoom, state));
            this.inner.raf.set(id);
        })
    }
}

// ── the area's api ────────────────────────────────────────────────────────────

impl StageCapability {
    /// Momentum pan from a release velocity in viewport px/s (`pan_by`'s
    /// sign convention). Without frames it only snaps into bounds.
    pub fn fling(&self, velocity_x: f64, velocity_y: f64) -> Result<(), Error> {
        self.start_camera_physics(velocity_x, velocity_y)
    }

    /// A tween or fling is running.
    pub fn is_moving(&self) -> bool {
        self.inner.raf.get() != 0
    }

    /// Halt any running tween or fling where it is.
    pub fn stop_motion(&self) {
        if self.inner.raf.get() == 0 {
            return;
        }
        self.cancel_anim();
        self.inner.events.motion_ended.emit(&StageMotionEndedEvent { camera: self.cam() });
    }
}
