//! `ink.ts`: recognising a straight freehand stroke and snapping it to an axis.

use crate::js;
use crate::types::{InkStraightenOptions, Point};

/// `straightenInkStroke(points, options)`.
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
    let max_deviation = points.iter().fold(0.0, |max, p| {
        js::max(max, (dx * (first.y - p.y) - (first.x - p.x) * dy).abs() / length)
    });
    if max_deviation / length >= options.deviation_threshold {
        return points.to_vec();
    }
    let angle = dy.abs().atan2(dx.abs()) * (180.0 / std::f64::consts::PI);
    if angle <= options.axis_snap_degrees {
        let y = points.iter().fold(0.0, |sum, p| sum + p.y) / points.len() as f64;
        return vec![Point { x: first.x, y }, Point { x: last.x, y }];
    }
    if angle >= 90.0 - options.axis_snap_degrees {
        let x = points.iter().fold(0.0, |sum, p| sum + p.x) / points.len() as f64;
        return vec![Point { x, y: first.y }, Point { x, y: last.y }];
    }
    points.to_vec()
}
