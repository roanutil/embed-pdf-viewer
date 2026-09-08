//! The Rust port of `@poc/geometry`.
//!
//! Two boundary styles live side by side on purpose, because the choice
//! between them is the entire question this POC exists to answer:
//!
//! * The FLAT surface (`rect_quad`, `quad_scale_about`, ...) moves `f64`
//!   slices. wasm-bindgen copies the slice into linear memory on the way in
//!   and allocates one `Float64Array` on the way out. That is the cheap path.
//!
//! * The OBJECT surface (`rect_quad_objects`) moves the same data as JS
//!   objects through `serde-wasm-bindgen`. It reads better from TypeScript and
//!   costs noticeably more, because every `{x, y}` becomes a real JS object.
//!
//! Neither surface can reproduce the one thing the TypeScript original does
//! for free: return the caller's own array back when the transform is the
//! identity. Crossing the boundary always allocates.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

pub mod core;

pub use crate::core::{
    normalize_deg as core_normalize_deg, point_in_quad as core_point_in_quad,
    quad_bounds as core_quad_bounds, quad_rotate_about as core_quad_rotate_about,
    quad_scale_about as core_quad_scale_about, quad_translate as core_quad_translate,
    rect_quad as core_rect_quad, rotate_point as core_rotate_point, Quad, Rect, Point,
};

// ---------------------------------------------------------------------------
// Rust -> TypeScript. The reverse direction of the interop.
// ---------------------------------------------------------------------------

thread_local! {
    static HOST_LOG: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// Hands the crate a host logger. This is the reverse direction: Rust holding a
/// JS function and calling it later, which is how a real core would reach a
/// platform service it cannot implement itself.
#[wasm_bindgen(js_name = setHostLogger)]
pub fn set_host_logger(f: &js_sys::Function) {
    HOST_LOG.with(|slot| *slot.borrow_mut() = Some(f.clone()));
}

/// The borrow ends BEFORE the call into JS, and that ordering is the point.
///
/// Holding a `RefCell` borrow across a call into JavaScript is how a Rust core
/// kills itself: the host callback can re-enter Rust, and `set_host_logger`
/// takes `borrow_mut()`. `init.ts` re-registers the logger on every
/// `initGeometry()` call, including when the module is already up, so a host
/// logger that reached back through it panicked with `BorrowMutError`, and a
/// wasm panic leaves the instance dead for the rest of the page.
fn host_log(message: &str) {
    let logger = HOST_LOG.with(|slot| slot.borrow().clone());
    if let Some(f) = logger {
        let _ = f.call1(&JsValue::NULL, &JsValue::from_str(message));
    }
}

/// Calls the host logger `n` times and reports how many calls actually landed.
/// Exists so the demo can measure the Rust -> TypeScript direction instead of
/// guessing at it.
#[wasm_bindgen(js_name = benchHostCalls)]
pub fn bench_host_calls(n: u32) -> u32 {
    // Cloned out for the same reason as `host_log`, and it matters more here:
    // this held one borrow across `n` calls into JS.
    let Some(f) = HOST_LOG.with(|slot| slot.borrow().clone()) else {
        return 0;
    };
    let mut landed = 0u32;
    for i in 0..n {
        if f.call1(&JsValue::NULL, &JsValue::from_f64(i as f64)).is_ok() {
            landed += 1;
        }
    }
    landed
}

/// Announces the crate to the host. Called by the TS wrapper after init, so
/// the demo can show that the reverse direction works before anything else runs.
#[wasm_bindgen(js_name = greetHost)]
pub fn greet_host() {
    host_log(&format!("poc-geometry {} ready", env!("CARGO_PKG_VERSION")));
}

// ---------------------------------------------------------------------------
// The flat surface. f64 slices in, Float64Array out.
// ---------------------------------------------------------------------------

fn quad_from_slice(q: &[f64]) -> Option<Quad> {
    if q.len() != 8 {
        return None;
    }
    Some(Quad([
        Point { x: q[0], y: q[1] },
        Point { x: q[2], y: q[3] },
        Point { x: q[4], y: q[5] },
        Point { x: q[6], y: q[7] },
    ]))
}

fn quad_to_vec(q: Quad) -> Vec<f64> {
    let mut out = Vec::with_capacity(8);
    for p in q.0 {
        out.push(p.x);
        out.push(p.y);
    }
    out
}

fn bad_quad(name: &str) -> Vec<f64> {
    host_log(&format!("{name}: expected 8 floats, got a malformed quad"));
    vec![f64::NAN; 8]
}

#[wasm_bindgen(js_name = normalizeDeg)]
pub fn normalize_deg(deg: f64) -> f64 {
    crate::core::normalize_deg(deg)
}

#[wasm_bindgen(js_name = rotatePoint)]
pub fn rotate_point(px: f64, py: f64, ax: f64, ay: f64, deg: f64) -> Vec<f64> {
    let r = crate::core::rotate_point(Point { x: px, y: py }, Point { x: ax, y: ay }, deg);
    vec![r.x, r.y]
}

#[wasm_bindgen(js_name = rectQuad)]
pub fn rect_quad(x: f64, y: f64, width: f64, height: f64, rot: f64) -> Vec<f64> {
    quad_to_vec(crate::core::rect_quad(
        Rect {
            x,
            y,
            width,
            height,
        },
        rot,
    ))
}

#[wasm_bindgen(js_name = quadScaleAbout)]
pub fn quad_scale_about(q: &[f64], ax: f64, ay: f64, s: f64) -> Vec<f64> {
    match quad_from_slice(q) {
        Some(quad) => quad_to_vec(crate::core::quad_scale_about(
            quad,
            Point { x: ax, y: ay },
            s,
        )),
        None => bad_quad("quadScaleAbout"),
    }
}

#[wasm_bindgen(js_name = quadRotateAbout)]
pub fn quad_rotate_about(q: &[f64], ax: f64, ay: f64, deg: f64) -> Vec<f64> {
    match quad_from_slice(q) {
        Some(quad) => quad_to_vec(crate::core::quad_rotate_about(
            quad,
            Point { x: ax, y: ay },
            deg,
        )),
        None => bad_quad("quadRotateAbout"),
    }
}

#[wasm_bindgen(js_name = quadTranslate)]
pub fn quad_translate(q: &[f64], dx: f64, dy: f64) -> Vec<f64> {
    match quad_from_slice(q) {
        Some(quad) => quad_to_vec(crate::core::quad_translate(quad, Point { x: dx, y: dy })),
        None => bad_quad("quadTranslate"),
    }
}

#[wasm_bindgen(js_name = quadBounds)]
pub fn quad_bounds(q: &[f64]) -> Vec<f64> {
    match quad_from_slice(q) {
        Some(quad) => {
            let b = crate::core::quad_bounds(quad);
            vec![b.x, b.y, b.width, b.height]
        }
        None => {
            host_log("quadBounds: expected 8 floats, got a malformed quad");
            vec![f64::NAN; 4]
        }
    }
}

#[wasm_bindgen(js_name = pointInQuad)]
pub fn point_in_quad(px: f64, py: f64, q: &[f64]) -> bool {
    match quad_from_slice(q) {
        Some(quad) => crate::core::point_in_quad(Point { x: px, y: py }, quad),
        None => {
            host_log("pointInQuad: expected 8 floats, got a malformed quad");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// The object surface. Same math, ergonomic types, measurably more expensive.
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
pub struct RectDto {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize)]
pub struct VecDto {
    pub x: f64,
    pub y: f64,
}

/// The same `rectQuad`, but speaking `{x, y, width, height}` and returning
/// `[{x, y}, ...]`. Kept so the demo can measure what the nicer signature costs.
#[wasm_bindgen(js_name = rectQuadObjects)]
pub fn rect_quad_objects(rect: JsValue, rot: f64) -> Result<JsValue, JsValue> {
    let r: RectDto = serde_wasm_bindgen::from_value(rect)?;
    let quad = crate::core::rect_quad(
        Rect {
            x: r.x,
            y: r.y,
            width: r.width,
            height: r.height,
        },
        rot,
    );
    let dto: Vec<VecDto> = quad.0.iter().map(|p| VecDto { x: p.x, y: p.y }).collect();
    Ok(serde_wasm_bindgen::to_value(&dto)?)
}
