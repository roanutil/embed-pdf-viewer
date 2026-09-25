//! `motion.ts`: the camera's motion vocabulary, every formula pure.
//!
//! The capability owns the drivers (frames, dispatch, gesture state); the
//! laws they integrate live here, parameterized by `dt`/`t`: the ballistic
//! pair (`glide_step`, `spring_step`), the rubber-band curve and its inverse
//! (`rubber_out`, `rubber_in`), and the tween interpolators.

use serde::{Deserialize, Serialize};

/// Resistance coefficient of the rubber-band curve (the iOS feel).
pub const RUBBER: f64 = 0.55;
/// Glide decay per ms: UIScrollView's normal deceleration rate.
pub const FLING_DECAY: f64 = 0.998;
/// px/ms; below about 20 px/s the eye reads "stopped".
pub const FLING_STOP: f64 = 0.02;
/// rad/ms: critically damped, about 400 ms to settle.
pub const SPRING_OMEGA: f64 = 0.015;

/// Overshoot distance to displayed stretch, asymptotic to the viewport
/// dimension `dim`. `rubber_out(0, dim)` is 0.
pub fn rubber_out(d: f64, dim: f64) -> f64 {
    (1.0 - 1.0 / ((d * RUBBER) / dim + 1.0)) * dim
}

/// The exact inverse of [`rubber_out`] (defined for `out < dim`).
pub fn rubber_in(out: f64, dim: f64) -> f64 {
    (out * dim) / (RUBBER * (dim - out))
}

/// What one integration step hands back; `done` means at rest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionStep {
    pub p: f64,
    pub v: f64,
    pub done: bool,
}

/// One frame of free glide: exponential velocity decay, position advanced
/// by the midpoint integral of the decay over `dt`.
pub fn glide_step(p: f64, v: f64, dt: f64) -> MotionStep {
    let k = FLING_DECAY.powf(dt);
    let np = p + v * ((dt * (1.0 + k)) / 2.0);
    let nv = v * k;
    if nv.abs() < FLING_STOP {
        MotionStep { p: np, v: 0.0, done: true }
    } else {
        MotionStep { p: np, v: nv, done: false }
    }
}

/// One frame of critically-damped spring toward `edge` (semi-implicit
/// Euler). Lands exactly on the edge once within half a pixel at negligible
/// speed.
pub fn spring_step(p: f64, v: f64, edge: f64, dt: f64) -> MotionStep {
    let nv = v + (-SPRING_OMEGA * SPRING_OMEGA * (p - edge) - 2.0 * SPRING_OMEGA * v) * dt;
    let np = p + nv * dt;
    if (np - edge).abs() < 0.5 && nv.abs() < 0.01 {
        return MotionStep { p: edge, v: 0.0, done: true };
    }
    MotionStep { p: np, v: nv, done: false }
}

pub fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powf(3.0)
}

/// Geometric zoom interpolation, `z0 * (z1 / z0)^k`.
pub fn zoom_lerp(z0: f64, z1: f64, k: f64) -> f64 {
    z0 * (z1 / z0).powf(k)
}
