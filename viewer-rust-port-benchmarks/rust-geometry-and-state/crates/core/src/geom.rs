//! The geometry layer, unchanged from rust-geometry-only's crate. It never crosses the
//! boundary here: the whole core is Rust now, so this is just a module.

use serde::{Deserialize, Serialize};

/// A position in content space (y-down). The same struct also carries the
/// `by` offset of `quad_translate`; it has no separate vector type.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad(pub [Point; 4]);

pub fn normalize_deg(deg: f64) -> f64 {
    // The same expression as typescript-baseline's `normalizeDeg`. A single
    // `% 360` plus a conditional add sends a tiny negative (say -1e-20) to
    // 360.0, which is outside the [0, 360) range every caller assumes.
    ((deg % 360.0) + 360.0) % 360.0
}

pub fn rotate_point(p: Point, about: Point, deg: f64) -> Point {
    if deg == 0.0 {
        return p;
    }
    let (sin, cos) = deg.to_radians().sin_cos();
    let dx = p.x - about.x;
    let dy = p.y - about.y;
    Point {
        x: about.x + dx * cos - dy * sin,
        y: about.y + dx * sin + dy * cos,
    }
}

pub fn scale_point(p: Point, about: Point, s: f64) -> Point {
    if s == 1.0 {
        return p;
    }
    Point {
        x: about.x + (p.x - about.x) * s,
        y: about.y + (p.y - about.y) * s,
    }
}

pub fn rect_quad(rect: Rect, rot: f64) -> Quad {
    let Rect {
        x,
        y,
        width: w,
        height: h,
    } = rect;
    let corners = [
        Point { x, y },
        Point { x: x + w, y },
        Point { x: x + w, y: y + h },
        Point { x, y: y + h },
    ];
    if normalize_deg(rot) == 0.0 {
        return Quad(corners);
    }
    let c = Point {
        x: x + w / 2.0,
        y: y + h / 2.0,
    };
    Quad(corners.map(|p| rotate_point(p, c, rot)))
}

pub fn quad_scale_about(q: Quad, about: Point, s: f64) -> Quad {
    Quad(q.0.map(|p| scale_point(p, about, s)))
}

pub fn quad_rotate_about(q: Quad, about: Point, deg: f64) -> Quad {
    Quad(q.0.map(|p| rotate_point(p, about, deg)))
}

pub fn quad_bounds(q: Quad) -> Rect {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in q.0 {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    }
}

pub fn point_in_quad(p: Point, q: Quad) -> bool {
    let mut sign = 0i32;
    for i in 0..4 {
        let a = q.0[i];
        let b = q.0[(i + 1) % 4];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross == 0.0 {
            continue;
        }
        let s = if cross > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = s;
        } else if sign != s {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::normalize_deg;

    /// Mirrors typescript-baseline's `((deg % 360) + 360) % 360`, which folds a
    /// tiny negative back to 0 rather than to 360.
    #[test]
    fn normalize_deg_folds_into_the_half_open_range() {
        assert_eq!(normalize_deg(-90.0), 270.0);
        assert_eq!(normalize_deg(450.0), 90.0);
        assert_eq!(normalize_deg(0.0), 0.0);
        assert_eq!(normalize_deg(-1e-20), 0.0);
        assert_eq!(normalize_deg(-0.0), 0.0);
        assert!(normalize_deg(-1e-20) < 360.0);
    }
}
