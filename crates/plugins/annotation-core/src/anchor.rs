//! `anchor.ts`: screen-anchored (`noZoom`/`noRotate`) bodies. One projection
//! ([`anchored_geom`]) and its exact inverse ([`unanchored_geom`]), shared
//! by render, hit, chrome and gestures.

use serde::{Deserialize, Serialize};

use crate::flags::FlagBearer;
use crate::geometry::{
    geom_bounds, geom_rotate_about, geom_scale_about, geom_translate, normalize_deg, rotate_point,
};
use crate::js;
use crate::kinds::caps_for;
use crate::types::{Geom, Point, Rect, ViewEnv};

/// Which display factors an annotation is exempt from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnchorMode {
    pub zoom: bool,
    pub upright: bool,
}

/// The anchor behaviours of one annotation: its flags OR'd with the kind's
/// statics. `None` when not anchored.
pub fn anchor_mode_of(a: &impl FlagBearer) -> Option<AnchorMode> {
    let caps = caps_for(a.subtype());
    let zoom = a.flags().no_zoom || caps.no_zoom;
    let upright = a.flags().no_rotate || caps.no_rotate;
    (zoom || upright).then_some(AnchorMode { zoom, upright })
}

/// Free content-space geometry the projection applies to.
fn projectable(g: &Geom) -> bool {
    match g {
        Geom::Rect { .. } | Geom::Line { .. } | Geom::Poly { .. } | Geom::Ink { .. } => true,
        Geom::Text { callout, .. } => callout.is_none(),
        Geom::Quads { .. } | Geom::Caret { .. } => false,
    }
}

/// The fixed page point: the geometry's bounds top-left.
pub fn anchor_of(g: &Geom) -> Point {
    let b = geom_bounds(g);
    Point { x: b.x, y: b.y }
}

const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

struct Factors {
    s: f64,
    r: f64,
}

/// The projection's stored-to-view inverse factors, or `None` for the
/// identity. `s` clamps to `max(zoom, 1)`: below 100% the body scales with
/// the page.
fn factors(mode: Option<AnchorMode>, view: Option<ViewEnv>) -> Option<Factors> {
    let (mode, view) = (mode?, view?);
    let zoom = if view.zoom == 0.0 || view.zoom.is_nan() { 1.0 } else { view.zoom };
    let s = if mode.zoom { js::max(zoom, 1.0) } else { 1.0 };
    let r = if mode.upright { normalize_deg(view.rotation) } else { 0.0 };
    (s != 1.0 || r != 0.0).then_some(Factors { s, r })
}

/// The effective content-space geometry of a screen-anchored body at `view`.
pub fn anchored_geom(g: &Geom, mode: Option<AnchorMode>, view: Option<ViewEnv>) -> Geom {
    let Some(f) = factors(mode, view).filter(|_| projectable(g)) else {
        return g.clone();
    };
    let a = anchor_of(g);
    let mut out = g.clone();
    if f.s != 1.0 {
        out = geom_scale_about(&out, a, 1.0 / f.s, 1.0 / f.s);
    }
    if f.r != 0.0 {
        out = geom_rotate_about(&out, a, normalize_deg(-f.r));
    }
    out
}

/// The exact inverse of [`anchored_geom`]: the stored geometry whose own
/// projection is `target`.
pub fn unanchored_geom(target: &Geom, mode: Option<AnchorMode>, view: Option<ViewEnv>) -> Geom {
    let Some(f) = factors(mode, view).filter(|_| projectable(target)) else {
        return target.clone();
    };
    let mut lin = target.clone();
    if f.s != 1.0 {
        lin = geom_scale_about(&lin, ORIGIN, f.s, f.s);
    }
    if f.r != 0.0 {
        lin = geom_rotate_about(&lin, ORIGIN, f.r);
    }
    let b = anchor_of(&lin);
    let a = rotate_point(b, ORIGIN, -f.r);
    geom_translate(&lin, Point { x: a.x / f.s - b.x, y: a.y / f.s - b.y })
}

/// Stroke width in the projected space.
pub fn anchored_stroke_width(width: f64, mode: Option<AnchorMode>, view: Option<ViewEnv>) -> f64 {
    factors(mode, view).map_or(width, |f| width / f.s)
}

/// A baked raster's box under the projection: the unrotated projected box
/// and the rotation to re-apply. `None` for the identity.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnchoredBox {
    #[serde(rename = "box")]
    pub box_: Rect,
    pub rot: f64,
}

pub fn anchored_box(
    bx: Rect,
    anchor: Point,
    mode: Option<AnchorMode>,
    view: Option<ViewEnv>,
) -> Option<AnchoredBox> {
    let f = factors(mode, view)?;
    let c = Point { x: bx.x + bx.width / 2.0, y: bx.y + bx.height / 2.0 };
    let scaled =
        Point { x: anchor.x + (c.x - anchor.x) / f.s, y: anchor.y + (c.y - anchor.y) / f.s };
    let pc = if f.r != 0.0 { rotate_point(scaled, anchor, normalize_deg(-f.r)) } else { scaled };
    let w = bx.width / f.s;
    let h = bx.height / f.s;
    Some(AnchoredBox {
        box_: Rect { x: pc.x - w / 2.0, y: pc.y - h / 2.0, width: w, height: h },
        rot: normalize_deg(-f.r),
    })
}
