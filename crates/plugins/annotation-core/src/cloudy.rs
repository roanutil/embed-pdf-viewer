//! `cloudy.ts`: cloudy-border (scalloped outline) path generation, after
//! Apache PDFBox's `CloudyBorder` (the curl placement PDFium bakes into the
//! `/BE` appearance). Pure: a content-space box or vertex ring in, SVG path
//! data in absolute content coordinates out. The math runs in PDFBox's y-up
//! frame; [`PathBuilder`] flips back on the way out.

use std::f64::consts::PI;

use crate::js;
use crate::types::{Point, Rect};

const ANGLE_180: f64 = PI;
const ANGLE_90: f64 = PI / 2.0;
const ANGLE_34: f64 = (34.0 * PI) / 180.0;
const ANGLE_30: f64 = (30.0 * PI) / 180.0;
const ANGLE_12: f64 = (12.0 * PI) / 180.0;

/// The most curls one cloudy border may ask for. Curls sit at least
/// `2 * cos(34deg) * 0.5` (about 0.83) units apart, so this is a perimeter
/// of roughly 830,000 units: about fourteen times round the largest page a
/// PDF allows (14,400 units a side). Geometry from a hostile /Rect,
/// /Vertices or /BE past it would make the loops below allocate without
/// bound (or, past 2^53, never advance), so it draws no border instead. The
/// TypeScript has no such limit; every real document stays far below it,
/// and there the output is identical.
pub(crate) const MAX_CLOUDY_CURLS: f64 = 1_000_000.0;

/// The narrowest curl spacing any path here can reach.
const MIN_CURL_ADVANCE: f64 = 0.8;

/// True when tracing `length` units of border could exceed
/// [`MAX_CLOUDY_CURLS`]. NaN is let through: the loops never run on it.
fn over_budget(length: f64) -> bool {
    length / MIN_CURL_ADVANCE > MAX_CLOUDY_CURLS
}

/// `Number(n.toFixed(4)).toString()`.
fn r(n: f64) -> String {
    js::number_to_string(js::fixed(n, 4))
}

/// Accumulates SVG path commands: y-up input, y-down output offset into the
/// box origin.
struct PathBuilder {
    parts: Vec<String>,
    started: bool,
    ox: f64,
    oy: f64,
}

impl PathBuilder {
    fn new(ox: f64, oy: f64) -> Self {
        PathBuilder { parts: Vec::new(), started: false, ox, oy }
    }
    fn move_to(&mut self, x: f64, y: f64) {
        self.parts.push(format!("M {} {}", r(x + self.ox), r(-y + self.oy)));
        self.started = true;
    }
    fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        self.parts.push(format!(
            "C {} {}, {} {}, {} {}",
            r(x1 + self.ox),
            r(-y1 + self.oy),
            r(x2 + self.ox),
            r(-y2 + self.oy),
            r(x3 + self.ox),
            r(-y3 + self.oy)
        ));
    }
    fn close(&mut self) {
        if self.started {
            self.parts.push("Z".into());
        }
    }
    fn build(&self) -> String {
        self.parts.join(" ")
    }
}

fn distance(a: Point, b: Point) -> f64 {
    js::hypot(b.x - a.x, b.y - a.y)
}

fn cosine(dx: f64, hyp: f64) -> f64 {
    if hyp == 0.0 { 0.0 } else { dx / hyp }
}

fn sine(dy: f64, hyp: f64) -> f64 {
    if hyp == 0.0 { 0.0 } else { dy / hyp }
}

/// Signed area (shoelace): positive is counter-clockwise.
fn polygon_direction(pts: &[Point]) -> f64 {
    let mut a = 0.0;
    for i in 0..pts.len() {
        let j = (i + 1) % pts.len();
        a += pts[i].x * pts[j].y - pts[i].y * pts[j].x;
    }
    a
}

fn ensure_positive_winding(pts: &mut [Point]) {
    if polygon_direction(pts) < 0.0 {
        pts.reverse();
    }
}

fn remove_zero_length_segments(polygon: &[Point]) -> Vec<Point> {
    if polygon.len() <= 2 {
        return polygon.to_vec();
    }
    let tol = 0.5;
    let mut out = vec![polygon[0]];
    for &cur in &polygon[1..] {
        let prev = out[out.len() - 1];
        if (cur.x - prev.x).abs() >= tol || (cur.y - prev.y).abs() >= tol {
            out.push(cur);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn arc_segment(
    start: f64,
    end: f64,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    out: &mut PathBuilder,
    add_move_to: bool,
) {
    let cos_a = start.cos();
    let sin_a = start.sin();
    let cos_b = end.cos();
    let sin_b = end.sin();
    let denom = ((end - start) / 2.0).sin();
    if denom == 0.0 {
        if add_move_to {
            out.move_to(cx + rx * cos_a, cy + ry * sin_a);
        }
        return;
    }
    let bcp = ((4.0 / 3.0) * (1.0 - ((end - start) / 2.0).cos())) / denom;
    if add_move_to {
        out.move_to(cx + rx * cos_a, cy + ry * sin_a);
    }
    out.curve_to(
        cx + rx * (cos_a - bcp * sin_a),
        cy + ry * (sin_a + bcp * cos_a),
        cx + rx * (cos_b + bcp * sin_b),
        cy + ry * (sin_b - bcp * cos_b),
        cx + rx * cos_b,
        cy + ry * sin_b,
    );
}

fn arc_segment_to_array(start: f64, end: f64, rx: f64, ry: f64) -> Vec<Point> {
    let cos_a = start.cos();
    let sin_a = start.sin();
    let cos_b = end.cos();
    let sin_b = end.sin();
    let denom = ((end - start) / 2.0).sin();
    if denom == 0.0 {
        return Vec::new();
    }
    let bcp = ((4.0 / 3.0) * (1.0 - ((end - start) / 2.0).cos())) / denom;
    vec![
        Point { x: rx * (cos_a - bcp * sin_a), y: ry * (sin_a + bcp * cos_a) },
        Point { x: rx * (cos_b + bcp * sin_b), y: ry * (sin_b - bcp * cos_b) },
        Point { x: rx * cos_b, y: ry * sin_b },
    ]
}

#[allow(clippy::too_many_arguments)]
fn get_arc(
    start: f64,
    end: f64,
    rx: f64,
    ry: f64,
    cx: f64,
    cy: f64,
    out: &mut PathBuilder,
    add_move_to: bool,
) {
    let mut angle_todo = end - start;
    while angle_todo < 0.0 {
        angle_todo += 2.0 * PI;
    }
    let sweep = angle_todo;
    let mut angle_done = 0.0;
    if add_move_to {
        out.move_to(cx + rx * start.cos(), cy + ry * start.sin());
    }
    while angle_todo > ANGLE_90 {
        arc_segment(start + angle_done, start + angle_done + ANGLE_90, cx, cy, rx, ry, out, false);
        angle_done += ANGLE_90;
        angle_todo -= ANGLE_90;
    }
    if angle_todo > 0.0 {
        arc_segment(start + angle_done, start + sweep, cx, cy, rx, ry, out, false);
    }
}

#[allow(clippy::too_many_arguments)]
fn add_corner_curl(
    angle_prev: f64,
    angle_cur: f64,
    radius: f64,
    cx: f64,
    cy: f64,
    alpha: f64,
    alpha_prev: f64,
    out: &mut PathBuilder,
    add_move_to: bool,
) {
    let a = angle_prev + ANGLE_180 + alpha_prev;
    let b = a - (22.0 * PI) / 180.0;
    arc_segment(a, b, cx, cy, radius, radius, out, add_move_to);
    get_arc(b, angle_cur - alpha, radius, radius, cx, cy, out, false);
}

fn add_first_intermediate_curl(
    angle_cur: f64,
    rad: f64,
    alpha: f64,
    cx: f64,
    cy: f64,
    out: &mut PathBuilder,
) {
    let a = angle_cur + ANGLE_180;
    arc_segment(a + alpha, a + alpha - ANGLE_30, cx, cy, rad, rad, out, false);
    arc_segment(a + alpha - ANGLE_30, a + ANGLE_90, cx, cy, rad, rad, out, false);
    arc_segment(a + ANGLE_90, a + ANGLE_180 - ANGLE_34, cx, cy, rad, rad, out, false);
}

fn intermediate_curl_template(angle_cur: f64, rad: f64) -> Vec<Point> {
    let a = angle_cur + ANGLE_180;
    let mut out = arc_segment_to_array(a + ANGLE_34, a + ANGLE_12, rad, rad);
    out.extend(arc_segment_to_array(a + ANGLE_12, a + ANGLE_90, rad, rad));
    out.extend(arc_segment_to_array(a + ANGLE_90, a + ANGLE_180 - ANGLE_34, rad, rad));
    out
}

fn output_curl_template(template: &[Point], x: f64, y: f64, out: &mut PathBuilder) {
    let mut i = 0;
    while i + 2 < template.len() {
        out.curve_to(
            template[i].x + x,
            template[i].y + y,
            template[i + 1].x + x,
            template[i + 1].y + y,
            template[i + 2].x + x,
            template[i + 2].y + y,
        );
        i += 3;
    }
}

fn ellipse_cloud_radius(intensity: f64, line_width: f64) -> f64 {
    4.75 * intensity + 0.5 * line_width
}

fn polygon_cloud_radius(intensity: f64, line_width: f64) -> f64 {
    4.0 * intensity + 0.5 * line_width
}

/// `(n, adjustedRadius)`: the intermediate curl count along an edge.
fn compute_params_polygon(ideal_radius: f64, k: f64, length: f64) -> (f64, f64) {
    if length == 0.0 {
        return (-1.0, ideal_radius);
    }
    let remaining = length - 2.0 * k * ideal_radius;
    if remaining <= 0.0 {
        return (0.0, ideal_radius);
    }
    let n = js::max(1.0, (remaining / (2.0 * k * ideal_radius)).ceil());
    (n, remaining / (n * 2.0 * k))
}

fn cloudy_polygon_impl(
    vertices: &[Point],
    is_ellipse: bool,
    intensity: f64,
    line_width: f64,
    out: &mut PathBuilder,
) {
    let mut polygon = remove_zero_length_segments(vertices);
    ensure_positive_winding(&mut polygon);
    let n = polygon.len();
    if n < 2 {
        return;
    }

    if intensity <= 0.0 {
        out.move_to(polygon[0].x, polygon[0].y);
        for p in &polygon[1..] {
            out.curve_to(p.x, p.y, p.x, p.y, p.x, p.y);
        }
        return;
    }

    let mut ideal_radius = if is_ellipse {
        ellipse_cloud_radius(intensity, line_width)
    } else {
        polygon_cloud_radius(intensity, line_width)
    };
    if ideal_radius < 0.5 {
        ideal_radius = 0.5;
    }
    let k = ANGLE_34.cos();

    let edge_alphas: Vec<f64> = (0..n - 1)
        .map(|j| {
            let len = distance(polygon[j], polygon[j + 1]);
            if len <= 0.0 || len >= 2.0 * k * ideal_radius {
                ANGLE_34
            } else {
                js::min(1.0, len / (2.0 * ideal_radius)).acos()
            }
        })
        .collect();

    let mut angle_prev = 0.0;
    let mut started = false;
    for j in 0..n - 1 {
        let pt = polygon[j];
        let pt_next = polygon[j + 1];
        let len = distance(pt, pt_next);
        if len == 0.0 {
            continue;
        }

        let (params_n, adjusted_radius) = compute_params_polygon(ideal_radius, k, len);
        if params_n < 0.0 {
            if !started {
                out.move_to(pt.x, pt.y);
                started = true;
            }
            continue;
        }

        let edge_radius = js::max(0.5, adjusted_radius);
        let interm_advance = 2.0 * k * edge_radius;
        let first_advance = k * ideal_radius + k * edge_radius;
        let angle_cur = (pt_next.y - pt.y).atan2(pt_next.x - pt.x);
        if j == 0 {
            let pt_prev = polygon[n - 2];
            angle_prev = (pt.y - pt_prev.y).atan2(pt.x - pt_prev.x);
        }
        let cos = cosine(pt_next.x - pt.x, len);
        let sin = sine(pt_next.y - pt.y, len);
        let alpha = edge_alphas[j];
        let alpha_prev =
            edge_alphas.get(if j == 0 { n - 2 } else { j - 1 }).copied().unwrap_or(ANGLE_34);

        add_corner_curl(
            angle_prev,
            angle_cur,
            ideal_radius,
            pt.x,
            pt.y,
            alpha,
            alpha_prev,
            out,
            !started,
        );
        started = true;

        // With no intermediate curls the TypeScript advances the pen to the
        // edge's end and never reads it again: nothing to draw.
        if params_n != 0.0 {
            let mut x = pt.x + first_advance * cos;
            let mut y = pt.y + first_advance * sin;
            let mut num_interm = params_n;
            if params_n >= 1.0 {
                add_first_intermediate_curl(angle_cur, edge_radius, ANGLE_34, x, y, out);
                x += interm_advance * cos;
                y += interm_advance * sin;
                num_interm = params_n - 1.0;
            }
            let template = intermediate_curl_template(angle_cur, edge_radius);
            let mut i = 0.0;
            while i < num_interm {
                output_curl_template(&template, x, y, out);
                x += interm_advance * cos;
                y += interm_advance * sin;
                i += 1.0;
            }
        }
        angle_prev = angle_cur;
    }
}

fn flatten_ellipse(left: f64, bottom: f64, right: f64, top: f64) -> Vec<Point> {
    let cx = (left + right) / 2.0;
    let cy = (bottom + top) / 2.0;
    let rx = (right - left) / 2.0;
    let ry = (top - bottom) / 2.0;
    if rx <= 0.0 || ry <= 0.0 {
        return Vec::new();
    }
    let segments = js::max(32.0, (js::max(rx, ry) * 2.0).ceil());
    let mut pts = Vec::new();
    let mut i = 0.0;
    while i <= segments {
        let angle = (2.0 * PI * i) / segments;
        pts.push(Point { x: cx + rx * angle.cos(), y: cy + ry * angle.sin() });
        i += 1.0;
    }
    pts
}

fn compute_params_ellipse(pt: Point, pt_next: Point, rad: f64, curl_adv: f64) -> f64 {
    let len = distance(pt, pt_next);
    if len == 0.0 {
        return ANGLE_34;
    }
    let arg = (curl_adv / 2.0 + (len - curl_adv) / 2.0) / rad;
    // Written as two comparisons, not a range test: NaN falls through to acos.
    if arg < -1.0 || arg > 1.0 { 0.0 } else { arg.acos() }
}

#[allow(clippy::too_many_arguments)]
fn cloudy_ellipse_impl(
    left: f64,
    bottom: f64,
    right: f64,
    top: f64,
    intensity: f64,
    line_width: f64,
    out: &mut PathBuilder,
) {
    let plain_ellipse = |out: &mut PathBuilder| {
        let rx = (right - left).abs() / 2.0;
        let ry = (top - bottom).abs() / 2.0;
        get_arc(0.0, 2.0 * PI, rx, ry, (left + right) / 2.0, (bottom + top) / 2.0, out, true);
    };

    if intensity <= 0.0 {
        return plain_ellipse(out);
    }

    let width = right - left;
    let height = top - bottom;
    let mut cloud_radius = ellipse_cloud_radius(intensity, line_width);

    if width < 0.5 * cloud_radius && height < 0.5 * cloud_radius {
        return plain_ellipse(out);
    }

    if (width < 5.0 && height > 20.0) || (width > 20.0 && height < 5.0) {
        let ring = [
            Point { x: left, y: bottom },
            Point { x: right, y: bottom },
            Point { x: right, y: top },
            Point { x: left, y: top },
            Point { x: left, y: bottom },
        ];
        return cloudy_polygon_impl(&ring, true, intensity, line_width, out);
    }

    let radius_adj = ANGLE_12.sin() * cloud_radius - 1.5;
    let mut adj_left = left;
    let mut adj_right = right;
    let mut adj_bottom = bottom;
    let mut adj_top = top;
    if width > 2.0 * radius_adj {
        adj_left += radius_adj;
        adj_right -= radius_adj;
    } else {
        let mid = (left + right) / 2.0;
        adj_left = mid - 0.1;
        adj_right = mid + 0.1;
    }
    if height > 2.0 * radius_adj {
        adj_bottom += radius_adj;
        adj_top -= radius_adj;
    } else {
        let mid = (top + bottom) / 2.0;
        adj_top = mid + 0.1;
        adj_bottom = mid - 0.1;
    }

    let flat = flatten_ellipse(adj_left, adj_bottom, adj_right, adj_top);
    if flat.len() < 2 {
        return;
    }
    let mut tot_len = 0.0;
    for i in 1..flat.len() {
        tot_len += distance(flat[i - 1], flat[i]);
    }

    let k = ANGLE_34.cos();
    let n = (tot_len / (2.0 * k * cloud_radius)).ceil();
    if n < 2.0 {
        return plain_ellipse(out);
    }

    let mut curl_advance = tot_len / n;
    cloud_radius = curl_advance / (2.0 * k);
    if cloud_radius < 0.5 {
        cloud_radius = 0.5;
        curl_advance = 2.0 * k * cloud_radius;
    } else if cloud_radius < 3.0 {
        return plain_ellipse(out);
    }

    let mut centers: Vec<Point> = Vec::new();
    let mut remain = 0.0;
    let toler = line_width * 0.1;
    for i in 0..flat.len() - 1 {
        let p1 = flat[i];
        let p2 = flat[i + 1];
        let seg_len = distance(p1, p2);
        if seg_len == 0.0 {
            continue;
        }
        let mut todo = seg_len + remain;
        if todo >= curl_advance - toler || i == flat.len() - 2 {
            let cos = cosine(p2.x - p1.x, seg_len);
            let sin = sine(p2.y - p1.y, seg_len);
            let mut d = curl_advance - remain;
            while todo >= curl_advance - toler {
                centers.push(Point { x: p1.x + d * cos, y: p1.y + d * sin });
                todo -= curl_advance;
                d += curl_advance;
            }
            remain = js::max(0.0, todo);
        } else {
            remain += seg_len;
        }
    }

    let m = centers.len();
    let mut angle_prev = 0.0;
    let mut alpha_prev = 0.0;
    for i in 0..m {
        let pt = centers[i];
        let pt_next = centers[(i + 1) % m];
        if i == 0 {
            let pt_prev = centers[m - 1];
            angle_prev = (pt.y - pt_prev.y).atan2(pt.x - pt_prev.x);
            alpha_prev = compute_params_ellipse(pt_prev, pt, cloud_radius, curl_advance);
        }
        let angle_cur = (pt_next.y - pt.y).atan2(pt_next.x - pt.x);
        let alpha = compute_params_ellipse(pt, pt_next, cloud_radius, curl_advance);
        add_corner_curl(
            angle_prev,
            angle_cur,
            cloud_radius,
            pt.x,
            pt.y,
            alpha,
            alpha_prev,
            out,
            i == 0,
        );
        angle_prev = angle_cur;
        alpha_prev = alpha;
    }
}

/// The per-side inset from a shape's outer box to the cloud's inner
/// boundary: the scallop radius plus half the stroke. This is the `/RD`.
pub fn cloudy_border_extent(intensity: f64, stroke_width: f64, ellipse: bool) -> f64 {
    let cr = if ellipse {
        ellipse_cloud_radius(intensity, stroke_width)
    } else {
        polygon_cloud_radius(intensity, stroke_width)
    };
    cr + stroke_width / 2.0
}

/// SVG path data for a cloudy polygon border: curls centred on the vertex
/// path, reaching outward.
pub fn cloudy_poly_path(points: &[Point], intensity: f64, stroke_width: f64) -> String {
    let mut out = PathBuilder::new(0.0, 0.0);
    let mut ring: Vec<Point> = points.iter().map(|p| Point { x: p.x, y: -p.y }).collect();
    if let (Some(&first), Some(&last)) = (ring.first(), ring.last())
        && (first.x != last.x || first.y != last.y)
    {
        ring.push(first);
    }
    let length: f64 = ring.windows(2).map(|w| distance(w[0], w[1])).sum();
    if over_budget(length) {
        return String::new();
    }
    cloudy_polygon_impl(&ring, false, intensity, stroke_width, &mut out);
    out.close();
    out.build()
}

/// SVG path data for a cloudy square or circle: scallops generated on the
/// box inset by [`cloudy_border_extent`], bulging back out to its edge.
pub fn cloudy_path(bx: Rect, ellipse: bool, intensity: f64, stroke_width: f64) -> String {
    let inset = cloudy_border_extent(intensity, stroke_width, ellipse);
    let mut out = PathBuilder::new(bx.x, bx.y);
    let left = inset;
    let top = inset;
    let right = bx.width - inset;
    let bottom = bx.height - inset;
    // Twice the box's semi-perimeter bounds both the ring and the flattened
    // ellipse (which takes about `2 * max(rx, ry)` points).
    if over_budget(2.0 * ((right - left).abs() + (bottom - top).abs())) {
        return String::new();
    }
    if ellipse {
        cloudy_ellipse_impl(left, -bottom, right, -top, intensity, stroke_width, &mut out);
    } else {
        let ring = [
            Point { x: left, y: -top },
            Point { x: right, y: -top },
            Point { x: right, y: -bottom },
            Point { x: left, y: -bottom },
            Point { x: left, y: -top },
        ];
        cloudy_polygon_impl(&ring, false, intensity, stroke_width, &mut out);
    }
    out.close();
    out.build()
}
