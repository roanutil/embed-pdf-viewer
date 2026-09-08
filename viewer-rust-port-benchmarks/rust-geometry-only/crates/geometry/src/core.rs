//! The math, with no wasm-bindgen anywhere in sight.
//!
//! Keeping the pure core free of `#[wasm_bindgen]` is what lets `cargo test`
//! run it natively against the frozen vectors, and it is what a Swift or
//! Kotlin binding would sit on top of instead of the JS glue.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec2 {
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

/// Four positional corners, clockwise from the unrotated top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad(pub [Vec2; 4]);

pub fn normalize_deg(deg: f64) -> f64 {
    let m = deg % 360.0;
    if m < 0.0 {
        m + 360.0
    } else {
        m
    }
}

pub fn rotate_point(p: Vec2, about: Vec2, deg: f64) -> Vec2 {
    if deg == 0.0 {
        return p;
    }
    let rad = deg.to_radians();
    let (sin, cos) = rad.sin_cos();
    let dx = p.x - about.x;
    let dy = p.y - about.y;
    Vec2 {
        x: about.x + dx * cos - dy * sin,
        y: about.y + dx * sin + dy * cos,
    }
}

pub fn scale_point(p: Vec2, about: Vec2, s: f64) -> Vec2 {
    if s == 1.0 {
        return p;
    }
    Vec2 {
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
        Vec2 { x, y },
        Vec2 { x: x + w, y },
        Vec2 { x: x + w, y: y + h },
        Vec2 { x, y: y + h },
    ];
    if normalize_deg(rot) == 0.0 {
        return Quad(corners);
    }
    let c = Vec2 {
        x: x + w / 2.0,
        y: y + h / 2.0,
    };
    Quad(corners.map(|p| rotate_point(p, c, rot)))
}

pub fn quad_scale_about(q: Quad, about: Vec2, s: f64) -> Quad {
    Quad(q.0.map(|p| scale_point(p, about, s)))
}

pub fn quad_rotate_about(q: Quad, about: Vec2, deg: f64) -> Quad {
    Quad(q.0.map(|p| rotate_point(p, about, deg)))
}

pub fn quad_translate(q: Quad, by: Vec2) -> Quad {
    Quad(q.0.map(|p| Vec2 {
        x: p.x + by.x,
        y: p.y + by.y,
    }))
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

/// Winding test, matching the TypeScript exactly: a zero cross product means
/// the point is ON that edge, and the edge is skipped rather than deciding.
pub fn point_in_quad(p: Vec2, q: Quad) -> bool {
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

/// The anchor projection, ported so `cargo test` can check it against the same
/// frozen vectors the TypeScript suite reads. The wasm surface does not export
/// it: in this POC only geometry crossed the boundary, and the brain that calls
/// this still lives in TypeScript.
pub fn anchor_factors(anchored: bool, zoom: f64, rotation: f64) -> Option<(f64, f64)> {
    if !anchored {
        return None;
    }
    let z = if zoom == 0.0 { 1.0 } else { zoom };
    let s = z.max(1.0);
    let r = normalize_deg(rotation);
    if s != 1.0 || r != 0.0 {
        Some((s, r))
    } else {
        None
    }
}

pub fn project_quad(q: Quad, anchored: bool, zoom: f64, rotation: f64) -> Quad {
    let Some((s, r)) = anchor_factors(anchored, zoom, rotation) else {
        return q;
    };
    let b = quad_bounds(q);
    let anchor = Vec2 { x: b.x, y: b.y };
    let mut out = q;
    if s != 1.0 {
        out = quad_scale_about(out, anchor, 1.0 / s);
    }
    if r != 0.0 {
        out = quad_rotate_about(out, anchor, normalize_deg(-r));
    }
    out
}
