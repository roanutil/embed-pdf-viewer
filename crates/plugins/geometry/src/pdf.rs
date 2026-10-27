//! The file's coordinates: `engine-core`'s `geometry/primitives.ts` and the
//! two quad helpers of `geometry/convert.ts` the viewer side calls
//! (`pdfQuadBounds`, `normalizePdfQuad`).
//!
//! `epdf-engine` has its own copies for the native engine, but it links
//! PDFium and cannot build for wasm32, where the plugins that read
//! `/QuadPoints` and redaction regions run. These are pure and follow the
//! TypeScript's `Math.*` through [`crate::js`].

use serde::{Deserialize, Serialize};

use crate::{Point, Quad, js};

/// A point in PDF user space (y-up). The same two numbers as [`Point`].
pub type PdfPoint = Point;

/// A quad in PDF user space, corners named in the text's own upright frame.
/// The same four named corners as [`Quad`]; only the space differs.
pub type PdfQuad = Quad;

/// A `/QuadPoints` entry's four points, in the order the file gives them.
pub type PdfQuadPoints = [PdfPoint; 4];

/// A rectangle in the file's coordinates: PDF user space as edges (y-up, so
/// `top > bottom` and `right > left` once normalized), page-box origin
/// preserved.
///
/// The members are declared in the order every TypeScript producer writes
/// them (`normalizePdfRect`, `pdfRectOf`, `pdfQuadBounds`): left, right,
/// bottom, top. The interface itself declares left, bottom, right, top, but
/// a value's key order is its builder's, and input order does not matter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
    pub top: f64,
}

/// Enclosing axis-aligned box of a quad, in PDF user space. Orientation
/// agnostic, so it is right for turned and skewed quads.
pub fn pdf_quad_bounds(q: PdfQuad) -> PdfRect {
    let xs = [q.upper_left.x, q.upper_right.x, q.lower_left.x, q.lower_right.x];
    let ys = [q.upper_left.y, q.upper_right.y, q.lower_left.y, q.lower_right.y];
    PdfRect {
        left: js::min_of(&xs),
        right: js::max_of(&xs),
        bottom: js::min_of(&ys),
        top: js::max_of(&ys),
    }
}

const QUAD_EPSILON: f64 = 1e-6;

fn is_finite_point(p: PdfPoint) -> bool {
    p.x.is_finite() && p.y.is_finite()
}

/// Whether the four points, read as upper-left, upper-right, lower-left,
/// lower-right, make a quad (`isZigzagQuad`).
fn is_zigzag_quad([ul, ur, ll, lr]: PdfQuadPoints) -> bool {
    if ![ul, ur, ll, lr].into_iter().all(is_finite_point) {
        return false;
    }
    let edge = |from: PdfPoint, to: PdfPoint| Point { x: to.x - from.x, y: to.y - from.y };
    let upper = edge(ul, ur);
    let lower = edge(ll, lr);
    let left = edge(ul, ll);
    let right = edge(ur, lr);
    let lengths = [upper, lower, left, right].map(|e| js::hypot(e.x, e.y));
    if lengths.iter().any(|length| *length <= QUAD_EPSILON) {
        return false;
    }
    // Opposite edges run the same way: a crossed or reversed order fails here.
    if upper.x * lower.x + upper.y * lower.y <= 0.0 {
        return false;
    }
    if left.x * right.x + left.y * right.y <= 0.0 {
        return false;
    }
    // Both ends have area and turn the same way.
    let left_area = upper.x * left.y - upper.y * left.x;
    let right_area = lower.x * right.y - lower.y * right.x;
    if left_area.abs() <= QUAD_EPSILON * lengths[0] * lengths[2] {
        return false;
    }
    if right_area.abs() <= QUAD_EPSILON * lengths[1] * lengths[3] {
        return false;
    }
    // `Math.sign(a) === Math.sign(b)`: NaN never matches.
    js::sign(left_area) == js::sign(right_area)
}

/// Name the corners of a `/QuadPoints` entry. Writers disagree on the order:
///
/// 1. the order Acrobat writes and reads (upper-left, upper-right,
///    lower-left, lower-right) is kept, turned quads included;
/// 2. the ring order some writers use (upper-left, upper-right, lower-right,
///    lower-left) is repaired;
/// 3. anything else is named from its shape: the corners go round their
///    middle, the edge highest on the page (largest y) is the upper one, and
///    its end with the smaller x is the left.
pub fn normalize_pdf_quad(points: PdfQuadPoints) -> PdfQuad {
    let [first, second, third, fourth] = points;
    if is_zigzag_quad(points) {
        return Quad {
            upper_left: first,
            upper_right: second,
            lower_left: third,
            lower_right: fourth,
        };
    }
    if is_zigzag_quad([first, second, fourth, third]) {
        return Quad {
            upper_left: first,
            upper_right: second,
            lower_left: fourth,
            lower_right: third,
        };
    }

    let finite = points.map(|p| if is_finite_point(p) { p } else { Point { x: 0.0, y: 0.0 } });
    // `reduce((sum, p) => sum + p.x, 0)`: from +0, left to right.
    let cx = finite.iter().fold(0.0, |sum, p| sum + p.x) / 4.0;
    let cy = finite.iter().fold(0.0, |sum, p| sum + p.y) / 4.0;
    let mut ring = finite;
    // `Array.prototype.sort` is stable, as `sort_by` is; the comparator's
    // number orders by its sign, and NaN compares as equal.
    ring.sort_by(|a, b| {
        let d = (a.y - cy).atan2(a.x - cx) - (b.y - cy).atan2(b.x - cx);
        d.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut upper = 0;
    for i in 1..4 {
        if ring[i].y + ring[(i + 1) % 4].y > ring[upper].y + ring[(upper + 1) % 4].y {
            upper = i;
        }
    }
    // Round the ring: upper ends, then the lower ends back.
    let upper_ends = [ring[upper], ring[(upper + 1) % 4]];
    let lower_ends = [ring[(upper + 3) % 4], ring[(upper + 2) % 4]];
    if upper_ends[0].x <= upper_ends[1].x {
        Quad {
            upper_left: upper_ends[0],
            upper_right: upper_ends[1],
            lower_left: lower_ends[0],
            lower_right: lower_ends[1],
        }
    } else {
        Quad {
            upper_left: upper_ends[1],
            upper_right: upper_ends[0],
            lower_left: lower_ends[1],
            lower_right: lower_ends[0],
        }
    }
}
