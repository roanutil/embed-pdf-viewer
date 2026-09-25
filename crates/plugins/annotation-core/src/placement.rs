//! `placement.ts`: where a bare click puts a box or a segment, page-clamped,
//! and the annotation geometry that placement becomes.

use serde::{Deserialize, Serialize};

use crate::geometry::{
    rect_from_points, shape_rect_for, transposed_about_center, upright_anchored_rect,
    upright_rotation,
};
use crate::js;
use crate::props::style_from_props;
use crate::types::{AnnotationProps, BoxAnchor, ClickCreate, Geom, Point, Rect};

/// A resolved click placement.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ClickPlacement {
    Box { rect: Rect, rot: f64 },
    Segment { a: Point, b: Point },
}

/// Slide a rect (as a unit) inside `bx`, pinning at the origin edge when it
/// doesn't fit.
pub fn clamp_rect_to_box(r: Rect, bx: Option<Rect>) -> Rect {
    let Some(bx) = bx else {
        return r;
    };
    Rect {
        x: js::min(js::max(r.x, bx.x), js::max(bx.x, bx.x + bx.width - r.width)),
        y: js::min(js::max(r.y, bx.y), js::max(bx.y, bx.y + bx.height - r.height)),
        ..r
    }
}

/// The options `resolveClickPlacement` reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacementOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_box: Option<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upright: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_rotation: Option<f64>,
}

/// Resolve a click-create policy at a content point.
pub fn resolve_click_placement(
    point: Point,
    policy: &ClickCreate,
    opts: &PlacementOptions,
) -> ClickPlacement {
    if let Some(length) = policy.length {
        let ang = (policy.angle_deg.unwrap_or(0.0) * std::f64::consts::PI) / 180.0;
        let b = Point { x: point.x + ang.cos() * length, y: point.y + ang.sin() * length };
        let bounds = rect_from_points(point, b);
        let placed = clamp_rect_to_box(bounds, opts.page_box);
        let dx = placed.x - bounds.x;
        let dy = placed.y - bounds.y;
        return ClickPlacement::Segment {
            a: Point { x: point.x + dx, y: point.y + dy },
            b: Point { x: b.x + dx, y: b.y + dy },
        };
    }
    let width = policy.width.unwrap_or(f64::NAN);
    let height = policy.height.unwrap_or(f64::NAN);
    let display_rotation = opts.display_rotation.unwrap_or(0.0);
    let rot = if opts.upright == Some(true) && display_rotation != 0.0 && !display_rotation.is_nan()
    {
        upright_rotation(display_rotation)
    } else {
        0.0
    };
    let rect = if policy.anchor == Some(BoxAnchor::TopLeft) {
        if rot != 0.0 && !rot.is_nan() {
            upright_anchored_rect(point, width, height, display_rotation)
        } else {
            Rect { x: point.x, y: point.y, width, height }
        }
    } else {
        let r = Rect { x: point.x - width / 2.0, y: point.y - height / 2.0, width, height };
        if rot == 90.0 || rot == 270.0 { transposed_about_center(r) } else { r }
    };
    ClickPlacement::Box { rect: clamp_rect_to_box(rect, opts.page_box), rot }
}

/// Annotation only: the geometry a placement commits as, for the kinds a
/// click can author; `None` otherwise.
pub fn click_create_geom(
    subtype: &str,
    placement: &ClickPlacement,
    def: &AnnotationProps,
) -> Option<Geom> {
    let rot_of = |rot: f64| (rot != 0.0 && !rot.is_nan()).then_some(rot);
    match *placement {
        ClickPlacement::Segment { a, b } => (subtype == "line").then_some(Geom::Line {
            a,
            b,
            ends: Some(def.line_endings),
            rot: None,
        }),
        ClickPlacement::Box { rect, rot } => match subtype {
            "free-text" => Some(Geom::Text { rect, callout: None, rot: rot_of(rot) }),
            "square" | "circle" => {
                let ellipse = subtype == "circle";
                Some(Geom::Rect {
                    rect: shape_rect_for(rect, ellipse, &style_from_props(def)),
                    ellipse,
                    rot: rot_of(rot),
                })
            }
            _ => None,
        },
    }
}
