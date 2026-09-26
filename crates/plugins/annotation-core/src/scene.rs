//! `scene.ts`: the render contract. A render item becomes a flat list of
//! painted nodes; a framework maps each to one element.

use epdf_geometry::{text_quad_bounds, text_quad_ring};

use crate::geometry::geom_scene;
use crate::js;
use crate::measurement::{MeasurementAppearance, distance_scene, measurement_caption_scene};
use crate::measurement_shape::shape_measurement_layout;
use crate::types::{
    BlendMode, Border, Geom, Label, Paint, Point, Quad, Rect, RenderItem, RenderNode, Round,
    SceneNode, Style, TextAlign, TextQuad, TextStyle,
};

/// `Number(n.toFixed(3))`.
fn num(n: f64) -> f64 {
    js::fixed(n, 3)
}

/// CSS mix-blend-mode for live vector paint; `normal` needs none.
pub fn blend_for(style: &Style) -> Option<BlendMode> {
    (style.blend_mode != BlendMode::Normal).then_some(style.blend_mode)
}

fn shape_paint(style: &Style, closed: bool) -> Paint {
    Paint {
        fill: if closed { style.interior_color.clone() } else { None },
        stroke: Some(style.color.clone()),
        width: Some(style.stroke_width),
        opacity: Some(style.opacity),
        dash: match &style.border {
            Border::Dashed { dash } => Some(dash.clone()),
            _ => None,
        },
        join: matches!(style.border, Border::Cloudy { .. }).then_some(Round::Round),
        ..Paint::default()
    }
}

/// A smooth squiggle along an arbitrary baseline: one `Q` hump, then
/// reflected `T` segments.
fn squiggle_path(start: Point, u: Point, n: Point, w: f64, amp: f64) -> String {
    let half = js::max(2.0, amp * 1.5);
    let at = |t: f64, off: f64| Point {
        x: start.x + u.x * t + n.x * off,
        y: start.y + u.y * t + n.y * off,
    };
    let p =
        |v: Point| format!("{} {}", js::number_to_string(num(v.x)), js::number_to_string(num(v.y)));
    let hump = at(half / 2.0, amp);
    let mut d = format!("M {} Q {} {}", p(at(0.0, 0.0)), p(hump), p(at(half, 0.0)));
    let mut t = half;
    while t + half <= w + 0.5 {
        d.push_str(&format!(" T {}", p(at(t + half, 0.0))));
        t += half;
    }
    d
}

fn markup_scene(subtype: &str, quads: &[TextQuad], style: &Style) -> Vec<SceneNode> {
    let color = &style.color;
    let opacity = style.opacity;
    let mut nodes = Vec::new();
    let stroke = |lw: f64| Paint {
        stroke: Some(color.clone()),
        width: Some(lw),
        opacity: Some(opacity),
        blend: blend_for(style),
        ..Paint::default()
    };
    for q in quads {
        let down =
            Point { x: q.lower_start.x - q.upper_start.x, y: q.lower_start.y - q.upper_start.y };
        let h = js::hypot(down.x, down.y);
        let w_vec =
            Point { x: q.lower_end.x - q.lower_start.x, y: q.lower_end.y - q.lower_start.y };
        let w = js::hypot(w_vec.x, w_vec.y);
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        let n = Point { x: down.x / h, y: down.y / h };
        let lw = js::min(2.5, js::max(0.75, h * 0.06));
        match subtype {
            "underline" => nodes.push(SceneNode::Line {
                a: Point { x: q.lower_start.x - n.x * lw, y: q.lower_start.y - n.y * lw },
                b: Point { x: q.lower_end.x - n.x * lw, y: q.lower_end.y - n.y * lw },
                paint: stroke(lw),
            }),
            "strikeout" => nodes.push(SceneNode::Line {
                a: Point {
                    x: (q.upper_start.x + q.lower_start.x) / 2.0,
                    y: (q.upper_start.y + q.lower_start.y) / 2.0,
                },
                b: Point {
                    x: (q.upper_end.x + q.lower_end.x) / 2.0,
                    y: (q.upper_end.y + q.lower_end.y) / 2.0,
                },
                paint: stroke(lw),
            }),
            "squiggly" => {
                let amp = js::min(2.0, js::max(1.0, h * 0.08));
                let u = Point { x: w_vec.x / w, y: w_vec.y / w };
                let start =
                    Point { x: q.lower_start.x - n.x * amp, y: q.lower_start.y - n.y * amp };
                nodes.push(SceneNode::Path {
                    d: squiggle_path(start, u, Point { x: -n.x, y: -n.y }, w, amp),
                    paint: stroke(lw),
                });
            }
            _ => nodes.push(SceneNode::Poly {
                points: text_quad_ring(*q).to_vec(),
                closed: true,
                paint: Paint {
                    fill: Some(color.clone()),
                    opacity: Some(opacity),
                    blend: blend_for(style),
                    ..Paint::default()
                },
            }),
        }
    }
    nodes
}

struct RedactRegion {
    ring: Quad,
    bounds: Rect,
}

fn rect_ring(r: Rect) -> Quad {
    [
        Point { x: r.x, y: r.y },
        Point { x: r.x + r.width, y: r.y },
        Point { x: r.x + r.width, y: r.y + r.height },
        Point { x: r.x, y: r.y + r.height },
    ]
}

fn redact_regions(geom: &Geom) -> Vec<RedactRegion> {
    match geom {
        Geom::Quads { quads } => quads
            .iter()
            .filter_map(|q| {
                let bounds = text_quad_bounds(*q);
                (bounds.width > 0.0 && bounds.height > 0.0)
                    .then(|| RedactRegion { ring: text_quad_ring(*q), bounds })
            })
            .collect(),
        Geom::Rect { rect, .. } => vec![RedactRegion { ring: rect_ring(*rect), bounds: *rect }],
        _ => Vec::new(),
    }
}

/// Redaction label layout: top-aligned, `/Q` alignment, `/Repeat` tiling a
/// grid that fits the region (at most 400 nodes). Character advance is
/// estimated at 0.55 em; the length is in UTF-16 code units.
pub fn layout_redact_label(
    region: Rect,
    label: &Label,
    text: Option<&TextStyle>,
) -> Vec<SceneNode> {
    if label.text.is_empty() {
        return Vec::new();
    }
    let size = match text {
        Some(t) if t.font_size > 0.0 => t.font_size,
        _ => region.height * 0.6,
    };
    let font_size = js::max(4.0, size);
    let char_w = font_size * 0.55;
    let text_w = js::utf16_len(&label.text) as f64 * char_w;
    let line_h = font_size * 1.2;
    let paint = Paint {
        fill: Some(text.map_or_else(|| "#ffffff".to_string(), |t| t.font_color.clone())),
        opacity: Some(1.0),
        ..Paint::default()
    };
    let font_family = text.map(|t| t.font_family.clone()).filter(|f| !f.is_empty());
    let node = |at: Point| SceneNode::Text {
        at,
        text: label.text.clone(),
        font_size,
        font_family: font_family.clone(),
        rotation: None,
        paint: paint.clone(),
    };
    let baseline = |row_top: f64| row_top + font_size * 0.95;

    if !label.repeat {
        if font_size > region.height {
            return Vec::new();
        }
        let x = match text.map(|t| t.text_align) {
            Some(TextAlign::Center) => region.x + js::max(0.0, (region.width - text_w) / 2.0),
            Some(TextAlign::Right) => region.x + js::max(0.0, region.width - text_w),
            _ => region.x,
        };
        return vec![node(Point { x, y: baseline(region.y) })];
    }

    let cols = js::max(1.0, ((region.width + char_w) / (text_w + char_w)).floor());
    let rows = js::max(1.0, (region.height / line_h).floor());
    let mut nodes = Vec::new();
    let mut r = 0.0;
    while r < rows && nodes.len() < 400 {
        let mut c = 0.0;
        while c < cols && nodes.len() < 400 {
            nodes.push(node(Point {
                x: region.x + c * (text_w + char_w),
                y: baseline(region.y + r * line_h),
            }));
            c += 1.0;
        }
        r += 1.0;
    }
    nodes
}

fn redact_scene(item: &RenderItem) -> Vec<SceneNode> {
    let regions = redact_regions(&item.geom);
    if item.hovered != Some(true) {
        let width = if item.style.stroke_width == 0.0 || item.style.stroke_width.is_nan() {
            1.5
        } else {
            item.style.stroke_width
        };
        let paint = Paint {
            stroke: Some(item.style.color.clone()),
            width: Some(width),
            opacity: Some(item.style.opacity),
            ..Paint::default()
        };
        return regions
            .into_iter()
            .map(|region| SceneNode::Poly {
                points: region.ring.to_vec(),
                closed: true,
                paint: paint.clone(),
            })
            .collect();
    }
    let mut nodes = Vec::new();
    if let Some(fill) = item.style.interior_color.as_ref().filter(|c| !c.is_empty()) {
        let paint = Paint { fill: Some(fill.clone()), opacity: Some(1.0), ..Paint::default() };
        for region in &regions {
            nodes.push(SceneNode::Poly {
                points: region.ring.to_vec(),
                closed: true,
                paint: paint.clone(),
            });
        }
    }
    if let Some(label) = &item.label {
        for region in &regions {
            nodes.extend(layout_redact_label(region.bounds, label, item.text.as_ref()));
        }
    }
    nodes
}

/// The full painted scene for one annotation.
pub fn scene(item: &RenderItem) -> Vec<SceneNode> {
    if item.subtype == "link" {
        return Vec::new();
    }
    if let Some(MeasurementAppearance::Distance(d)) = &item.measure {
        return distance_scene(&item.geom, d, &item.style);
    }
    if item.subtype == "redact" {
        return redact_scene(item);
    }
    if let Geom::Quads { quads } = &item.geom {
        return markup_scene(&item.subtype, quads, &item.style);
    }
    if let Geom::Caret { .. } = item.geom {
        let paint = Paint {
            fill: Some(item.style.color.clone()),
            stroke: Some(item.style.color.clone()),
            width: Some(0.5),
            opacity: Some(item.style.opacity),
            ..Paint::default()
        };
        return geom_scene(&item.geom, 0.0, None)
            .into_iter()
            .map(|n| SceneNode::painted(n, paint.clone()))
            .collect();
    }
    let ink = matches!(item.geom, Geom::Ink { .. });
    let mut nodes: Vec<SceneNode> =
        geom_scene(&item.geom, item.style.stroke_width, Some(&item.style.border))
            .into_iter()
            .map(|n| {
                let closed = match &n {
                    RenderNode::Rect { .. }
                    | RenderNode::Ellipse { .. }
                    | RenderNode::Path { .. } => true,
                    RenderNode::Poly { closed, .. } => *closed,
                    RenderNode::Line { .. } => false,
                };
                let mut paint =
                    Paint { blend: blend_for(&item.style), ..shape_paint(&item.style, closed) };
                if ink {
                    paint.cap = Some(Round::Round);
                    paint.join = Some(Round::Round);
                }
                SceneNode::painted(n, paint)
            })
            .collect();
    if let Some(measure) = &item.measure {
        let caption = match measure {
            MeasurementAppearance::Shape(s) => {
                shape_measurement_layout(&item.geom, s, &item.style).and_then(|l| l.caption)
            }
            // A distance returned above.
            MeasurementAppearance::Distance(_) => None,
        };
        nodes.extend(measurement_caption_scene(caption.as_ref(), &item.style));
    }
    nodes
}
