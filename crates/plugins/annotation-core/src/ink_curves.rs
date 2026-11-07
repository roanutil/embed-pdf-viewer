//! `ink-curves.ts`: how Acrobat draws an ink stroke (`/InkList`) when it
//! writes the appearance, and so how the runtime draws it: cubic Béziers
//! fitted to the points as Schneider fits digitized curves, without his
//! Newton step.
//!
//! - Repeated points in a row count once.
//! - A piece of the stroke is one cubic. At the stroke's ends it leaves and
//!   arrives along the first and last segment; where two pieces meet, along
//!   the line from the point before to the point after.
//! - A piece of two points gets handles a third of its length.
//! - A longer piece gets the least-squares cubic over its points, placed by
//!   chord length. It stays one piece when every point is within the
//!   tolerance of the curve and the curve is within the tolerance of the
//!   lines between the points; otherwise it splits at its worst point.
//! - A stroke of one point is a dot: a circle of radius 0.5.

use crate::bezier::{bezier_at, flatten_bezier, path_number};
use crate::js;
use crate::types::Point;

/// `INK_CURVE_TOLERANCE`: how far the curves may stray, in points.
pub const INK_CURVE_TOLERANCE: f64 = std::f64::consts::SQRT_2;

/// `INK_DOT_RADIUS`: the radius of the dot a stroke of one point draws.
pub const INK_DOT_RADIUS: f64 = 0.5;

/// `InkCubic`: one cubic of a stroke, from the end of the one before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkCubic {
    pub control1: Point,
    pub control2: Point,
    pub end: Point,
}

/// `InkStrokeCurves`: one stroke as it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum InkStrokeCurves {
    Curves { start: Point, cubics: Vec<InkCubic> },
    Dot { center: Point },
}

#[derive(Clone, Copy)]
struct Piece {
    first: usize,
    last: usize,
    leaving: Point,
    arriving: Point,
}

fn plus(a: Point, b: Point) -> Point {
    Point { x: a.x + b.x, y: a.y + b.y }
}
fn minus(a: Point, b: Point) -> Point {
    Point { x: a.x - b.x, y: a.y - b.y }
}
fn times(a: Point, s: f64) -> Point {
    Point { x: a.x * s, y: a.y * s }
}
fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}
fn distance(a: Point, b: Point) -> f64 {
    js::hypot(a.x - b.x, a.y - b.y)
}

/// `v` at length 1; no direction (0, 0) when it has no length.
fn unit(v: Point) -> Point {
    let length = js::hypot(v.x, v.y);
    if length == 0.0 {
        Point { x: 0.0, y: 0.0 }
    } else {
        Point { x: v.x / length, y: v.y / length }
    }
}

/// Where each point of the piece sits along it, 0 to 1, by chord length.
fn chord_length_places(points: &[Point], piece: Piece) -> Vec<f64> {
    let mut places = vec![0.0];
    for i in piece.first + 1..=piece.last {
        let last = places[places.len() - 1];
        places.push(last + distance(points[i], points[i - 1]));
    }
    let length = places[places.len() - 1];
    places.into_iter().map(|place| place / length).collect()
}

/// The least-squares cubic from the piece's first point to its last, along
/// its two directions; handles shorter than a millionth of the span, or
/// backwards, are a third of the span instead.
fn least_squares_cubic(points: &[Point], piece: Piece, places: &[f64]) -> [Point; 4] {
    let start = points[piece.first];
    let end = points[piece.last];
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (i, &u) in places.iter().enumerate() {
        let rest = 1.0 - u;
        let b0 = rest * rest * rest;
        let b1 = 3.0 * u * rest * rest;
        let b2 = 3.0 * u * u * rest;
        let b3 = u * u * u;
        let a0 = times(piece.leaving, b1);
        let a1 = times(piece.arriving, b2);
        c00 += dot(a0, a0);
        c01 += dot(a0, a1);
        c11 += dot(a1, a1);
        let off = minus(
            points[piece.first + i],
            plus(plus(times(start, b0), times(start, b1)), plus(times(end, b2), times(end, b3))),
        );
        x0 += dot(a0, off);
        x1 += dot(a1, off);
    }
    let det = c00 * c11 - c01 * c01;
    let mut leaving_length = if det == 0.0 { 0.0 } else { (x0 * c11 - x1 * c01) / det };
    let mut arriving_length = if det == 0.0 { 0.0 } else { (c00 * x1 - c01 * x0) / det };
    let span = distance(end, start);
    if leaving_length < 1e-6 * span || arriving_length < 1e-6 * span {
        leaving_length = span / 3.0;
        arriving_length = span / 3.0;
    }
    [
        start,
        plus(start, times(piece.leaving, leaving_length)),
        plus(end, times(piece.arriving, arriving_length)),
        end,
    ]
}

/// The piece's point farthest from the curve at its place: the square of
/// that distance, and which (the last of equals).
fn worst_point(points: &[Point], piece: Piece, places: &[f64], cubic: &[Point; 4]) -> (f64, usize) {
    let [p0, p1, p2, p3] = *cubic;
    // Math.floor((last - first + 1) / 2) + first.
    let mut worst = (0.0, (piece.last - piece.first).div_ceil(2) + piece.first);
    for i in piece.first + 1..piece.last {
        let off = minus(bezier_at(p0, p1, p2, p3, places[i - piece.first]), points[i]);
        let squared = dot(off, off);
        if squared >= worst.0 {
            worst = (squared, i);
        }
    }
    worst
}

fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = minus(b, a);
    let length_squared = dot(ab, ab);
    let t = if length_squared == 0.0 {
        0.0
    } else {
        js::min(1.0, js::max(0.0, dot(minus(p, a), ab) / length_squared))
    };
    distance(p, plus(a, times(ab, t)))
}

/// How far the curve strays from the lines between the piece's points: the
/// farthest of 65 places along it, each from its nearest line.
fn distance_from_lines(points: &[Point], piece: Piece, cubic: &[Point; 4]) -> f64 {
    let [p0, p1, p2, p3] = *cubic;
    let mut farthest = 0.0;
    for k in 0..=64 {
        let at = bezier_at(p0, p1, p2, p3, f64::from(k) / 64.0);
        let mut nearest = f64::INFINITY;
        for i in piece.first..piece.last {
            nearest = js::min(nearest, distance_to_segment(at, points[i], points[i + 1]));
        }
        farthest = js::max(farthest, nearest);
    }
    farthest
}

/// `fitInkStroke(stroke, tolerance)`: the cubics that fit the stroke's
/// points, or a dot for one point; `None` for a stroke with no points.
pub fn fit_ink_stroke(stroke: &[Point], tolerance: f64) -> Option<InkStrokeCurves> {
    let mut points: Vec<Point> = Vec::new();
    for &point in stroke {
        match points.last() {
            Some(previous) if previous.x == point.x && previous.y == point.y => {}
            _ => points.push(point),
        }
    }
    match points.len() {
        0 => return None,
        1 => return Some(InkStrokeCurves::Dot { center: points[0] }),
        _ => {}
    }
    // The pieces still to fit, the next on top: a split piece's first half
    // is fitted (and split) before its second, so the cubics come out in order.
    let mut cubics = Vec::new();
    let last = points.len() - 1;
    let mut pending = vec![Piece {
        first: 0,
        last,
        leaving: unit(minus(points[1], points[0])),
        arriving: unit(minus(points[last - 1], points[last])),
    }];
    while let Some(piece) = pending.pop() {
        let start = points[piece.first];
        let end = points[piece.last];
        if piece.last - piece.first == 1 {
            let handle = distance(end, start) / 3.0;
            cubics.push(InkCubic {
                control1: plus(start, times(piece.leaving, handle)),
                control2: plus(end, times(piece.arriving, handle)),
                end,
            });
            continue;
        }
        let places = chord_length_places(&points, piece);
        let cubic = least_squares_cubic(&points, piece, &places);
        let (squared, index) = worst_point(&points, piece, &places, &cubic);
        if squared < tolerance * tolerance
            && distance_from_lines(&points, piece, &cubic) < tolerance
        {
            cubics.push(InkCubic { control1: cubic[1], control2: cubic[2], end });
            continue;
        }
        // The halves meet at the worst point, along the line from the point
        // before it to the point after.
        let split = index;
        let back = unit(minus(points[split - 1], points[split + 1]));
        pending.push(Piece {
            first: split,
            last: piece.last,
            leaving: times(back, -1.0),
            arriving: piece.arriving,
        });
        pending.push(Piece {
            first: piece.first,
            last: split,
            leaving: piece.leaving,
            arriving: back,
        });
    }
    Some(InkStrokeCurves::Curves { start: points[0], cubics })
}

/// `inkCurvesPath(curves)`: the stroke's curves as SVG path data.
pub fn ink_curves_path(start: Point, cubics: &[InkCubic]) -> String {
    let at = |point: Point| format!("{} {}", path_number(point.x), path_number(point.y));
    let mut parts = vec![format!("M {}", at(start))];
    for cubic in cubics {
        parts.push(format!("C {} {} {}", at(cubic.control1), at(cubic.control2), at(cubic.end)));
    }
    parts.join(" ")
}

/// How far the points along the curves may stray from them.
const POLYLINE_TOLERANCE: f64 = 0.05;

/// `inkCurvesPolyline(curves)`: points along the stroke's curves, close
/// enough to stand in for them in a hit test.
pub fn ink_curves_polyline(start: Point, cubics: &[InkCubic]) -> Vec<Point> {
    let mut points = vec![start];
    let mut from = start;
    for cubic in cubics {
        points.extend(flatten_bezier(
            from,
            cubic.control1,
            cubic.control2,
            cubic.end,
            POLYLINE_TOLERANCE,
        ));
        from = cubic.end;
    }
    points
}
