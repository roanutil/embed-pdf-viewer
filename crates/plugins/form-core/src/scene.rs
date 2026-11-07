//! `scene.ts`: a widget's drawing on the page. The generator's operations go
//! from the widget's upright frame (PDF points, y up) to page space (y down),
//! turned by the widget's quarter turn about the middle of its box, as scene
//! nodes. Paths become SVG path data; each text run becomes a `textRun` node
//! at the place the engine puts it, in its font's CSS (its program first,
//! which the node carries for mounting).
//!
//! [`SceneNode`] and [`Paint`] are `core-annotation`'s types
//! (`epdf-annotation-core`), as the TypeScript imports them: a `path` and a
//! `textRun` node, each optionally clipped.

use epdf_annotation_core::{FontProgramRef, ShapeScene, TextRunScene};
use epdf_fonts::FontStyle;
use epdf_geometry::{Point, Rect, js};
use serde::Deserialize;
use serde_json::Value;

pub use epdf_annotation_core::{Paint, SceneNode};

use crate::{DrawOp, PathSegment, PdfRect, Rgb, WidgetDrawing};

/// Where a widget is: its box in page space, before the turn, and the turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct WidgetPlacement {
    #[serde(rename = "box")]
    pub box_: Rect,
    /// Degrees clockwise, a quarter turn, about the middle of `box_`.
    pub rotation: Option<f64>,
}

fn style_name(style: FontStyle) -> &'static str {
    match style {
        FontStyle::Normal => "normal",
        FontStyle::Italic => "italic",
    }
}

fn path_node(d: String, paint: Paint, clip: Option<Rect>) -> SceneNode {
    SceneNode::Path(
        ShapeScene {
            rect: None,
            a: None,
            b: None,
            points: None,
            closed: None,
            d: Some(d),
            paint,
            clip,
            order: Default::default(),
        }
        .ordered(&["d", "paint", "clip"]),
    )
}

fn color(hex: String) -> Option<Value> {
    Some(Value::String(hex))
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
            DrawOp::Fill { path, color: rgb, even_odd } => nodes.push(path_node(
                path_data(path),
                Paint {
                    fill: color(hex_of(*rgb)),
                    fill_rule: even_odd.then(|| "evenodd".into()),
                    ..Paint::default()
                },
                clip,
            )),
            DrawOp::Stroke { path, color: rgb, width, dash } => nodes.push(path_node(
                path_data(path),
                Paint {
                    stroke: color(hex_of(*rgb)),
                    width: Some(*width),
                    dash: dash.clone(),
                    ..Paint::default()
                },
                clip,
            )),
            DrawOp::Clip { rect, ops } => stack.push((ops, Some(clip_of(rect)), 0)),
            DrawOp::Text { size, color: rgb, runs } => {
                for run in runs {
                    let at = to_page(run.x, run.y);
                    nodes.push(SceneNode::TextRun(TextRunScene {
                        at: Point { x: round(at.x), y: round(at.y) },
                        text: run.text.clone(),
                        rtl: run.rtl,
                        plain: run.plain,
                        joins_previous: run.joins_previous.then_some(true),
                        joins_next: run.joins_next.then_some(true),
                        font_size: *size,
                        font_family: run.font.font_family.clone(),
                        font_weight: Some(f64::from(run.font.font_weight)),
                        font_style: Some(style_name(run.font.font_style).into()),
                        // `turn` is falsy at 0 and NaN.
                        rotation: (turn != 0.0 && !turn.is_nan()).then_some(turn),
                        program: Some(FontProgramRef(run.font.program.clone())),
                        paint: Paint { fill: color(hex_of(*rgb)), ..Paint::default() },
                        clip,
                        order: Default::default(),
                    }));
                }
            }
        }
    }
    nodes
}
