//! `endings.ts`: the `/LE` arrowheads and tip shapes. One spec per ending
//! drives both the rendered nodes and the bounding-box points.

use std::f64::consts::PI;

use crate::js;
use crate::types::{LineEnding, Point, Rect, RenderNode};

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Poly,
    Ellipse,
}

struct EndingSpec {
    shape: Shape,
    closed: bool,
    /// Final rotation given the segment angle pointing into the tip.
    rotation: fn(f64) -> f64,
    /// Local-frame points (tip at origin, body along -X).
    points: fn(f64, bool) -> Vec<Point>,
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

fn cap_points(half: f64) -> Vec<Point> {
    vec![Point { x: -half, y: 0.0 }, Point { x: half, y: 0.0 }]
}

fn spec_of(ending: Option<LineEnding>) -> Option<EndingSpec> {
    let same = |a: f64| a;
    let reversed = |a: f64| a + PI;
    Some(match ending? {
        LineEnding::None => return None,
        LineEnding::OpenArrow => {
            EndingSpec { shape: Shape::Poly, closed: false, rotation: same, points: arrow_points }
        }
        LineEnding::ClosedArrow => {
            EndingSpec { shape: Shape::Poly, closed: true, rotation: same, points: arrow_points }
        }
        LineEnding::ROpenArrow => EndingSpec {
            shape: Shape::Poly,
            closed: false,
            rotation: reversed,
            points: arrow_points,
        },
        LineEnding::RClosedArrow => EndingSpec {
            shape: Shape::Poly,
            closed: true,
            rotation: reversed,
            points: arrow_points,
        },
        LineEnding::Circle => EndingSpec {
            shape: Shape::Ellipse,
            closed: true,
            rotation: |_| 0.0,
            points: |sw, _| {
                let r = (sw * 5.0) / 2.0;
                vec![Point { x: -r, y: -r }, Point { x: r, y: r }]
            },
        },
        LineEnding::Square => EndingSpec {
            shape: Shape::Poly,
            closed: true,
            rotation: same,
            points: |sw, _| {
                let h = (sw * 6.0) / 2.0;
                vec![
                    Point { x: -h, y: -h },
                    Point { x: h, y: -h },
                    Point { x: h, y: h },
                    Point { x: -h, y: h },
                ]
            },
        },
        LineEnding::Diamond => EndingSpec {
            shape: Shape::Poly,
            closed: true,
            rotation: same,
            points: |sw, _| {
                let h = (sw * 6.0) / 2.0;
                vec![
                    Point { x: h, y: 0.0 },
                    Point { x: 0.0, y: h },
                    Point { x: -h, y: 0.0 },
                    Point { x: 0.0, y: -h },
                ]
            },
        },
        LineEnding::Butt => EndingSpec {
            shape: Shape::Poly,
            closed: false,
            rotation: |a| a + PI / 2.0,
            points: |sw, _| cap_points((sw * 6.0) / 2.0),
        },
        LineEnding::Slash => EndingSpec {
            shape: Shape::Poly,
            closed: false,
            rotation: |a| a + PI / 1.5,
            points: |sw, _| cap_points((sw * 18.0) / 2.0),
        },
    })
}

fn rotate_translate(p: Point, angle: f64, tip: Point) -> Point {
    let c = angle.cos();
    let s = angle.sin();
    Point { x: tip.x + p.x * c - p.y * s, y: tip.y + p.x * s + p.y * c }
}

fn placed(spec: &EndingSpec, tip: Point, angle: f64, stroke_width: f64) -> Vec<Point> {
    let rot = (spec.rotation)(angle);
    (spec.points)(stroke_width, spec.closed)
        .into_iter()
        .map(|p| rotate_translate(p, rot, tip))
        .collect()
}

/// Content-space points an ending contributes to the visual bounds.
pub fn ending_points(
    tip: Point,
    angle: f64,
    ending: Option<LineEnding>,
    stroke_width: f64,
) -> Vec<Point> {
    spec_of(ending).map_or_else(Vec::new, |spec| placed(&spec, tip, angle, stroke_width))
}

/// Content-space render nodes for an ending.
pub fn ending_nodes(
    tip: Point,
    angle: f64,
    ending: Option<LineEnding>,
    stroke_width: f64,
) -> Vec<RenderNode> {
    let Some(spec) = spec_of(ending) else {
        return Vec::new();
    };
    let pts = placed(&spec, tip, angle, stroke_width);
    if spec.shape == Shape::Ellipse {
        let (a, b) = (pts[0], pts[1]);
        return vec![RenderNode::Ellipse {
            rect: Rect {
                x: js::min(a.x, b.x),
                y: js::min(a.y, b.y),
                width: (b.x - a.x).abs(),
                height: (b.y - a.y).abs(),
            },
        }];
    }
    vec![RenderNode::Poly { points: pts, closed: spec.closed }]
}
