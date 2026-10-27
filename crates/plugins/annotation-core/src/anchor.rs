//! `anchor.ts`: screen-anchored annotations (`noZoom` / `noRotate`). One
//! projection ([`anchored_geom`]) and its exact inverse
//! ([`unanchored_geom`]), shared by render, hit, chrome and gestures.

use crate::flags::FlagBearer;
use crate::geometry::{geom_bounds, geom_rotate_about, geom_scale_about, geom_translate};
use crate::js_object;
use crate::json;
use crate::object::Json;
use crate::record::kind_of;
use crate::rect::{normalize_deg, rotate_point};
use crate::types::{Point, Rect, Shape, ViewEnv};

js_object! {
    /// Which display factors an annotation is exempt from (`AnchorMode`).
    pub struct AnchorMode {
        "zoom" => pub zoom: bool,
        "upright" => pub upright: bool,
    }
}

/// `anchorModeOf(record)`: its `/F` flags or'd with its kind's statics.
pub fn anchor_mode_of(record: &dyn FlagBearer) -> Option<AnchorMode> {
    let annotation = record.bearer_annotation();
    let caps = kind_of(annotation).caps;
    let zoom = json::truthy(annotation.get("noZoom")) || caps.no_zoom;
    let upright = json::truthy(annotation.get("noRotate")) || caps.no_rotate;
    (zoom || upright).then(|| AnchorMode { zoom, upright, order: Default::default() })
}

fn projectable(geometry: &Shape) -> bool {
    match geometry {
        Shape::Box(_) | Shape::Line(_) | Shape::Poly(_) | Shape::Ink(_) => true,
        Shape::TextBox(s) => s.callout_line.is_null(),
        _ => false,
    }
}

/// `anchorOf(geometry)`: the bounds' top-left.
pub fn anchor_of(geometry: &Shape) -> Point {
    let rect = geom_bounds(geometry);
    Point { x: rect.x, y: rect.y }
}

const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

/// The projection's factors `(s, r)`, `None` when it is the identity.
fn factors(mode: Option<&AnchorMode>, view: Option<&ViewEnv>) -> Option<(f64, f64)> {
    let (mode, view) = (mode?, view?);
    let zoom = if view.zoom == 0.0 || view.zoom.is_nan() { 1.0 } else { view.zoom };
    let scale = if mode.zoom { crate::js::max(zoom, 1.0) } else { 1.0 };
    let rotation = if mode.upright { normalize_deg(view.rotation) } else { 0.0 };
    (scale != 1.0 || rotation != 0.0).then_some((scale, rotation))
}

/// `anchoredScale(mode, view)`.
pub fn anchored_scale(mode: Option<&AnchorMode>, view: Option<&ViewEnv>) -> f64 {
    factors(mode, view).map_or(1.0, |(s, _)| 1.0 / s)
}

/// `anchoredGeom(geometry, mode, view)`.
pub fn anchored_geom(geometry: &Shape, mode: Option<&AnchorMode>, view: Option<&ViewEnv>) -> Shape {
    let Some((s, r)) = factors(mode, view) else { return geometry.clone() };
    if !projectable(geometry) {
        return geometry.clone();
    }
    let point = anchor_of(geometry);
    let mut out = geometry.clone();
    if s != 1.0 {
        out = geom_scale_about(&out, point, 1.0 / s, 1.0 / s, None);
    }
    if r != 0.0 {
        out = geom_rotate_about(&out, point, normalize_deg(-r));
    }
    out
}

/// `unanchoredGeom(target, mode, view)`: the exact inverse.
pub fn unanchored_geom(target: &Shape, mode: Option<&AnchorMode>, view: Option<&ViewEnv>) -> Shape {
    let Some((s, r)) = factors(mode, view) else { return target.clone() };
    if !projectable(target) {
        return target.clone();
    }
    let mut lin = target.clone();
    if s != 1.0 {
        lin = geom_scale_about(&lin, ORIGIN, s, s, None);
    }
    if r != 0.0 {
        lin = geom_rotate_about(&lin, ORIGIN, r);
    }
    let point = anchor_of(&lin);
    let unrotated = rotate_point(point, ORIGIN, -r);
    geom_translate(&lin, Point { x: unrotated.x / s - point.x, y: unrotated.y / s - point.y })
}

/// `anchoredStrokeWidth(width, mode, view)`.
pub fn anchored_stroke_width(width: f64, mode: Option<&AnchorMode>, view: Option<&ViewEnv>) -> f64 {
    factors(mode, view).map_or(width, |(s, _)| width / s)
}

/// `anchoredBox(box, anchor, mode, view)`: a raster box under the projection, and its turn.
pub fn anchored_box(
    rect: Rect,
    anchor: Point,
    mode: Option<&AnchorMode>,
    view: Option<&ViewEnv>,
) -> Option<(Rect, f64)> {
    let (s, r) = factors(mode, view)?;
    let center = Point { x: rect.x + rect.width / 2.0, y: rect.y + rect.height / 2.0 };
    let scaled =
        Point { x: anchor.x + (center.x - anchor.x) / s, y: anchor.y + (center.y - anchor.y) / s };
    let pc = if r != 0.0 { rotate_point(scaled, anchor, normalize_deg(-r)) } else { scaled };
    let width = rect.width / s;
    let height = rect.height / s;
    Some((Rect { x: pc.x - width / 2.0, y: pc.y - height / 2.0, width, height }, normalize_deg(-r)))
}

/// The anchored box as the TypeScript object prints it: `{ box, rot }`.
pub fn anchored_box_value(result: Option<(Rect, f64)>) -> serde_json::Value {
    match result {
        None => serde_json::Value::Null,
        Some((rect, rot)) => {
            serde_json::Value::Object(json::object([("box", rect.json()), ("rot", json::num(rot))]))
        }
    }
}
