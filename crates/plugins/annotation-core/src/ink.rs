//! `ink.ts`: recognise a nearly straight freehand stroke and axis-snap it.

use crate::js;
use crate::types::{InkStraightenOptions, Point};

/// A sufficiently straight stroke close to horizontal or vertical becomes a
/// two-point line on the mean coordinate; anything else is returned as is.
pub fn straighten_ink_stroke(points: &[Point], options: &InkStraightenOptions) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let first = points[0];
    let last = points[points.len() - 1];
    let dx = last.x - first.x;
    let dy = last.y - first.y;
    let length = js::hypot(dx, dy);
    if length < 3.0 {
        return points.to_vec();
    }
    let max_deviation = points.iter().fold(0.0, |max, point| {
        let distance = (dx * (first.y - point.y) - (first.x - point.x) * dy).abs() / length;
        js::max(max, distance)
    });
    if max_deviation / length >= options.deviation_threshold {
        return points.to_vec();
    }
    let angle = dy.abs().atan2(dx.abs()) * (180.0 / std::f64::consts::PI);
    let n = points.len() as f64;
    if angle <= options.axis_snap_degrees {
        let y = points.iter().fold(0.0, |sum, p| sum + p.y) / n;
        return vec![Point { x: first.x, y }, Point { x: last.x, y }];
    }
    if angle >= 90.0 - options.axis_snap_degrees {
        let x = points.iter().fold(0.0, |sum, p| sum + p.x) / n;
        return vec![Point { x, y: first.y }, Point { x, y: last.y }];
    }
    points.to_vec()
}
