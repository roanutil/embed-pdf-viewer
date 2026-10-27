//! The engine's geometry helpers the predictor uses: rects and quarter turns
//! in the file's coordinates (`geometry/convert.ts`), turning points
//! (`geometry/pointTurn.ts`), the flip between page space and y-up
//! coordinates (`geometry/pageSpace.ts`'s mirroring), and their page-space
//! wrappers (`pageSpace/helpers.ts`).

use epdf_geometry::js;
use epdf_geometry::{PdfRect, Point, Quad, Rect};

pub type PdfPoint = Point;

/// `isFinitePdfRect` on four edges.
pub fn is_finite_pdf_rect(r: &PdfRect) -> bool {
    [r.left, r.bottom, r.right, r.top].iter().all(|e| e.is_finite())
}

/// `normalizePdfRect`.
pub fn normalize_pdf_rect(r: PdfRect) -> PdfRect {
    PdfRect {
        left: js::min(r.left, r.right),
        right: js::max(r.left, r.right),
        bottom: js::min(r.top, r.bottom),
        top: js::max(r.top, r.bottom),
    }
}

fn js_mod(a: f64, b: f64) -> f64 {
    a % b
}

/// `quarterTurnOf(degrees)`: the turn as 0, 90, 180 or 270, or `None`.
pub fn quarter_turn_of(degrees: f64) -> Option<f64> {
    let turn = js_mod(js_mod(degrees, 360.0) + 360.0, 360.0);
    if js_mod(turn, 90.0) == 0.0 { Some(turn) } else { None }
}

/// `pdfQuarterTurnBox(rect, rotation)`.
pub fn pdf_quarter_turn_box(rect: PdfRect, rotation: f64) -> PdfRect {
    if rotation == 0.0 || rotation == 180.0 {
        return rect;
    }
    let x = (rect.left + rect.right) / 2.0;
    let y = (rect.bottom + rect.top) / 2.0;
    let half_width = (rect.top - rect.bottom) / 2.0;
    let half_height = (rect.right - rect.left) / 2.0;
    PdfRect {
        left: x - half_width,
        bottom: y - half_height,
        right: x + half_width,
        top: y + half_height,
    }
}

/// `pdfRectTurnedBounds(rect, degrees)`.
pub fn pdf_rect_turned_bounds(rect: PdfRect, degrees: f64) -> PdfRect {
    if let Some(turn) = quarter_turn_of(degrees) {
        return pdf_quarter_turn_box(rect, turn);
    }
    let radians = degrees * std::f64::consts::PI / 180.0;
    let cos = radians.cos().abs();
    let sin = radians.sin().abs();
    let width = rect.right - rect.left;
    let height = rect.top - rect.bottom;
    let half_across = (width * cos + height * sin) / 2.0;
    let half_up = (width * sin + height * cos) / 2.0;
    let x = (rect.left + rect.right) / 2.0;
    let y = (rect.bottom + rect.top) / 2.0;
    PdfRect { left: x - half_across, bottom: y - half_up, right: x + half_across, top: y + half_up }
}

/// `isSamePdfRect`: the same within what a file's numbers hold.
pub fn is_same_pdf_rect(rect: PdfRect, other: PdfRect) -> bool {
    let size = js::max_of(&[
        rect.right - rect.left,
        rect.top - rect.bottom,
        other.right - other.left,
        other.top - other.bottom,
    ]);
    let tolerance = 0.01 + 1e-4 * size;
    (rect.left - other.left).abs() <= tolerance
        && (rect.bottom - other.bottom).abs() <= tolerance
        && (rect.right - other.right).abs() <= tolerance
        && (rect.top - other.top).abs() <= tolerance
}

/// `pdfPointsBounds`: the box around `points`; a zero box at the origin when there are none.
pub fn pdf_points_bounds(points: &[PdfPoint]) -> PdfRect {
    if points.is_empty() {
        return PdfRect { left: 0.0, right: 0.0, bottom: 0.0, top: 0.0 };
    }
    let mut left = f64::INFINITY;
    let mut bottom = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut top = f64::NEG_INFINITY;
    for p in points {
        left = js::min(left, p.x);
        bottom = js::min(bottom, p.y);
        right = js::max(right, p.x);
        top = js::max(top, p.y);
    }
    PdfRect { left, right, bottom, top }
}

/// A turn: degrees clockwise as the page shows it, about `center` (`PdfPointTurn`, `PagePointTurn`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointTurn {
    pub degrees: f64,
    pub center: Point,
}

/// `pdfPointTurned`.
pub fn pdf_point_turned(point: PdfPoint, turn: PointTurn) -> PdfPoint {
    let radians = turn.degrees * std::f64::consts::PI / 180.0;
    let cos = radians.cos();
    let sin = radians.sin();
    let dx = point.x - turn.center.x;
    let dy = point.y - turn.center.y;
    Point { x: turn.center.x + dx * cos + dy * sin, y: turn.center.y - dx * sin + dy * cos }
}

/// `pdfPointUnturned`.
pub fn pdf_point_unturned(point: PdfPoint, turn: PointTurn) -> PdfPoint {
    pdf_point_turned(point, PointTurn { degrees: -turn.degrees, center: turn.center })
}

fn middle_of(rect: PdfRect) -> PdfPoint {
    Point { x: (rect.left + rect.right) / 2.0, y: (rect.bottom + rect.top) / 2.0 }
}

/// `pdfTurnOfUpright`.
pub fn pdf_turn_of_upright(points: &[PdfPoint], degrees: f64) -> PointTurn {
    PointTurn { degrees, center: middle_of(pdf_points_bounds(points)) }
}

/// `pdfTurnOfDrawn`.
pub fn pdf_turn_of_drawn(points: &[PdfPoint], degrees: f64) -> PointTurn {
    let origin = Point { x: 0.0, y: 0.0 };
    let back: Vec<PdfPoint> = points
        .iter()
        .map(|p| pdf_point_turned(*p, PointTurn { degrees: -degrees, center: origin }))
        .collect();
    let middle = middle_of(pdf_points_bounds(&back));
    PointTurn { degrees, center: pdf_point_turned(middle, PointTurn { degrees, center: origin }) }
}

/// `pdfCalloutEnd(box, toward, rotation)`.
pub fn pdf_callout_end(rect: PdfRect, toward: PdfPoint, rotation: f64) -> PdfPoint {
    let center = Point { x: (rect.left + rect.right) / 2.0, y: (rect.bottom + rect.top) / 2.0 };
    let turn = PointTurn { degrees: rotation, center };
    let turned = rotation != 0.0 && !rotation.is_nan();
    let local = if turned { pdf_point_unturned(toward, turn) } else { toward };
    let dx = local.x - center.x;
    let dy = local.y - center.y;
    let end = if dx.abs() >= dy.abs() {
        Point { x: if dx >= 0.0 { rect.right } else { rect.left }, y: center.y }
    } else {
        Point { x: center.x, y: if dy > 0.0 { rect.top } else { rect.bottom } }
    };
    if turned { pdf_point_turned(end, turn) } else { end }
}

// ── the flip between page space and y-up coordinates ───────────────────────

pub fn mirrored_point(p: Point) -> PdfPoint {
    Point { x: p.x, y: -p.y }
}

pub fn unmirrored_point(p: PdfPoint) -> Point {
    Point { x: p.x, y: -p.y }
}

pub fn mirrored_rect(b: Rect) -> PdfRect {
    PdfRect { left: b.x, right: b.x + b.width, bottom: -(b.y + b.height), top: -b.y }
}

pub fn unmirrored_box(r: PdfRect) -> Rect {
    Rect { x: r.left, y: -r.top, width: r.right - r.left, height: r.top - r.bottom }
}

pub fn mirrored_quad(q: Quad) -> Quad {
    Quad {
        upper_left: mirrored_point(q.upper_left),
        upper_right: mirrored_point(q.upper_right),
        lower_left: mirrored_point(q.lower_left),
        lower_right: mirrored_point(q.lower_right),
    }
}

fn mirrored_turn(turn: PointTurn) -> PointTurn {
    PointTurn { degrees: turn.degrees, center: mirrored_point(turn.center) }
}

fn unmirrored_turn(turn: PointTurn) -> PointTurn {
    PointTurn { degrees: turn.degrees, center: unmirrored_point(turn.center) }
}

/// `pagePointTurned`.
pub fn page_point_turned(point: Point, turn: PointTurn) -> Point {
    unmirrored_point(pdf_point_turned(mirrored_point(point), mirrored_turn(turn)))
}

/// `pagePointUnturned`.
pub fn page_point_unturned(point: Point, turn: PointTurn) -> Point {
    unmirrored_point(pdf_point_unturned(mirrored_point(point), mirrored_turn(turn)))
}

/// `pageTurnOfUpright`.
pub fn page_turn_of_upright(points: &[Point], degrees: f64) -> PointTurn {
    let flipped: Vec<PdfPoint> = points.iter().map(|p| mirrored_point(*p)).collect();
    unmirrored_turn(pdf_turn_of_upright(&flipped, degrees))
}

/// `pageTurnOfDrawn`.
pub fn page_turn_of_drawn(points: &[Point], degrees: f64) -> PointTurn {
    let flipped: Vec<PdfPoint> = points.iter().map(|p| mirrored_point(*p)).collect();
    unmirrored_turn(pdf_turn_of_drawn(&flipped, degrees))
}

/// The kinds drawn about a box that can turn (`appearanceTurn.ts`).
const BOX_KINDS: [&str; 5] = ["square", "circle", "free-text", "stamp", "caret"];

/// `pdfAppearanceTurnOf`.
pub fn pdf_appearance_turn_of(
    subtype: &str,
    rect: PdfRect,
    turned_box: Option<PdfRect>,
    rotation: Option<f64>,
    intent: Option<&str>,
) -> Option<f64> {
    let rotation = rotation.filter(|r| *r != 0.0 && !r.is_nan())?;
    let turned_box = turned_box?;
    if !BOX_KINDS.contains(&subtype) {
        return None;
    }
    if subtype == "free-text" && intent == Some("free-text-callout") {
        return None;
    }
    is_same_pdf_rect(pdf_rect_turned_bounds(turned_box, rotation), rect).then_some(rotation)
}

/// `appearanceTurnOf` on a page-space annotation.
pub fn appearance_turn_of(
    subtype: &str,
    rect: Rect,
    turned_box: Option<Rect>,
    rotation: Option<f64>,
    intent: Option<&str>,
) -> Option<f64> {
    pdf_appearance_turn_of(
        subtype,
        mirrored_rect(rect),
        turned_box.map(mirrored_rect),
        rotation,
        intent,
    )
}
