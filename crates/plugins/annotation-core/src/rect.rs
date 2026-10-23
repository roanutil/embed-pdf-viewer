//! `rect.ts`: plain page-space math. Rect corners, the eight resize
//! handles, resizing (upright or turned about the middle), turning points
//! about a pivot, and point-in-polygon.

use epdf_geometry::{Point, Rect, apply_point, js, rotate_about};

pub const MIN_SIZE: f64 = 4.0;

pub const RECT_HANDLES: [&str; 8] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

/// `RECT_CURSOR[handle]`.
pub fn rect_cursor(handle: &str) -> &'static str {
    match handle {
        "nw" | "se" => "nwse-resize",
        "ne" | "sw" => "nesw-resize",
        "n" | "s" => "ns-resize",
        _ => "ew-resize",
    }
}

fn handle_base_angle(handle: &str) -> f64 {
    match handle {
        "n" => 0.0,
        "ne" => 45.0,
        "e" => 90.0,
        "se" => 135.0,
        "s" => 180.0,
        "sw" => 225.0,
        "w" => 270.0,
        "nw" => 315.0,
        _ => f64::NAN,
    }
}

const SECTOR_CURSORS: [&str; 8] = [
    "ns-resize",
    "nesw-resize",
    "ew-resize",
    "nwse-resize",
    "ns-resize",
    "nesw-resize",
    "ew-resize",
    "nwse-resize",
];

/// `SECTOR_CURSORS[i]`, `undefined` (as `None`) out of range.
fn sector_cursor(index: f64) -> Option<&'static str> {
    if index.is_finite() && index >= 0.0 && index.fract() == 0.0 {
        SECTOR_CURSORS.get(index as usize).copied()
    } else {
        None
    }
}

/// `rotatedHandleCursor(handle, rot)`; `None` where the TypeScript reads past the table.
pub fn rotated_handle_cursor(handle: &str, rot: f64) -> Option<&'static str> {
    sector_cursor(js::round(normalize_deg(handle_base_angle(handle) + rot) / 45.0) % 8.0)
}

/// `cursorOnScreen(cursor, rotation)`.
pub fn cursor_on_screen(cursor: &str, rotation: f64) -> String {
    match SECTOR_CURSORS.iter().position(|c| *c == cursor) {
        None => cursor.to_string(),
        Some(sector) => {
            let index = (sector as f64 + js::round(normalize_deg(rotation) / 45.0)) % 8.0;
            sector_cursor(index).map_or_else(|| "undefined".into(), str::to_string)
        }
    }
}

struct Edges {
    w: bool,
    e: bool,
    n: bool,
    s: bool,
}

fn rect_edges(handle: &str) -> Edges {
    Edges {
        w: matches!(handle, "nw" | "w" | "sw"),
        e: matches!(handle, "ne" | "e" | "se"),
        n: matches!(handle, "nw" | "n" | "ne"),
        s: matches!(handle, "sw" | "s" | "se"),
    }
}

/// `rectHandlePoint(rect, handle)`.
pub fn rect_handle_point(rect: Rect, handle: &str) -> Point {
    let edges = rect_edges(handle);
    Point {
        x: if edges.w {
            rect.x
        } else if edges.e {
            rect.x + rect.width
        } else {
            rect.x + rect.width / 2.0
        },
        y: if edges.n {
            rect.y
        } else if edges.s {
            rect.y + rect.height
        } else {
            rect.y + rect.height / 2.0
        },
    }
}

/// `resizeRect(base, handle, to)`.
pub fn resize_rect(base: Rect, handle: &str, to: Point) -> Rect {
    let edges = rect_edges(handle);
    let mut left = base.x;
    let mut right = base.x + base.width;
    let mut top = base.y;
    let mut bottom = base.y + base.height;
    if edges.w {
        left = to.x;
    }
    if edges.e {
        right = to.x;
    }
    if edges.n {
        top = to.y;
    }
    if edges.s {
        bottom = to.y;
    }
    Rect {
        x: js::min(left, right),
        y: js::min(top, bottom),
        width: js::max(MIN_SIZE, (right - left).abs()),
        height: js::max(MIN_SIZE, (bottom - top).abs()),
    }
}

/// `rectFromPoints(from, to)`.
pub fn rect_from_points(from: Point, to: Point) -> Rect {
    Rect {
        x: js::min(from.x, to.x),
        y: js::min(from.y, to.y),
        width: (to.x - from.x).abs(),
        height: (to.y - from.y).abs(),
    }
}

/// `rectsIntersect(left, right)`, edges included.
pub fn rects_intersect(left: Rect, right: Rect) -> bool {
    left.x <= right.x + right.width
        && left.x + left.width >= right.x
        && left.y <= right.y + right.height
        && left.y + left.height >= right.y
}

/// `rectContains(rect, point)`.
pub fn rect_contains(rect: Rect, point: Point) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

/// `segDist(point, from, to)`: distance from a point to a segment.
pub fn seg_dist(point: Point, from: Point, to: Point) -> f64 {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let len2 = dx * dx + dy * dy;
    let fraction = if len2 == 0.0 {
        0.0
    } else {
        js::max(0.0, js::min(1.0, ((point.x - from.x) * dx + (point.y - from.y) * dy) / len2))
    };
    js::hypot(point.x - (from.x + fraction * dx), point.y - (from.y + fraction * dy))
}

/// `pointRectDistance(point, rect)`.
pub fn point_rect_distance(point: Point, rect: Rect) -> f64 {
    let dx = js::max_of(&[rect.x - point.x, 0.0, point.x - (rect.x + rect.width)]);
    let dy = js::max_of(&[rect.y - point.y, 0.0, point.y - (rect.y + rect.height)]);
    js::hypot(dx, dy)
}

/// `segmentCrossesRect(from, to, rect)`.
pub fn segment_crosses_rect(from: Point, to: Point, rect: Rect) -> bool {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let sides = [
        (-dx, from.x - rect.x),
        (dx, rect.x + rect.width - from.x),
        (-dy, from.y - rect.y),
        (dy, rect.y + rect.height - from.y),
    ];
    let mut enter = 0.0;
    let mut leave = 1.0;
    for (step, room) in sides {
        if step == 0.0 {
            if room < 0.0 {
                return false;
            }
            continue;
        }
        let t = room / step;
        if step < 0.0 {
            enter = js::max(enter, t);
        } else {
            leave = js::min(leave, t);
        }
        if enter > leave {
            return false;
        }
    }
    true
}

/// `segmentRectDistance(from, to, rect)`.
pub fn segment_rect_distance(from: Point, to: Point, rect: Rect) -> f64 {
    if segment_crosses_rect(from, to, rect) {
        return 0.0;
    }
    let mut values = vec![point_rect_distance(from, rect), point_rect_distance(to, rect)];
    values.extend(rect_corner_points(rect).iter().map(|c| seg_dist(*c, from, to)));
    js::min_of(&values)
}

/// `polygonTouchesRect(ring, rect)`.
pub fn polygon_touches_rect(ring: &[Point], rect: Rect) -> bool {
    if ring.iter().any(|v| rect_contains(rect, *v)) {
        return true;
    }
    if point_in_poly(Point { x: rect.x, y: rect.y }, ring) {
        return true;
    }
    (0..ring.len()).any(|i| segment_crosses_rect(ring[i], ring[(i + 1) % ring.len()], rect))
}

/// `pointInPoly(point, points)`: even-odd.
pub fn point_in_poly(point: Point, points: &[Point]) -> bool {
    let mut inside = false;
    let n = points.len();
    if n == 0 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let vertex = points[i];
        let previous = points[j];
        if (vertex.y > point.y) != (previous.y > point.y)
            && point.x
                < ((previous.x - vertex.x) * (point.y - vertex.y)) / (previous.y - vertex.y)
                    + vertex.x
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// `unionRect(points)`: the box around the points (infinite for none).
pub fn union_rect(points: &[Point]) -> Rect {
    let mut x0 = f64::INFINITY;
    let mut y0 = f64::INFINITY;
    let mut x1 = f64::NEG_INFINITY;
    let mut y1 = f64::NEG_INFINITY;
    for p in points {
        x0 = js::min(x0, p.x);
        y0 = js::min(y0, p.y);
        x1 = js::max(x1, p.x);
        y1 = js::max(y1, p.y);
    }
    Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
}

/// `expandRect(rect, pad)`.
pub fn expand_rect(rect: Rect, pad: f64) -> Rect {
    Rect {
        x: rect.x - pad,
        y: rect.y - pad,
        width: rect.width + 2.0 * pad,
        height: rect.height + 2.0 * pad,
    }
}

/// `rectCornerPoints(rect)`: nw, ne, se, sw.
pub fn rect_corner_points(rect: Rect) -> [Point; 4] {
    [
        Point { x: rect.x, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y + rect.height },
        Point { x: rect.x, y: rect.y + rect.height },
    ]
}

pub const DEG2RAD: f64 = std::f64::consts::PI / 180.0;

/// `normalizeDeg(degrees)`: into `[0, 360)`.
pub fn normalize_deg(degrees: f64) -> f64 {
    ((degrees % 360.0) + 360.0) % 360.0
}

/// `rectCenter(rect)`.
pub fn rect_center(rect: Rect) -> Point {
    Point { x: rect.x + rect.width / 2.0, y: rect.y + rect.height / 2.0 }
}

/// `transposedAboutCenter(rect)`: sides swapped about its centre.
pub fn transposed_about_center(rect: Rect) -> Rect {
    let p = rect_center(rect);
    Rect {
        x: p.x - rect.height / 2.0,
        y: p.y - rect.width / 2.0,
        width: rect.height,
        height: rect.width,
    }
}

/// `rotatePoint(point, pivot, deg)`: clockwise in page space.
pub fn rotate_point(point: Point, pivot: Point, deg: f64) -> Point {
    apply_point(rotate_about(pivot, deg * DEG2RAD), point)
}

/// `rotatedAabb(rect, deg, pivot)`.
pub fn rotated_aabb(rect: Rect, deg: f64, pivot: Option<Point>) -> Rect {
    if deg == 0.0 || deg.is_nan() {
        return rect;
    }
    let pivot = pivot.unwrap_or_else(|| rect_center(rect));
    let corners: Vec<Point> =
        rect_corner_points(rect).iter().map(|c| rotate_point(*c, pivot, deg)).collect();
    union_rect(&corners)
}

/// `insetRect(rect, pad)`.
pub fn inset_rect(rect: Rect, pad: f64) -> Rect {
    let width = js::max(0.0, rect.width - 2.0 * pad);
    let height = js::max(0.0, rect.height - 2.0 * pad);
    Rect {
        x: rect.x + (rect.width - width) / 2.0,
        y: rect.y + (rect.height - height) / 2.0,
        width,
        height,
    }
}

/// `OPPOSITE_HANDLE[handle]`.
pub fn opposite_handle(handle: &str) -> Option<&'static str> {
    Some(match handle {
        "nw" => "se",
        "ne" => "sw",
        "se" => "nw",
        "sw" => "ne",
        "n" => "s",
        "s" => "n",
        "e" => "w",
        "w" => "e",
        _ => return None,
    })
}

/// `resizeRotatedRect(base, rot, handle, to)`.
pub fn resize_rotated_rect(base: Rect, rot: f64, handle: &str, to: Point) -> Rect {
    if rot == 0.0 || rot.is_nan() {
        return resize_rect(base, handle, to);
    }
    let c0 = rect_center(base);
    let local_to = rotate_point(to, c0, -rot);
    let next = resize_rect(base, handle, local_to);
    // An unknown handle has no opposite: `rectHandlePoint` of no edges is the middle.
    let anchor_local = rect_handle_point(base, opposite_handle(handle).unwrap_or(""));
    let anchor_world = rotate_point(anchor_local, c0, rot);
    let next_center = rect_center(next);
    let off_x = anchor_local.x - next_center.x;
    let off_y = anchor_local.y - next_center.y;
    let rad = rot * DEG2RAD;
    let cos = rad.cos();
    let sin = rad.sin();
    let r_off_x = cos * off_x - sin * off_y;
    let r_off_y = sin * off_x + cos * off_y;
    let cx = anchor_world.x - r_off_x;
    let cy = anchor_world.y - r_off_y;
    Rect {
        x: cx - next.width / 2.0,
        y: cy - next.height / 2.0,
        width: next.width,
        height: next.height,
    }
}
