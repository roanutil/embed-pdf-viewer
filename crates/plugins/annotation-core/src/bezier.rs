//! `bezier.ts`: cubic Béziers, as the cloudy borders and ink draw them: the
//! point at a parameter, where a curve turns, points along it, and a number
//! for SVG path data.

use crate::js;
use crate::types::Point;

/// `bezierAt(p0, p1, p2, p3, t)`: the point at `t` on the cubic `p0..p3`.
pub fn bezier_at(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let rest = 1.0 - t;
    let w = [rest * rest * rest, 3.0 * rest * rest * t, 3.0 * rest * t * t, t * t * t];
    Point {
        x: w[0] * p0.x + w[1] * p1.x + w[2] * p2.x + w[3] * p3.x,
        y: w[0] * p0.y + w[1] * p1.y + w[2] * p2.y + w[3] * p3.y,
    }
}

/// `bezierTurns(p0, p1, p2, p3)`: the parameters in (0, 1) where a cubic
/// coordinate's slope is zero.
pub fn bezier_turns(p0: f64, p1: f64, p2: f64, p3: f64) -> Vec<f64> {
    let a = -p0 + 3.0 * p1 - 3.0 * p2 + p3;
    let b = 2.0 * (p0 - 2.0 * p1 + p2);
    let c = p1 - p0;
    let roots = if a.abs() < 1e-12 {
        if b.abs() < 1e-12 { Vec::new() } else { vec![-c / b] }
    } else if b * b - 4.0 * a * c < 0.0 {
        Vec::new()
    } else {
        vec![
            (-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a),
            (-b - (b * b - 4.0 * a * c).sqrt()) / (2.0 * a),
        ]
    };
    roots.into_iter().filter(|t| *t > 0.0 && *t < 1.0).collect()
}

/// `flattenBezier(p0, p1, p2, p3, tolerance)`: points along the cubic after
/// `p0`, evenly spaced in `t`, whose chords stay within `tolerance` of it.
pub fn flatten_bezier(p0: Point, p1: Point, p2: Point, p3: Point, tolerance: f64) -> Vec<Point> {
    let bend = js::max(
        js::hypot(p0.x - 2.0 * p1.x + p2.x, p0.y - 2.0 * p1.y + p2.y),
        js::hypot(p1.x - 2.0 * p2.x + p3.x, p1.y - 2.0 * p2.y + p3.y),
    );
    let steps = js::max(1.0, ((0.75 * bend) / tolerance).sqrt().ceil());
    let mut points = Vec::new();
    let mut i = 1.0;
    while i <= steps {
        points.push(bezier_at(p0, p1, p2, p3, i / steps));
        i += 1.0;
    }
    points
}

/// `pathNumber(value)`: a number in SVG path data, at most four decimals.
pub fn path_number(value: f64) -> String {
    js::number_to_string(js::fixed(value, 4))
}
