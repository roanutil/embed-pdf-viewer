//! `snap.ts`: alignment snapping for the move gesture.

use crate::anchor::anchor_mode_of;
use crate::geometry::selection_quad;
use crate::hit::is_selectable;
use crate::js;
use crate::record::{shape_of, style_of};
use crate::rect::union_rect;
use crate::types::{Guide, Id, Model, PageRef, Point, Rect};

/// A snapped move: the adjusted delta and the guides to draw (`SnapResult`).
#[derive(Clone, Debug, PartialEq)]
pub struct SnapResult {
    pub delta: Point,
    pub guides: Vec<Guide>,
}

const GUIDE_PAD: f64 = 14.0;

#[derive(Clone, Copy)]
struct Bounds {
    min: Point,
    max: Point,
}

fn to_bounds(rect: Rect) -> Bounds {
    Bounds {
        min: Point { x: rect.x, y: rect.y },
        max: Point { x: rect.x + rect.width, y: rect.y + rect.height },
    }
}

fn keys_x(b: &Bounds) -> [f64; 3] {
    [b.min.x, (b.min.x + b.max.x) / 2.0, b.max.x]
}

fn keys_y(b: &Bounds) -> [f64; 3] {
    [b.min.y, (b.min.y + b.max.y) / 2.0, b.max.y]
}

fn shift(b: &Bounds, p: Point) -> Bounds {
    Bounds {
        min: Point { x: b.min.x + p.x, y: b.min.y + p.y },
        max: Point { x: b.max.x + p.x, y: b.max.y + p.y },
    }
}

/// A record's selection quad; nothing for an id the model doesn't hold
/// (where the TypeScript throws reading it, this snaps to what is there).
fn annot_quad(model: &Model, id: &str) -> Vec<Point> {
    match model.record(id) {
        Some(record) => {
            selection_quad(&shape_of(&record.annotation), style_of(&record.annotation).stroke())
                .to_vec()
        }
        None => Vec::new(),
    }
}

fn guide(axis: &str, at: f64, lo: f64, hi: f64) -> Guide {
    Guide { axis: axis.into(), at, lo, hi, order: Default::default() }
}

/// `computeMoveSnap(model, ids, page, raw, threshold, pageBox)`.
pub fn compute_move_snap(
    model: &Model,
    ids: &[Id],
    page: &PageRef,
    raw: Point,
    threshold: f64,
    page_box: Option<Rect>,
) -> SnapResult {
    if ids.iter().any(|id| model.record(id).is_some_and(|r| anchor_mode_of(r).is_some())) {
        return SnapResult { delta: raw, guides: Vec::new() };
    }
    let corners: Vec<Point> = ids.iter().flat_map(|id| annot_quad(model, id)).collect();
    let base = to_bounds(union_rect(&corners));
    let moving_box = shift(&base, raw);
    let mut targets: Vec<Bounds> = page_box.map(to_bounds).into_iter().collect();
    for id in model.order.iter() {
        if ids.contains(id) {
            continue;
        }
        let Some(record) = model.record(id) else { continue };
        if record.page_number() != Some(page.object_number)
            || !is_selectable(model, id)
            || anchor_mode_of(record).is_some()
        {
            continue;
        }
        targets.push(to_bounds(union_rect(&annot_quad(model, id))));
    }
    let mut bx: Option<(f64, f64, Bounds)> = None;
    let mut by: Option<(f64, f64, Bounds)> = None;
    for tb in &targets {
        for tk in keys_x(tb) {
            for mk in keys_x(&moving_box) {
                let diff = tk - mk;
                if diff.abs() < threshold
                    && bx.is_none_or(|(adjust, _, _)| diff.abs() < adjust.abs())
                {
                    bx = Some((diff, tk, *tb));
                }
            }
        }
        for tk in keys_y(tb) {
            for mk in keys_y(&moving_box) {
                let diff = tk - mk;
                if diff.abs() < threshold
                    && by.is_none_or(|(adjust, _, _)| diff.abs() < adjust.abs())
                {
                    by = Some((diff, tk, *tb));
                }
            }
        }
    }
    let delta = Point { x: raw.x + bx.map_or(0.0, |b| b.0), y: raw.y + by.map_or(0.0, |b| b.0) };
    let snapped = shift(&base, delta);
    let mut guides = Vec::new();
    if let Some((_, at, tb)) = bx {
        guides.push(guide(
            "x",
            at,
            js::min(snapped.min.y, tb.min.y) - GUIDE_PAD,
            js::max(snapped.max.y, tb.max.y) + GUIDE_PAD,
        ));
    }
    if let Some((_, at, tb)) = by {
        guides.push(guide(
            "y",
            at,
            js::min(snapped.min.x, tb.min.x) - GUIDE_PAD,
            js::max(snapped.max.x, tb.max.x) + GUIDE_PAD,
        ));
    }
    SnapResult { delta, guides }
}
