//! `scene.ts`: `scene(item)`, the render contract. A render item becomes a
//! flat list of fully painted nodes; a renderer maps each to one element.

use epdf_geometry::{quad_bounds, quad_ring};
use epdf_text_core::{
    Alignment, FieldText, FieldTextOptions, PaintFonts, PdfRect, default_paint_fonts, layout_field,
};
use serde_json::Value;

use crate::appearance::draws_live;
use crate::geometry::geom_scene;
use crate::icons::{file_attachment_icon_scene, note_icon_scene};
use crate::js;
use crate::json;
use crate::kinds::{blend_for, dash_of};
use crate::measurement::{MeasurementAppearance, distance_scene, measurement_caption_scene};
use crate::measurement_shape::shape_measurement_layout;
use crate::shapes::caret::CARET_STROKE_WIDTH;
use crate::types::{
    FontProgramRef, ItemSource, Paint, Point, Quad, Rect, RedactLabel, RenderItem, RenderNode,
    SceneNode, Shape, Style, TextRunScene, TextStyle,
};

fn num(value: f64) -> f64 {
    js::fixed(value, 3)
}

fn s(text: &str) -> Value {
    Value::String(text.into())
}

/// `shapePaint(style, closed)`, its blend laid over it.
/// `style.color ?? undefined`: a `null` color strokes nothing.
fn stroke_color(style: &Style) -> Option<Value> {
    style.color.clone().filter(|c| !c.is_null())
}

/// `HAIRLINE_KINDS`: the kinds a width of 0 draws as a hairline.
const HAIRLINE_KINDS: [&str; 3] = ["line", "polyline", "ink"];

fn shape_paint(style: &Style, closed: bool) -> Paint {
    let cloudy = style.cloudy_intensity.as_option().is_some_and(|i| *i != 0.0 && !i.is_nan());
    let mut keys = vec!["fill", "stroke", "width", "opacity", "dash"];
    if cloudy {
        keys.push("join");
    }
    keys.push("blend");
    Paint {
        fill: if closed { style.interior_color.as_option().map(|c| s(c)) } else { None },
        stroke: stroke_color(style),
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
    // One segment per `half` (at least 2) units. A hostile /QuadPoints width
    // would otherwise grow the string without bound, or past 2^53 never
    // advance `position`. Every real quad is a few thousand units at most,
    // so the limit (the same budget as a cloudy border's) never engages
    // there; the TypeScript has none (review finding M20, kept).
    if length / half > crate::cloudy::MAX_CLOUDY_CURLS {
        return String::new();
    }
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
        stroke: stroke_color(style),
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
                    fill: stroke_color(style),
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

/// The size a repeated label draws at when its `fontSize` is 0 (fit): fitting
/// would only shrink the label as it repeats, so the engine sets this one.
const REPEAT_FIT_FONT_SIZE: f64 = 12.0;

/// How many times a repeated label's text doubles at most, as in the engine:
/// 1,024 labels.
const MAX_REPEAT_DOUBLINGS: usize = 10;

/// `QUADDING[textAlign ?? 'left']`: `/Q` by the label's alignment. An
/// alignment the table lacks is `undefined`, which the layout places as left.
fn quadding(align: Option<&str>) -> Alignment {
    match align {
        Some("center") => Alignment::Center,
        Some("right") => Alignment::Right,
        _ => Alignment::Left,
    }
}

/// `redactLabelText(size, label, text, fonts)`: a redaction label laid out in
/// a region `width` × `height`, as the engine lays it out when it writes the
/// mark's overlay: a form field's text in the region (PDF points, y up from
/// its bottom), top-aligned, placed across by `/Q`. A label shown once at
/// size 0 fits the region on one line; one at a set size, or a repeated one,
/// wraps to it. A repeated label's text is the label space-joined to itself,
/// doubled until its lines reach the region's bottom. `None` for no text.
pub fn redact_label_text(
    width: f64,
    height: f64,
    label: &RedactLabel,
    text: Option<&TextStyle>,
    fonts: &PaintFonts,
) -> Option<FieldText> {
    if label.text.is_empty() {
        return None;
    }
    let repeat = json::truthy(label.repeat.as_ref());
    // `text?.fontSize ?? 0`.
    let set_size = text
        .and_then(|t| t.font_size.as_ref())
        .filter(|size| !json::nullish(Some(size)))
        .map_or(0.0, |size| json::number(size).unwrap_or(f64::NAN));
    let fitted = set_size.abs() < 0.0001;
    let options = FieldTextOptions {
        plate: PdfRect { left: 0.0, bottom: 0.0, right: width, top: height },
        font_size: if repeat && fitted { REPEAT_FIT_FONT_SIZE } else { set_size },
        alignment: quadding(text.and_then(TextStyle::align)),
        multiline: repeat || !fitted,
        password: false,
        limit: 0,
        cells: 0,
        centered: false,
    };
    let mut shown = label.text.clone();
    let mut laid = layout_field(&shown, fonts, &options);
    let mut doubling = 0;
    while repeat && doubling < MAX_REPEAT_DOUBLINGS && laid.content_height < height {
        shown = format!("{shown} {shown}");
        laid = layout_field(&shown, fonts, &options);
        doubling += 1;
    }
    Some(laid)
}

/// `layoutRedactLabel(region, label, text)`: a redaction label's preview in
/// `region` (page space), the runs [`redact_label_text`] lays out in the
/// fonts the label carries (its standard font, or Helvetica, without them),
/// clipped to the region.
pub fn layout_redact_label(
    region: Rect,
    label: &RedactLabel,
    text: Option<&TextStyle>,
) -> Vec<SceneNode> {
    let fonts = label.fonts.0.clone().unwrap_or_else(|| {
        default_paint_fonts(text.and_then(|t| t.font_family.as_ref()).and_then(Value::as_str))
    });
    let Some(laid) = redact_label_text(region.width, region.height, label, text, &fonts) else {
        return Vec::new();
    };
    let fill = text
        .and_then(|t| t.font_color.clone())
        .filter(|c| !json::nullish(Some(c)))
        .unwrap_or(s("#ffffff"));
    let paint = Paint { fill: Some(fill), opacity: Some(json::num(1.0)), ..Paint::default() }
        .ordered(&["fill", "opacity"]);
    let mut nodes = Vec::new();
    for line in &laid.lines {
        for run in &line.runs {
            nodes.push(SceneNode::TextRun(TextRunScene {
                at: Point {
                    x: num(region.x + run.x),
                    y: num(region.y + region.height - line.baseline),
                },
                text: run.text.clone(),
                rtl: run.rtl,
                plain: run.plain,
                joins_previous: run.joins_previous.then_some(true),
                joins_next: run.joins_next.then_some(true),
                font_size: laid.font_size,
                font_family: run.font.font_family.clone(),
                font_weight: Some(f64::from(run.font.font_weight)),
                font_style: Some(
                    match run.font.font_style {
                        epdf_fonts::FontStyle::Normal => "normal",
                        epdf_fonts::FontStyle::Italic => "italic",
                    }
                    .into(),
                ),
                rotation: None,
                program: Some(FontProgramRef(run.font.program.clone())),
                paint: paint.clone(),
                clip: Some(region),
                order: Default::default(),
            }));
        }
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
            stroke: stroke_color(&item.style),
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
            fill: stroke_color(&item.style),
            stroke: stroke_color(&item.style),
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
    // A line, polyline or ink at width 0 is a hairline, as the engine and
    // Acrobat draw it; a square, circle or polygon at width 0 has no outline.
    let hairline =
        item.style.stroke_width == 0.0 && HAIRLINE_KINDS.contains(&item.subtype.as_str());
    let mut nodes: Vec<SceneNode> = geom_scene(&item.geometry, item.style.stroke())
        .iter()
        .map(|node| {
            let closed = match node {
                RenderNode::Rect(_) | RenderNode::Ellipse(_) => true,
                RenderNode::Path(_) => !ink,
                RenderNode::Poly(p) => p.closed,
                RenderNode::Line(_) => false,
            };
            let mut paint = shape_paint(&item.style, closed);
            let mut keys = vec!["fill", "stroke", "width", "opacity", "dash", "join", "blend"];
            if hairline {
                paint.hairline = Some(true);
                keys.push("hairline");
            }
            // Ink is drawn as Acrobat draws it: round caps and no join, and a
            // stroke of one point is a dot filled in the stroke's colour. The
            // stroke the pen is still drawing is its points (a `poly`), joined
            // round as a pen lays them.
            if ink {
                paint.fill = if closed { stroke_color(&item.style) } else { None };
                paint.line_cap = Some("round".into());
                keys.push("lineCap");
                if matches!(node, RenderNode::Poly(_)) {
                    if paint.join.is_none() {
                        // A spread adds `join` last when the paint had none.
                        keys.retain(|k| *k != "join");
                        keys.push("join");
                    }
                    paint.join = Some("round".into());
                }
            }
            SceneNode::painted(node, paint.ordered(&keys))
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

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: Point = Point { x: 0.0, y: 0.0 };
    const RIGHT: Point = Point { x: 1.0, y: 0.0 };
    const UP: Point = Point { x: 0.0, y: 1.0 };

    #[test]
    fn a_normal_squiggle_draws() {
        let d = squiggle_path(ORIGIN, RIGHT, UP, 600.0, 1.5);
        assert!(d.starts_with("M 0 0 Q") && d.contains(" T "));
    }

    #[test]
    fn an_absurd_squiggle_draws_nothing() {
        for w in [1e12, 1e17, f64::INFINITY] {
            assert_eq!(squiggle_path(ORIGIN, RIGHT, UP, w, 1.5), "", "{w}");
        }
    }
}
