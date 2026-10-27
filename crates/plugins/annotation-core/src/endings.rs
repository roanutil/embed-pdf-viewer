//! `endings.ts`: the line endings (`/LE`) drawn at the ends of lines and
//! polylines. One spec per ending drives the drawing (`endingNodes`), the
//! bounding points (`endingPoints`) and what it paints (`endingPieces`).

use std::f64::consts::PI;

use crate::painted::PaintedPiece;
use crate::rect::rect_center;
use crate::types::{Point, Rect, RenderNode};

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Poly,
    Ellipse,
}

struct Spec {
    shape: Shape,
    closed: bool,
    rotation: fn(f64) -> f64,
    points: fn(f64) -> Vec<Point>,
}

fn arrow_points(sw: f64, closed: bool) -> Vec<Point> {
    let len = sw * 9.0;
    let wing = PI / 6.0;
    let x = -len * wing.cos();
    let y = len * wing.sin();
    if closed {
        vec![Point { x: 0.0, y: 0.0 }, Point { x, y }, Point { x, y: -y }]
    } else {
        vec![Point { x, y }, Point { x: 0.0, y: 0.0 }, Point { x, y: -y }]
    }
}

fn open_points(sw: f64) -> Vec<Point> {
    arrow_points(sw, false)
}

fn closed_points(sw: f64) -> Vec<Point> {
    arrow_points(sw, true)
}

fn same(angle: f64) -> f64 {
    angle
}

fn reversed(angle: f64) -> f64 {
    angle + PI
}

fn line_cap(sw: f64, factor: f64) -> Vec<Point> {
    let half = (sw * factor) / 2.0;
    vec![Point { x: -half, y: 0.0 }, Point { x: half, y: 0.0 }]
}

fn spec_of(ending: Option<&str>) -> Option<Spec> {
    Some(match ending? {
        "open-arrow" => {
            Spec { shape: Shape::Poly, closed: false, rotation: same, points: open_points }
        }
        "closed-arrow" => {
            Spec { shape: Shape::Poly, closed: true, rotation: same, points: closed_points }
        }
        "r-open-arrow" => {
            Spec { shape: Shape::Poly, closed: false, rotation: reversed, points: open_points }
        }
        "r-closed-arrow" => {
            Spec { shape: Shape::Poly, closed: true, rotation: reversed, points: closed_points }
        }
        "circle" => Spec {
            shape: Shape::Ellipse,
            closed: true,
            rotation: |_| 0.0,
            points: |sw| {
                let radius = (sw * 5.0) / 2.0;
                vec![Point { x: -radius, y: -radius }, Point { x: radius, y: radius }]
            },
        },
        "square" => Spec {
            shape: Shape::Poly,
            closed: true,
            rotation: same,
            points: |sw| {
                let h = (sw * 6.0) / 2.0;
                vec![
                    Point { x: -h, y: -h },
                    Point { x: h, y: -h },
                    Point { x: h, y: h },
                    Point { x: -h, y: h },
                ]
            },
        },
        "diamond" => Spec {
            shape: Shape::Poly,
            closed: true,
            rotation: same,
            points: |sw| {
                let h = (sw * 6.0) / 2.0;
                vec![
                    Point { x: h, y: 0.0 },
                    Point { x: 0.0, y: h },
                    Point { x: -h, y: 0.0 },
                    Point { x: 0.0, y: -h },
                ]
            },
        },
        "butt" => Spec {
            shape: Shape::Poly,
            closed: false,
            rotation: |a| a + PI / 2.0,
            points: |sw| line_cap(sw, 6.0),
        },
        "slash" => Spec {
            shape: Shape::Poly,
            closed: false,
            rotation: |a| a + PI / 1.5,
            points: |sw| line_cap(sw, 18.0),
        },
        _ => return None,
    })
}

fn rotate_translate(p: Point, angle: f64, tip: Point) -> Point {
    let cosine = angle.cos();
    let sine = angle.sin();
    Point { x: tip.x + p.x * cosine - p.y * sine, y: tip.y + p.x * sine + p.y * cosine }
}

/// `endingPoints(tip, angle, ending, strokeWidth)`: the page points an ending adds to the bounds.
pub fn ending_points(
    tip: Point,
    angle: f64,
    ending: Option<&str>,
    stroke_width: f64,
) -> Vec<Point> {
    let Some(spec) = spec_of(ending) else { return Vec::new() };
    let rot = (spec.rotation)(angle);
    (spec.points)(stroke_width).into_iter().map(|p| rotate_translate(p, rot, tip)).collect()
}

/// `endingNodes(tip, angle, ending, strokeWidth)`: what the renderer draws.
pub fn ending_nodes(
    tip: Point,
    angle: f64,
    ending: Option<&str>,
    stroke_width: f64,
) -> Vec<RenderNode> {
    let Some(spec) = spec_of(ending) else { return Vec::new() };
    let rot = (spec.rotation)(angle);
    let points: Vec<Point> =
        (spec.points)(stroke_width).into_iter().map(|p| rotate_translate(p, rot, tip)).collect();
    if spec.shape == Shape::Ellipse {
        let (p, o) = (points[0], points[1]);
        return vec![RenderNode::ellipse(Rect {
            x: crate::js::min(p.x, o.x),
            y: crate::js::min(p.y, o.y),
            width: (o.x - p.x).abs(),
            height: (o.y - p.y).abs(),
        })];
    }
    vec![RenderNode::poly(points, spec.closed)]
}

/// `endingPieces(nodes, strokeWidth)`: what the endings paint.
pub fn ending_pieces(nodes: &[RenderNode], stroke_width: f64) -> Vec<PaintedPiece> {
    let half_width = stroke_width / 2.0;
    let mut out = Vec::new();
    for node in nodes {
        match node {
            RenderNode::Ellipse(n) => out.push(PaintedPiece::Oval {
                center: rect_center(n.rect),
                rx: n.rect.width / 2.0,
                ry: n.rect.height / 2.0,
                rotation: 0.0,
                half_width,
                filled: true,
            }),
            RenderNode::Poly(n) => {
                if n.closed {
                    out.push(PaintedPiece::Area { ring: n.points.clone() });
                }
                out.push(PaintedPiece::Stroke {
                    points: n.points.clone(),
                    closed: n.closed,
                    half_width,
                });
            }
            _ => {}
        }
    }
    out
}
