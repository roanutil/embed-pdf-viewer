//! `motion.ts`: the camera's motion vocabulary, every formula pure.
//!
//! The capability owns the drivers (the clock's frames, state writes,
//! gesture state); the laws they integrate live here, parameterized by
//! elapsed time: the ballistic pair (`glide_step`, `spring_step`), the
//! rubber-band curve and its inverse (`rubber_out`, `rubber_in`), and the
//! tween interpolators.

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
/// `dimension`. `rubber_out(0, dimension)` is 0.
pub fn rubber_out(distance: f64, dimension: f64) -> f64 {
    (1.0 - 1.0 / ((distance * RUBBER) / dimension + 1.0)) * dimension
}

/// The exact inverse of [`rubber_out`] (defined for `stretch < dimension`).
pub fn rubber_in(stretch: f64, dimension: f64) -> f64 {
    (stretch * dimension) / (RUBBER * (dimension - stretch))
}

/// What one integration step hands back; `done` means at rest.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionStep {
    pub position: f64,
    pub velocity: f64,
    pub done: bool,
}

/// One frame of free glide: exponential velocity decay, the position
/// advanced by the midpoint integral of the decay over `elapsed_ms`.
pub fn glide_step(position: f64, velocity: f64, elapsed_ms: f64) -> MotionStep {
    let decay = FLING_DECAY.powf(elapsed_ms);
    let next_position = position + velocity * ((elapsed_ms * (1.0 + decay)) / 2.0);
    let next_velocity = velocity * decay;
    if next_velocity.abs() < FLING_STOP {
        MotionStep { position: next_position, velocity: 0.0, done: true }
    } else {
        MotionStep { position: next_position, velocity: next_velocity, done: false }
    }
}

/// One frame of a critically-damped spring toward `edge` (semi-implicit
/// Euler). Lands exactly on the edge once within half a pixel at negligible
/// speed.
pub fn spring_step(position: f64, velocity: f64, edge: f64, elapsed_ms: f64) -> MotionStep {
    let next_velocity = velocity
        + (-SPRING_OMEGA * SPRING_OMEGA * (position - edge) - 2.0 * SPRING_OMEGA * velocity)
            * elapsed_ms;
    let next_position = position + next_velocity * elapsed_ms;
    if (next_position - edge).abs() < 0.5 && next_velocity.abs() < 0.01 {
        return MotionStep { position: edge, velocity: 0.0, done: true };
    }
    MotionStep { position: next_position, velocity: next_velocity, done: false }
}

pub fn ease_out_cubic(progress: f64) -> f64 {
    1.0 - (1.0 - progress).powf(3.0)
}

/// Geometric zoom interpolation, `from * (to / from)^progress`.
pub fn zoom_lerp(from: f64, to: f64, progress: f64) -> f64 {
    from * (to / from).powf(progress)
}
