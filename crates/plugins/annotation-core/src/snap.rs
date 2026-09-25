//! `snap.ts`: alignment snapping for the move gesture. One snap per axis,
//! the closest in-threshold key pair wins.

use serde::{Deserialize, Serialize};

use crate::anchor::anchor_mode_of;
use crate::geometry::{selection_quad, union_rect};
use crate::hit::is_selectable;
use crate::js;
use crate::types::{Axis, Guide, Id, Model, PageObjectNumber, Point, Rect};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapResult {
    pub delta: Point,
    pub guides: Vec<Guide>,
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Point,
    max: Point,
}

const GUIDE_PAD: f64 = 14.0;

fn to_bounds(r: Rect) -> Bounds {
    Bounds { min: Point { x: r.x, y: r.y }, max: Point { x: r.x + r.width, y: r.y + r.height } }
}

fn keys_x(b: &Bounds) -> [f64; 3] {
    [b.min.x, (b.min.x + b.max.x) / 2.0, b.max.x]
}

fn keys_y(b: &Bounds) -> [f64; 3] {
    [b.min.y, (b.min.y + b.max.y) / 2.0, b.max.y]
}

fn shift(b: &Bounds, d: Point) -> Bounds {
    Bounds {
        min: Point { x: b.min.x + d.x, y: b.min.y + d.y },
        max: Point { x: b.max.x + d.x, y: b.max.y + d.y },
    }
}

/// An annotation's oriented footprint corners. The TypeScript reads
/// `m.byId[id]` unguarded, so a missing id throws there; it panics here.
fn annot_quad(m: &Model, id: &str) -> [Point; 4] {
    let a = &m.by_id[id];
    selection_quad(&a.geom, a.style.stroke_width, Some(&a.style.border))
}

struct Best {
    adjust: f64,
    at: f64,
    tb: Bounds,
}

/// Snap a move delta against the page box and every other selectable,
/// unanchored annotation on the page.
pub fn compute_move_snap(
    m: &Model,
    ids: &[Id],
    pon: PageObjectNumber,
    raw: Point,
    threshold: f64,
    page_box: Option<Rect>,
) -> SnapResult {
    if ids.iter().any(|id| m.by_id.get(id).is_some_and(|a| anchor_mode_of(a).is_some())) {
        return SnapResult { delta: raw, guides: Vec::new() };
    }
    let corners: Vec<Point> = ids.iter().flat_map(|id| annot_quad(m, id)).collect();
    let base = to_bounds(union_rect(&corners));
    let moving_box = shift(&base, raw);

    let mut targets: Vec<Bounds> = page_box.map(to_bounds).into_iter().collect();
    for id in &m.order {
        let a = &m.by_id[id];
        if !ids.contains(id) && a.pon == pon && is_selectable(m, id) && anchor_mode_of(a).is_none()
        {
            targets.push(to_bounds(union_rect(&annot_quad(m, id))));
        }
    }

    let mut bx: Option<Best> = None;
    let mut by: Option<Best> = None;
    for tb in &targets {
        for tk in keys_x(tb) {
            for mk in keys_x(&moving_box) {
                let diff = tk - mk;
                if diff.abs() < threshold && bx.as_ref().is_none_or(|b| diff.abs() < b.adjust.abs())
                {
                    bx = Some(Best { adjust: diff, at: tk, tb: *tb });
                }
            }
        }
        for tk in keys_y(tb) {
            for mk in keys_y(&moving_box) {
                let diff = tk - mk;
                if diff.abs() < threshold && by.as_ref().is_none_or(|b| diff.abs() < b.adjust.abs())
                {
                    by = Some(Best { adjust: diff, at: tk, tb: *tb });
                }
            }
        }
    }

    let delta = Point {
        x: raw.x + bx.as_ref().map_or(0.0, |b| b.adjust),
        y: raw.y + by.as_ref().map_or(0.0, |b| b.adjust),
    };
    let snapped = shift(&base, delta);
    let mut guides = Vec::new();
    if let Some(b) = &bx {
        guides.push(Guide {
            axis: Axis::X,
            at: b.at,
            lo: js::min(snapped.min.y, b.tb.min.y) - GUIDE_PAD,
            hi: js::max(snapped.max.y, b.tb.max.y) + GUIDE_PAD,
        });
    }
    if let Some(b) = &by {
        guides.push(Guide {
            axis: Axis::Y,
            at: b.at,
            lo: js::min(snapped.min.x, b.tb.min.x) - GUIDE_PAD,
            hi: js::max(snapped.max.x, b.tb.max.x) + GUIDE_PAD,
        });
    }
    SnapResult { delta, guides }
}
