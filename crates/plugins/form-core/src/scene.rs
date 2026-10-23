//! `scene.ts`: a widget's drawing on the page. The generator's operations go
//! from the widget's upright frame (PDF points, y up) to page space (y down),
//! turned by the widget's quarter turn about the middle of its box, as scene
//! nodes. Paths become SVG path data; each text run becomes a `glyphs` node,
//! every character at the place the engine puts it, in the standard face's
//! CSS (its program first, which the node carries for mounting).
//!
//! [`SceneNode`] and [`Paint`] are the part of `core-annotation`'s types this
//! function makes: a `path` and a `glyphs` node, each optionally clipped.

use epdf_fonts::{FontProgram, FontStyle, standard_font_css, standard_font_program};
use epdf_geometry::{Point, Rect, js};
use serde::{Deserialize, Serialize};

use crate::{DrawOp, PathSegment, PdfRect, Rgb, WidgetDrawing};

/// Where a widget is: its box in page space, before the turn, and the turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct WidgetPlacement {
    #[serde(rename = "box")]
    pub box_: Rect,
    /// Degrees clockwise, a quarter turn, about the middle of `box_`.
    pub rotation: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FillRule {
    #[serde(rename = "evenodd")]
    EvenOdd,
}

/// How to paint one node; an absent member means none.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Paint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<String>,
    /// The stroke width.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill_rule: Option<FillRule>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum SceneNode {
    /// SVG path data in page space.
    Path {
        d: String,
        paint: Paint,
        /// Drawn only inside this upright rectangle.
        #[serde(skip_serializing_if = "Option::is_none")]
        clip: Option<Rect>,
    },
    /// Text placed character by character: `offsets[i]` is how far along the
    /// baseline from `at` character `i` starts. `rotation` turns the run about
    /// `at`, degrees clockwise.
    Glyphs {
        at: Point,
        text: String,
        offsets: Vec<f64>,
        font_size: f64,
        font_family: String,
        font_weight: u32,
        font_style: FontStyle,
        #[serde(skip_serializing_if = "Option::is_none")]
        rotation: Option<f64>,
        program: &'static FontProgram,
        paint: Paint,
        #[serde(skip_serializing_if = "Option::is_none")]
        clip: Option<Rect>,
    },
}

/// `Math.round(Math.min(1, Math.max(0, channel)) * 255)` in hex, two digits.
fn hex_of(color: Rgb) -> String {
    let mut hex = String::from("#");
    for channel in color {
        let value = js::round(js::min(1.0, js::max(0.0, channel)) * 255.0);
        if value.is_nan() {
            hex.push_str("NaN");
        } else {
            hex.push_str(&format!("{:02x}", value as u32));
        }
    }
    hex
}

/// `Number(value.toFixed(3))`.
fn round(value: f64) -> f64 {
    js::to_fixed(value, 3).parse().unwrap_or(f64::NAN)
}

/// Places `drawing` on the page at `placement`.
pub fn widget_scene(drawing: &WidgetDrawing, placement: &WidgetPlacement) -> Vec<SceneNode> {
    let frame = placement.box_;
    let turn = ((placement.rotation.unwrap_or(0.0) % 360.0) + 360.0) % 360.0;
    let center_x = frame.x + frame.width / 2.0;
    let center_y = frame.y + frame.height / 2.0;
    // A point of the upright frame on the page: flipped to y down, then turned
    // clockwise about the box's middle.
    let to_page = |x: f64, y: f64| -> Point {
        let dx = frame.x + x - center_x;
        let dy = frame.y + (drawing.height - y) - center_y;
        if turn == 90.0 {
            Point { x: center_x - dy, y: center_y + dx }
        } else if turn == 180.0 {
            Point { x: center_x - dx, y: center_y - dy }
        } else if turn == 270.0 {
            Point { x: center_x + dy, y: center_y - dx }
        } else {
            Point { x: center_x + dx, y: center_y + dy }
        }
    };
    let point = |x: f64, y: f64| -> String {
        let at = to_page(x, y);
        format!("{} {}", js::number_to_string(round(at.x)), js::number_to_string(round(at.y)))
    };
    let path_data = |path: &[PathSegment]| -> String {
        path.iter()
            .map(|segment| match *segment {
                PathSegment::Rect { x, y, width, height } => format!(
                    "M {} L {} L {} L {} Z",
                    point(x, y),
                    point(x + width, y),
                    point(x + width, y + height),
                    point(x, y + height)
                ),
                PathSegment::Move { x, y } => format!("M {}", point(x, y)),
                PathSegment::Line { x, y } => format!("L {}", point(x, y)),
                PathSegment::Curve { x1, y1, x2, y2, x, y } => {
                    format!("C {} {} {}", point(x1, y1), point(x2, y2), point(x, y))
                }
                PathSegment::Close => "Z".to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    let clip_of = |rect: &PdfRect| -> Rect {
        let corner = to_page(rect.left, rect.bottom);
        let opposite = to_page(rect.right, rect.top);
        Rect {
            x: js::min(corner.x, opposite.x),
            y: js::min(corner.y, opposite.y),
            width: (opposite.x - corner.x).abs(),
            height: (opposite.y - corner.y).abs(),
        }
    };
    let mut nodes = Vec::new();
    let mut stack: Vec<(&[DrawOp], Option<Rect>, usize)> = vec![(&drawing.ops, None, 0)];
    // Walk the operations depth first, in order, each clip's nested in place.
    while let Some((ops, clip, index)) = stack.pop() {
        let Some(op) = ops.get(index) else { continue };
        stack.push((ops, clip, index + 1));
        match op {
            DrawOp::Fill { path, color, even_odd } => nodes.push(SceneNode::Path {
                d: path_data(path),
                paint: Paint {
                    fill: Some(hex_of(*color)),
                    fill_rule: even_odd.then_some(FillRule::EvenOdd),
                    ..Paint::default()
                },
                clip,
            }),
            DrawOp::Stroke { path, color, width, dash } => nodes.push(SceneNode::Path {
                d: path_data(path),
                paint: Paint {
                    stroke: Some(hex_of(*color)),
                    width: Some(*width),
                    dash: dash.clone(),
                    ..Paint::default()
                },
                clip,
            }),
            DrawOp::Clip { rect, ops } => stack.push((ops, Some(clip_of(rect)), 0)),
            DrawOp::Text { font, size, color, runs } => {
                let css = standard_font_css(*font);
                for run in runs {
                    let mut offsets = Vec::with_capacity(run.advances.len());
                    let mut along = 0.0;
                    for advance in &run.advances {
                        offsets.push(round(along));
                        along += advance;
                    }
                    let at = to_page(run.x, run.y);
                    nodes.push(SceneNode::Glyphs {
                        at: Point { x: round(at.x), y: round(at.y) },
                        text: run.text.clone(),
                        offsets,
                        font_size: *size,
                        font_family: css.font_family.clone(),
                        font_weight: css.font_weight,
                        font_style: css.font_style,
                        // `turn` is falsy at 0 and NaN.
                        rotation: (turn != 0.0 && !turn.is_nan()).then_some(turn),
                        program: standard_font_program(*font),
                        paint: Paint { fill: Some(hex_of(*color)), ..Paint::default() },
                        clip,
                    });
                }
            }
        }
    }
    nodes
}
