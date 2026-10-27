//! `placement.ts`: where a create gesture puts what it makes, and what a
//! tool's kind lays there.

use crate::geometry::{upright_anchored_rect, upright_rotation};
use crate::js;
use crate::json::Object;
use crate::record::kind_of;
use crate::rect::{rect_from_points, rotate_point, transposed_about_center};
use crate::types::{ClickCreate, ClickPolicy, Placement, Point, Rect, Shape};
use crate::update::page_bound::clamp_point_to_box;

/// The click / drag threshold in content units (`MIN_DRAG`).
pub const MIN_DRAG: f64 = 3.0;

/// What a create gesture lays (`GestureForm`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureForm {
    Box,
    Segment,
}

/// The page a placement is made on, and how the person sees it (`PlacementFrame`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlacementFrame {
    pub page_box: Option<Rect>,
    pub upright: Option<bool>,
    pub display_rotation: Option<f64>,
}

/// `clampRectToBox(rect, box)`: slid inside `box`.
pub fn clamp_rect_to_box(rect: Rect, bounds: Option<Rect>) -> Rect {
    let Some(b) = bounds else { return rect };
    Rect {
        x: js::min(js::max(rect.x, b.x), js::max(b.x, b.x + b.width - rect.width)),
        y: js::min(js::max(rect.y, b.y), js::max(b.y, b.y + b.height - rect.height)),
        ..rect
    }
}

/// `isDrag(form, from, to)`.
pub fn is_drag(form: GestureForm, from: Point, to: Point) -> bool {
    match form {
        GestureForm::Segment => js::hypot(to.x - from.x, to.y - from.y) >= MIN_DRAG,
        GestureForm::Box => (to.x - from.x).abs() >= MIN_DRAG || (to.y - from.y).abs() >= MIN_DRAG,
    }
}

fn upright_turn(frame: &PlacementFrame) -> f64 {
    match (frame.upright, frame.display_rotation) {
        (Some(true), Some(rotation)) if rotation != 0.0 && !rotation.is_nan() => {
            upright_rotation(rotation)
        }
        _ => 0.0,
    }
}

/// `gesturePlacement(form, from, to, clickCreate, frame)`.
pub fn gesture_placement(
    form: GestureForm,
    from: Point,
    to: Point,
    click_create: Option<&ClickPolicy>,
    frame: &PlacementFrame,
) -> Option<Placement> {
    if !is_drag(form, from, to) {
        return click_create
            .and_then(ClickPolicy::create)
            .map(|policy| resolve_click_placement(from, policy, frame));
    }
    let a = match frame.page_box {
        Some(b) => clamp_point_to_box(from, Some(b)),
        None => from,
    };
    let b = match frame.page_box {
        Some(bx) => clamp_point_to_box(to, Some(bx)),
        None => to,
    };
    if form == GestureForm::Segment {
        return Some(Placement::segment(a, b));
    }
    let rot = upright_turn(frame);
    let dragged = rect_from_points(a, b);
    Some(Placement::boxed(
        if rot % 180.0 != 0.0 { transposed_about_center(dragged) } else { dragged },
        rot,
    ))
}

/// `resolveClickPlacement(point, policy, frame)`.
pub fn resolve_click_placement(
    point: Point,
    policy: &ClickCreate,
    frame: &PlacementFrame,
) -> Placement {
    let seen =
        if frame.upright == Some(true) { frame.display_rotation.unwrap_or(0.0) } else { 0.0 };
    if policy.is_line() {
        let length = policy.length();
        let turn = (policy.rotation() * std::f64::consts::PI) / 180.0;
        let span = rotate_point(
            Point { x: turn.cos() * length, y: turn.sin() * length },
            Point { x: 0.0, y: 0.0 },
            -seen,
        );
        let before = match policy.anchor() {
            Some("start") => 0.0,
            Some("end") => 1.0,
            _ => 0.5,
        };
        let a = Point { x: point.x - span.x * before, y: point.y - span.y * before };
        let b = Point { x: a.x + span.x, y: a.y + span.y };
        let bounds = rect_from_points(a, b);
        let placed = clamp_rect_to_box(bounds, frame.page_box);
        let dx = placed.x - bounds.x;
        let dy = placed.y - bounds.y;
        return Placement::segment(
            Point { x: a.x + dx, y: a.y + dy },
            Point { x: b.x + dx, y: b.y + dy },
        );
    }
    let width = policy.width();
    let height = policy.height();
    let rot = upright_turn(frame);
    let rect = if policy.anchor() == Some("top-left") {
        upright_anchored_rect(point, width, height, seen)
    } else {
        Rect { x: point.x - width / 2.0, y: point.y - height / 2.0, width, height }
    };
    let shown = if rot % 180.0 != 0.0 { transposed_about_center(rect) } else { rect };
    let placed = clamp_rect_to_box(shown, frame.page_box);
    Placement::boxed(
        Rect { x: rect.x + placed.x - shown.x, y: rect.y + placed.y - shown.y, ..rect },
        rot,
    )
}

/// `placedShape(annotation, placement)`: the kind's shape at the placement.
pub fn placed_shape(annotation: &Object, placement: &Placement) -> Option<Shape> {
    kind_of(annotation).family.placed(placement, annotation)
}
