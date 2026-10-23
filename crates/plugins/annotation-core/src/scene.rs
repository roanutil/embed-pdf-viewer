//! `scene.ts`: `scene(item)`, the render contract. A render item becomes a
//! flat list of fully painted nodes; a renderer maps each to one element.

use epdf_geometry::{quad_bounds, quad_ring};
use serde_json::Value;

use crate::appearance::draws_live;
use crate::font_css::css_font_family_for_font;
use crate::geometry::geom_scene;
use crate::helvetica::{HELVETICA_ASCENT, HELVETICA_LINE_HEIGHT, helvetica_advance};
use crate::icons::{file_attachment_icon_scene, note_icon_scene};
use crate::js;
use crate::json;
use crate::kinds::{blend_for, dash_of};
use crate::measurement::{MeasurementAppearance, distance_scene, measurement_caption_scene};
use crate::measurement_shape::shape_measurement_layout;
use crate::shapes::caret::CARET_STROKE_WIDTH;
use crate::types::{
    ItemSource, Paint, Point, Quad, Rect, RedactLabel, RenderItem, RenderNode, SceneNode, Shape,
    Style, TextScene, TextStyle,
};

fn num(value: f64) -> f64 {
    js::fixed(value, 3)
}

fn s(text: &str) -> Value {
    Value::String(text.into())
}

/// `shapePaint(style, closed)`, its blend laid over it.
fn shape_paint(style: &Style, closed: bool) -> Paint {
    let cloudy = style.cloudy_intensity.as_option().is_some_and(|i| *i != 0.0 && !i.is_nan());
    let mut keys = vec!["fill", "stroke", "width", "opacity", "dash"];
    if cloudy {
        keys.push("join");
    }
    keys.push("blend");
    Paint {
        fill: if closed { style.interior_color.as_option().map(|c| s(c)) } else { None },
        stroke: style.color.clone(),
        width: Some(style.stroke_width),
        opacity: style.opacity.clone(),
        dash: dash_of(style),
        join: cloudy.then(|| "round".into()),
        blend: blend_for(style),
        ..Paint::default()
    }
    .ordered(&keys)
}

/// A smooth squiggle along a baseline.
fn squiggle_path(start: Point, direction: Point, normal: Point, length: f64, amp: f64) -> String {
    let half = js::max(2.0, amp * 1.5);
    let at = |distance: f64, off: f64| Point {
        x: start.x + direction.x * distance + normal.x * off,
        y: start.y + direction.y * distance + normal.y * off,
    };
    let fmt =
        |p: Point| format!("{} {}", js::number_to_string(num(p.x)), js::number_to_string(num(p.y)));
    let hump = at(half / 2.0, amp);
    let mut d = format!("M {} Q {} {}", fmt(at(0.0, 0.0)), fmt(hump), fmt(at(half, 0.0)));
    let mut position = half;
    while position + half <= length + 0.5 {
        d.push_str(&format!(" T {}", fmt(at(position + half, 0.0))));
        position += half;
    }
    d
}

fn line_paint(style: &Style, width: f64) -> Paint {
    Paint {
        stroke: style.color.clone(),
        width: Some(width),
        opacity: style.opacity.clone(),
        blend: blend_for(style),
        ..Paint::default()
    }
    .ordered(&["stroke", "width", "opacity", "blend"])
}

/// Per-subtype markup nodes on the quads' own edges.
fn markup_scene(subtype: &str, quads: &[Quad], style: &Style) -> Vec<SceneNode> {
    let mut nodes = Vec::new();
    for quad in quads {
        let down = Point {
            x: quad.lower_left.x - quad.upper_left.x,
            y: quad.lower_left.y - quad.upper_left.y,
        };
        let ink_height = js::hypot(down.x, down.y);
        let w = Point {
            x: quad.lower_right.x - quad.lower_left.x,
            y: quad.lower_right.y - quad.lower_left.y,
        };
        let baseline_length = js::hypot(w.x, w.y);
        if baseline_length <= 0.0 || ink_height <= 0.0 {
            continue;
        }
        let normal = Point { x: down.x / ink_height, y: down.y / ink_height };
        let lw = js::min(2.5, js::max(0.75, ink_height * 0.06));
        match subtype {
            "underline" => nodes.push(SceneNode::painted(
                &RenderNode::line(
                    Point {
                        x: quad.lower_left.x - normal.x * lw,
                        y: quad.lower_left.y - normal.y * lw,
                    },
                    Point {
                        x: quad.lower_right.x - normal.x * lw,
                        y: quad.lower_right.y - normal.y * lw,
                    },
                ),
                line_paint(style, lw),
            )),
            "strikeout" => nodes.push(SceneNode::painted(
                &RenderNode::line(
                    Point {
                        x: (quad.upper_left.x + quad.lower_left.x) / 2.0,
                        y: (quad.upper_left.y + quad.lower_left.y) / 2.0,
                    },
                    Point {
                        x: (quad.upper_right.x + quad.lower_right.x) / 2.0,
                        y: (quad.upper_right.y + quad.lower_right.y) / 2.0,
                    },
                ),
                line_paint(style, lw),
            )),
            "squiggly" => {
                let amp = js::min(2.0, js::max(1.0, ink_height * 0.08));
                let direction = Point { x: w.x / baseline_length, y: w.y / baseline_length };
                let start = Point {
                    x: quad.lower_left.x - normal.x * amp,
                    y: quad.lower_left.y - normal.y * amp,
                };
                let d = squiggle_path(
                    start,
                    direction,
                    Point { x: -normal.x, y: -normal.y },
                    baseline_length,
                    amp,
                );
                nodes.push(SceneNode::path(d, line_paint(style, lw)));
            }
            _ => nodes.push(SceneNode::painted(
                &RenderNode::poly(quad_ring(*quad).to_vec(), true),
                Paint {
                    fill: style.color.clone(),
                    opacity: style.opacity.clone(),
                    blend: blend_for(style),
                    ..Paint::default()
                }
                .ordered(&["fill", "opacity", "blend"]),
            )),
        }
    }
    nodes
}

struct RedactRegion {
    ring: [Point; 4],
    bounds: Rect,
}

fn rect_ring(rect: Rect) -> [Point; 4] {
    [
        Point { x: rect.x, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y },
        Point { x: rect.x + rect.width, y: rect.y + rect.height },
        Point { x: rect.x, y: rect.y + rect.height },
    ]
}

fn redact_regions(geometry: &Shape) -> Vec<RedactRegion> {
    match geometry {
        Shape::Quads(q) => q
            .quad_points
            .iter()
            .filter_map(|quad| {
                let bounds = quad_bounds(*quad);
                (bounds.width > 0.0 && bounds.height > 0.0)
                    .then(|| RedactRegion { ring: quad_ring(*quad), bounds })
            })
            .collect(),
        Shape::Box(b) => vec![RedactRegion { ring: rect_ring(b.box_), bounds: b.box_ }],
        _ => Vec::new(),
    }
}

const REPEAT_FIT_FONT_SIZE: f64 = 12.0;
const FIT_FONT_SIZES: [f64; 25] = [
    4.0, 6.0, 8.0, 9.0, 10.0, 12.0, 14.0, 18.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0,
    60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 144.0,
];

fn fit_font_size(region: Rect, advance: f64) -> f64 {
    let mut fitted = FIT_FONT_SIZES[0];
    for size in FIT_FONT_SIZES {
        if size * HELVETICA_LINE_HEIGHT > region.height || size * advance > region.width {
            break;
        }
        fitted = size;
    }
    fitted
}

/// `layoutRedactLabel(region, label, text)`.
pub fn layout_redact_label(
    region: Rect,
    label: &RedactLabel,
    text: Option<&TextStyle>,
) -> Vec<SceneNode> {
    if label.text.is_empty() {
        return Vec::new();
    }
    let repeat = json::truthy(label.repeat.as_ref());
    let advance = helvetica_advance(&label.text);
    let fit = if repeat { REPEAT_FIT_FONT_SIZE } else { fit_font_size(region, advance) };
    let size = text.map(TextStyle::size).filter(|s| *s > 0.0).unwrap_or(fit);
    let font_size = js::max(4.0, size);
    let text_w = advance * font_size;
    let gap = helvetica_advance(" ") * font_size;
    let line_h = font_size * HELVETICA_LINE_HEIGHT;
    let fill =
        text.and_then(|t| t.font_color.clone()).filter(|c| !c.is_null()).unwrap_or(s("#ffffff"));
    let paint = Paint { fill: Some(fill), opacity: Some(json::num(1.0)), ..Paint::default() }
        .ordered(&["fill", "opacity"]);
    let font_family =
        text.and_then(TextStyle::family).filter(|f| !f.is_empty()).map(css_font_family_for_font);
    let baseline = |row_top: f64| row_top + font_size * HELVETICA_ASCENT;
    let align = text.and_then(TextStyle::align);
    let line_start = |width: f64| match align {
        Some("center") => region.x + js::max(0.0, (region.width - width) / 2.0),
        Some("right") => region.x + js::max(0.0, region.width - width),
        _ => region.x,
    };
    let node = |at: Point| {
        SceneNode::Text(TextScene {
            at,
            text: label.text.clone(),
            font_size,
            font_family: font_family.clone(),
            rotation: None,
            paint: paint.clone(),
            clip: None,
            order: Default::default(),
        })
    };
    if !repeat {
        if font_size > region.height {
            return Vec::new();
        }
        return vec![node(Point { x: line_start(text_w), y: baseline(region.y) })];
    }
    let cols = js::max(1.0, ((region.width + gap) / (text_w + gap)).floor());
    let rows = js::max(1.0, (region.height / line_h).floor());
    let left = line_start(cols * text_w + (cols - 1.0) * gap);
    let mut nodes = Vec::new();
    let mut row = 0.0;
    while row < rows && nodes.len() < 400 {
        let mut column = 0.0;
        while column < cols && nodes.len() < 400 {
            nodes.push(node(Point {
                x: left + column * (text_w + gap),
                y: baseline(region.y + row * line_h),
            }));
            column += 1.0;
        }
        row += 1.0;
    }
    nodes
}

fn redact_scene(item: &RenderItem) -> Vec<SceneNode> {
    let regions = redact_regions(&item.geometry);
    if item.hovered != Some(true) {
        let width = if item.style.stroke_width != 0.0 && !item.style.stroke_width.is_nan() {
            item.style.stroke_width
        } else {
            1.5
        };
        let paint = Paint {
            stroke: item.style.color.clone(),
            width: Some(width),
            opacity: item.style.opacity.clone(),
            ..Paint::default()
        }
        .ordered(&["stroke", "width", "opacity"]);
        return regions
            .iter()
            .map(|r| SceneNode::painted(&RenderNode::poly(r.ring.to_vec(), true), paint.clone()))
            .collect();
    }
    let mut nodes = Vec::new();
    if let Some(fill) = item.style.interior_color.as_option().filter(|c| !c.is_empty()) {
        let paint =
            Paint { fill: Some(s(fill)), opacity: Some(json::num(1.0)), ..Paint::default() }
                .ordered(&["fill", "opacity"]);
        for region in &regions {
            nodes.push(SceneNode::painted(
                &RenderNode::poly(region.ring.to_vec(), true),
                paint.clone(),
            ));
        }
    }
    if let Some(label) = &item.label {
        for region in &regions {
            nodes.extend(layout_redact_label(region.bounds, label, item.text.as_ref()));
        }
    }
    nodes
}

/// `hasLiveDrawing(item)`.
pub fn has_live_drawing(item: &RenderItem) -> bool {
    item.source != ItemSource::Baked
        || item.lent_scene.0.is_some()
        || item.annotation.as_ref().is_some_and(|a| draws_live(a, false))
}

/// `scene(item)`: the full painted scene for one annotation.
pub fn scene(item: &RenderItem) -> Vec<SceneNode> {
    if let Some(lent) = &item.lent_scene.0 {
        return lent();
    }
    if item.subtype == "link" {
        return Vec::new();
    }
    let icon = item.icon.as_ref().and_then(Value::as_str);
    if let Shape::Box(b) = &item.geometry {
        if item.subtype == "text" {
            return note_icon_scene(icon, b.box_, &item.style);
        }
        if item.subtype == "file-attachment" {
            return file_attachment_icon_scene(icon, b.box_, &item.style);
        }
    }
    let measure = item.measure.clone().map(MeasurementAppearance);
    if let Some(m) = measure.as_ref().filter(|m| m.is_distance()) {
        return distance_scene(&item.geometry, m, &item.style);
    }
    if item.subtype == "redact" {
        return redact_scene(item);
    }
    if let Shape::Quads(q) = &item.geometry {
        return markup_scene(&item.subtype, &q.quad_points, &item.style);
    }
    if let Shape::Caret(_) = &item.geometry {
        let paint = Paint {
            fill: item.style.color.clone(),
            stroke: item.style.color.clone(),
            width: Some(CARET_STROKE_WIDTH),
            opacity: item.style.opacity.clone(),
            ..Paint::default()
        }
        .ordered(&["fill", "stroke", "width", "opacity"]);
        return geom_scene(&item.geometry, item.style.stroke())
            .iter()
            .map(|n| SceneNode::painted(n, paint.clone()))
            .collect();
    }
    let ink = matches!(item.geometry, Shape::Ink(_));
    let mut nodes: Vec<SceneNode> = geom_scene(&item.geometry, item.style.stroke())
        .iter()
        .map(|node| {
            let closed = match node {
                RenderNode::Rect(_) | RenderNode::Ellipse(_) | RenderNode::Path(_) => true,
                RenderNode::Poly(p) => p.closed,
                RenderNode::Line(_) => false,
            };
            let mut paint = shape_paint(&item.style, closed);
            if ink {
                paint.line_cap = Some("round".into());
                paint.join = Some("round".into());
            }
            SceneNode::painted(node, paint)
        })
        .collect();
    if let Some(m) = &measure {
        let layout = shape_measurement_layout(&item.geometry, m, &item.style);
        nodes.extend(measurement_caption_scene(
            layout.and_then(|l| l.caption).as_ref(),
            &item.style,
        ));
    }
    nodes
}
