//! `painted.ts`: what an annotation paints, as a few simple pieces, and the
//! two questions asked of them: is a point near the ink (a click), and does
//! a rectangle touch it (the marquee).

use crate::js;
use crate::rect::{
    point_in_poly, polygon_touches_rect, rect_corner_points, rotate_point, seg_dist,
    segment_rect_distance,
};
use crate::types::{Point, Rect};

/// One piece of what an annotation paints, in page space (`PaintedPiece`).
#[derive(Clone, Debug, PartialEq)]
pub enum PaintedPiece {
    /// Ink along a path, `half_width` either side of it.
    Stroke { points: Vec<Point>, closed: bool, half_width: f64 },
    /// A filled polygon (even-odd).
    Area { ring: Vec<Point> },
    /// A turned ellipse: its ink, and its inside when `filled`.
    Oval { center: Point, rx: f64, ry: f64, rotation: f64, half_width: f64, filled: bool },
}

/// `bodyPieces(ring)`: a solid body, hit inside and near its edge.
pub fn body_pieces(ring: &[Point]) -> Vec<PaintedPiece> {
    vec![
        PaintedPiece::Area { ring: ring.to_vec() },
        PaintedPiece::Stroke { points: ring.to_vec(), closed: true, half_width: 0.0 },
    ]
}

/// `paintedNear(pieces, point, margin)`.
pub fn painted_near(pieces: &[PaintedPiece], point: Point, margin: f64) -> bool {
    pieces.iter().any(|piece| piece_near(piece, point, margin))
}

/// `paintedTouches(pieces, rect)`.
pub fn painted_touches(pieces: &[PaintedPiece], rect: Rect) -> bool {
    pieces.iter().any(|piece| piece_touches(piece, rect))
}

fn some_segment(
    points: &[Point],
    closed: bool,
    mut test: impl FnMut(Point, Point) -> bool,
) -> bool {
    for i in 0..points.len().saturating_sub(1) {
        if test(points[i], points[i + 1]) {
            return true;
        }
    }
    closed && points.len() > 2 && test(points[points.len() - 1], points[0])
}

fn to_oval_frame(center: Point, rx: f64, ry: f64, rotation: f64, point: Point) -> Point {
    let local = if rotation != 0.0 && !rotation.is_nan() {
        rotate_point(point, center, -rotation)
    } else {
        point
    };
    Point { x: (local.x - center.x) / rx, y: (local.y - center.y) / ry }
}

fn piece_near(piece: &PaintedPiece, point: Point, margin: f64) -> bool {
    match piece {
        PaintedPiece::Stroke { points, closed, half_width } => {
            some_segment(points, *closed, |from, to| {
                seg_dist(point, from, to) <= half_width + margin
            })
        }
        PaintedPiece::Area { ring } => point_in_poly(point, ring),
        PaintedPiece::Oval { center, rx, ry, rotation, half_width, filled } => {
            if *rx <= 0.0 || *ry <= 0.0 {
                return false;
            }
            let local = to_oval_frame(*center, *rx, *ry, *rotation, point);
            let reach = js::hypot(local.x, local.y);
            let band = (half_width + margin) / js::min(*rx, *ry);
            if *filled { reach <= 1.0 + band } else { (reach - 1.0).abs() <= band }
        }
    }
}

fn piece_touches(piece: &PaintedPiece, rect: Rect) -> bool {
    match piece {
        PaintedPiece::Stroke { points, closed, half_width } => {
            some_segment(points, *closed, |from, to| {
                segment_rect_distance(from, to, rect) <= *half_width
            })
        }
        PaintedPiece::Area { ring } => polygon_touches_rect(ring, rect),
        PaintedPiece::Oval { center, rx, ry, rotation, half_width, filled } => {
            if *rx <= 0.0 || *ry <= 0.0 {
                return false;
            }
            let corners: Vec<Point> = rect_corner_points(rect)
                .iter()
                .map(|c| to_oval_frame(*center, *rx, *ry, *rotation, *c))
                .collect();
            let origin = Point { x: 0.0, y: 0.0 };
            let nearest = if point_in_poly(origin, &corners) {
                0.0
            } else {
                js::min_of(
                    &(0..4)
                        .map(|i| seg_dist(origin, corners[i], corners[(i + 1) % 4]))
                        .collect::<Vec<_>>(),
                )
            };
            let farthest =
                js::max_of(&corners.iter().map(|c| js::hypot(c.x, c.y)).collect::<Vec<_>>());
            let band = half_width / js::min(*rx, *ry);
            nearest <= 1.0 + band && (*filled || farthest >= 1.0 - band)
        }
    }
}
