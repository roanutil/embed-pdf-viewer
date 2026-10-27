//! `update/page-bound.ts`: gestures stay on their page. A sample from
//! another page is ignored, geometry pins to the page box, and a reshaping
//! gesture stops where its frame would reach further past the edge.

use crate::js;
use crate::rect::union_rect;
use crate::selection::{Live, annotation_selection_frame};
use crate::types::{Draft, Id, Model, PageRef, Point, PointerInput, Rect, ViewEnv};

fn clamp_axis(value: f64, lo: f64, hi: f64) -> f64 {
    js::max(lo, js::min(hi, value))
}

/// `clampPointToBox(point, box)`.
pub fn clamp_point_to_box(point: Point, bounds: Option<Rect>) -> Point {
    match bounds {
        Some(b) => Point {
            x: clamp_axis(point.x, b.x, b.x + b.width),
            y: clamp_axis(point.y, b.y, b.y + b.height),
        },
        None => point,
    }
}

/// `viewOf(input)`: the sample's view, when the caller gave one.
pub fn view_of(input: &PointerInput) -> Option<ViewEnv> {
    if input.zoom.is_some() || input.display_rotation.is_some() {
        Some(ViewEnv::new(input.zoom.unwrap_or(1.0), input.display_rotation.unwrap_or(0.0)))
    } else {
        None
    }
}

fn overhang_of(corners: &[Point], page: Rect) -> f64 {
    let mut overhang: f64 = 0.0;
    for p in corners {
        overhang = js::max_of(&[
            overhang,
            page.x - p.x,
            p.x - (page.x + page.width),
            page.y - p.y,
            p.y - (page.y + page.height),
        ]);
    }
    overhang
}

/// `fractionOnPage(page, cornersAt)`.
pub fn fraction_on_page(page: Option<Rect>, mut corners_at: impl FnMut(f64) -> Vec<Point>) -> f64 {
    let Some(page) = page else { return 1.0 };
    let allowed = overhang_of(&corners_at(0.0), page) + 1e-9;
    let mut fits = |fraction: f64| overhang_of(&corners_at(fraction), page) <= allowed;
    if fits(1.0) {
        return 1.0;
    }
    let mut inside = 0.0;
    let mut outside = 1.0;
    for _ in 0..30 {
        let middle = (inside + outside) / 2.0;
        if fits(middle) {
            inside = middle;
        } else {
            outside = middle;
        }
    }
    inside
}

/// `deltaOnPage(page, wanted, cornersFor)`.
pub fn delta_on_page(
    page: Option<Rect>,
    wanted: Point,
    mut corners_for: impl FnMut(Point) -> Vec<Point>,
) -> Point {
    let x = wanted.x * fraction_on_page(page, |f| corners_for(Point { x: wanted.x * f, y: 0.0 }));
    let y = wanted.y * fraction_on_page(page, |f| corners_for(Point { x, y: wanted.y * f }));
    Point { x, y }
}

/// `unionBoundsOf(model, ids, view)`: the union of the ids' selection frames.
pub fn union_bounds_of(model: &Model, ids: &[Id], view: Option<&ViewEnv>) -> Option<Rect> {
    let corners: Vec<Point> = ids
        .iter()
        .filter_map(|id| model.record(id))
        .flat_map(|r| annotation_selection_frame(r, view, &Live::default()).corners)
        .collect();
    (!corners.is_empty()).then(|| union_rect(&corners))
}

/// `clampMoveDelta(model, ids, delta, page, view)`.
pub fn clamp_move_delta(
    model: &Model,
    ids: &[Id],
    delta: Point,
    page: Option<Rect>,
    view: Option<&ViewEnv>,
) -> Point {
    let Some(page) = page else { return delta };
    let Some(rect) = union_bounds_of(model, ids, view) else { return delta };
    Point {
        x: clamp_axis(delta.x, page.x - rect.x, page.x + page.width - (rect.x + rect.width)),
        y: clamp_axis(delta.y, page.y - rect.y, page.y + page.height - (rect.y + rect.height)),
    }
}

/// `editDraftPage(model, draft)`: the page an edit draft is anchored to.
pub fn edit_draft_page(model: &Model, draft: &Draft) -> Option<PageRef> {
    let id = match draft {
        Draft::Handle(d) => Some(&d.id),
        Draft::Caption(d) => Some(&d.id),
        Draft::Leader(d) => Some(&d.id),
        Draft::Move(d) => d.ids.first(),
        Draft::Rotate(d) => d.ids.first(),
        Draft::Group(d) => d.ids.first(),
        _ => None,
    }?;
    model.record(id).map(|r| r.page())
}
