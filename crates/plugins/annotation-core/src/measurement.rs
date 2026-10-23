//! `measurement.ts` and `measurement-font.ts`: a distance measurement's
//! layout (its dimension line, leaders, caption and endings), what it draws,
//! where it is grabbed, and the fields a new measurement states.

use serde_json::Value;

use crate::endings::{ending_nodes, ending_pieces, ending_points};
use crate::engine::measure::{DimensionInput, measurement_readout};
use crate::geometry::{geom_rotation, selection_quad};
use crate::helvetica::helvetica_advance;
use crate::js;
use crate::json::{self, Object, num};
use crate::kinds::dash_of;
use crate::object::{Json, JsonError};
use crate::painted::{PaintedPiece, body_pieces};
use crate::rect::{rotate_point, union_rect};
use crate::shapes::points::{drawn_line_of, stroked_outline_of};
use crate::types::{
    Handle, Paint, Point, Rect, RenderNode, SceneNode, Shape, Stroke, Style, TextScene,
};

/// A measurement's appearance (`MeasurementAppearance`): the engine's own
/// fields for its scale, caption, leader and label, read off a dimension
/// annotation. Kept as the object the TypeScript builds.
#[derive(Clone, Debug, PartialEq)]
pub struct MeasurementAppearance(pub Object);

impl Json for MeasurementAppearance {
    fn from_json(value: Option<&Value>) -> Result<Self, JsonError> {
        Object::from_json(value).map(MeasurementAppearance)
    }
    fn to_json(&self) -> Option<Value> {
        Some(Value::Object(self.0.clone()))
    }
}

impl MeasurementAppearance {
    pub fn intent(&self) -> &str {
        json::get_str(&self.0, "intent").unwrap_or("")
    }
    /// A distance (`line-dimension`), rather than a perimeter or an area.
    pub fn is_distance(&self) -> bool {
        self.intent() == "line-dimension"
    }
    pub fn measure(&self) -> Option<&Value> {
        json::get(&self.0, "measure")
    }
    pub fn caption_enabled(&self) -> bool {
        json::truthy(self.0.get("captionEnabled"))
    }
    pub fn caption_position(&self) -> Option<&str> {
        json::get_str(&self.0, "captionPosition")
    }
    pub fn contents(&self) -> Option<&str> {
        json::get(&self.0, "contents").and_then(Value::as_str)
    }
    /// `captionOffset`, `None` when absent or null.
    pub fn caption_offset(&self) -> Option<(f64, f64)> {
        let offset = json::get(&self.0, "captionOffset")?;
        Some((
            offset.get("along").and_then(json::number).unwrap_or(f64::NAN),
            offset.get("perpendicular").and_then(json::number).unwrap_or(f64::NAN),
        ))
    }
    /// A member of `leader`, `None` when the leader or the member is absent.
    pub fn leader_member(&self, name: &str) -> Option<f64> {
        json::get(&self.0, "leader")
            .and_then(|l| l.get(name))
            .filter(|v| !json::nullish(Some(v)))
            .and_then(json::number)
    }
    pub fn leader_length(&self) -> f64 {
        self.leader_member("length").unwrap_or(0.0)
    }
    /// `{ ...appearance, leader: { ...appearance.leader, length } }`.
    pub fn with_leader_length(&self, length: f64) -> MeasurementAppearance {
        let mut leader =
            json::get(&self.0, "leader").and_then(Value::as_object).cloned().unwrap_or_default();
        leader.insert("length".into(), num(length));
        let mut out = self.0.clone();
        out.insert("leader".into(), Value::Object(leader));
        MeasurementAppearance(out)
    }
}

/// `measurementOf(annotation)`: the annotation's measurement, when it is one.
pub fn measurement_of(annotation: &Object) -> Option<MeasurementAppearance> {
    let subtype = json::get_str(annotation, "subtype")?;
    let intent = json::get_str(annotation, "intent");
    let pick = |names: &[&str]| -> Object {
        let mut out = Object::new();
        for name in names {
            if let Some(v) = annotation.get(*name).filter(|v| !json::undefined(Some(v))) {
                out.insert((*name).into(), v.clone());
            }
        }
        out
    };
    if subtype == "line" && intent == Some("line-dimension") {
        return Some(MeasurementAppearance(pick(&[
            "intent",
            "measure",
            "captionEnabled",
            "captionPosition",
            "captionOffset",
            "leader",
            "contents",
        ])));
    }
    if (subtype == "polyline" || subtype == "polygon")
        && matches!(intent, Some("polyline-dimension" | "polygon-dimension"))
    {
        return Some(MeasurementAppearance(pick(&[
            "intent",
            "measure",
            "captionEnabled",
            "contents",
        ])));
    }
    None
}

/// `measurementDraftFields(appearance)`: what a new measurement states beside its points.
pub fn measurement_draft_fields(appearance: &MeasurementAppearance) -> Object {
    let a = &appearance.0;
    let mut out = json::object([("intent", a.get("intent").cloned().unwrap_or(Value::Null))]);
    let measure = match json::get(a, "measure") {
        Some(m) if m.get("subtype").and_then(Value::as_str) == Some("rectilinear") => m.clone(),
        _ => Value::Null,
    };
    out.insert("measure".into(), measure);
    let caption: &[&str] = if appearance.is_distance() {
        &["captionEnabled", "captionPosition", "captionOffset", "leader"]
    } else {
        &["captionEnabled"]
    };
    for name in caption {
        if let Some(v) = a.get(*name).filter(|v| !json::undefined(Some(v))) {
            out.insert((*name).into(), v.clone());
        }
    }
    let subject = match appearance.intent() {
        "line-dimension" => "Distance",
        "polyline-dimension" => "Perimeter",
        _ => "Area",
    };
    out.insert("subject".into(), Value::String(subject.into()));
    out
}

// ── measurement-font.ts ─────────────────────────────────────────────────────

pub const DISTANCE_CAPTION_SIZE: f64 = 9.0;

/// `distanceCaptionWidth(text)`.
pub fn distance_caption_width(text: &str) -> f64 {
    helvetica_advance(text.trim_end_matches(is_js_trailing_space)) * DISTANCE_CAPTION_SIZE
}

/// `String.prototype.trimEnd`'s whitespace.
fn is_js_trailing_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

// ── layout ──────────────────────────────────────────────────────────────────

/// A dimension, leader or connector segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceSegment {
    pub from: Point,
    pub to: Point,
}

/// A measurement's caption (`DistanceCaptionLayout`).
#[derive(Clone, Debug, PartialEq)]
pub struct CaptionLayout {
    pub text: String,
    pub center: Point,
    pub along: Point,
    pub normal: Point,
    pub width: f64,
    pub height: f64,
    pub bounds: [Point; 4],
}

/// One layout drives a distance's preview, selection, handles and hit testing.
#[derive(Clone, Debug, PartialEq)]
pub struct DistanceLayout {
    pub along: Point,
    pub normal: Point,
    pub length: f64,
    pub measured_start: Point,
    pub measured_end: Point,
    pub dimension_start: Point,
    pub dimension_end: Point,
    pub dimension_segments: Vec<DistanceSegment>,
    pub leader_segments: Vec<DistanceSegment>,
    pub caption_connector: Vec<DistanceSegment>,
    pub caption: Option<CaptionLayout>,
    /// `inside` or `outside`.
    pub arrow_placement: &'static str,
    pub endings: Vec<RenderNode>,
    pub selection_points: Vec<Point>,
    pub visual_bounds: Rect,
}

const CAPTION_PADDING: f64 = 2.0;
const OUTSIDE_CAPTION_PADDING: f64 = 7.0;
const ARROW_RESERVE: f64 = 24.0;
const OUTSIDE_STUB_LENGTH: f64 = 20.0;

fn offset_point(p: Point, direction: Point, distance: f64) -> Point {
    Point { x: p.x + direction.x * distance, y: p.y + direction.y * distance }
}

fn project_delta(from: Point, to: Point, direction: Point) -> f64 {
    (to.x - from.x) * direction.x + (to.y - from.y) * direction.y
}

fn line_axes(start: Point, end: Point) -> (Point, Point, f64) {
    let length = js::hypot(end.x - start.x, end.y - start.y);
    let along = if length > 0.0 {
        Point { x: (end.x - start.x) / length, y: (end.y - start.y) / length }
    } else {
        Point { x: 1.0, y: 0.0 }
    };
    (along, Point { x: along.y, y: -along.x }, length)
}

/// `distanceLabel(geometry, appearance)`.
pub fn distance_label(geometry: &Shape, appearance: &MeasurementAppearance) -> String {
    let Shape::Line(line) = geometry else { return appearance.contents().unwrap_or("").into() };
    let input = DimensionInput {
        subtype: "line",
        intent: Some(appearance.intent()),
        measure: appearance.measure(),
        line_points: Some((line.line_points.start, line.line_points.end)),
        vertices: Vec::new(),
    };
    match measurement_readout(&input).label() {
        Some(label) => label.into(),
        None => appearance.contents().unwrap_or("").into(),
    }
}

fn caption_quad(center: Point, along: Point, normal: Point, width: f64, height: f64) -> [Point; 4] {
    let corner = |x: f64, y: f64| offset_point(offset_point(center, along, x), normal, y);
    [
        corner(-width / 2.0, -height / 2.0),
        corner(width / 2.0, -height / 2.0),
        corner(width / 2.0, height / 2.0),
        corner(-width / 2.0, height / 2.0),
    ]
}

fn caption_connector(
    midpoint: Point,
    caption: &CaptionLayout,
    along: Point,
    normal: Point,
    offset: Option<(f64, f64)>,
) -> Vec<DistanceSegment> {
    match offset {
        None => return Vec::new(),
        Some((a, p)) if a == 0.0 && p == 0.0 => return Vec::new(),
        _ => {}
    }
    let perpendicular = project_delta(midpoint, caption.center, normal);
    let horizontal = project_delta(midpoint, caption.center, along);
    let half_width = caption.width / 2.0 + CAPTION_PADDING;
    let half_height = caption.height / 2.0 + CAPTION_PADDING;
    if perpendicular.abs() <= half_height {
        return Vec::new();
    }
    if horizontal.abs() <= half_width {
        let to =
            offset_point(midpoint, normal, perpendicular - js::sign(perpendicular) * half_height);
        return vec![DistanceSegment { from: midpoint, to }];
    }
    let knee = offset_point(midpoint, normal, perpendicular);
    let to = offset_point(caption.center, along, -js::sign(horizontal) * half_width);
    vec![DistanceSegment { from: midpoint, to: knee }, DistanceSegment { from: knee, to }]
}

#[allow(clippy::too_many_arguments)]
fn dimension_segments(
    start: Point,
    end: Point,
    along: Point,
    normal: Point,
    length: f64,
    caption: Option<&CaptionLayout>,
    position: Option<&str>,
    outside: bool,
    stroke_width: f64,
) -> Vec<DistanceSegment> {
    if outside {
        let stub = OUTSIDE_STUB_LENGTH * stroke_width;
        return vec![
            DistanceSegment { from: offset_point(start, along, -stub), to: start },
            DistanceSegment { from: end, to: offset_point(end, along, stub) },
        ];
    }
    let Some(caption) = caption.filter(|_| position != Some("top")) else {
        return vec![DistanceSegment { from: start, to: end }];
    };
    let perpendicular = project_delta(start, caption.center, normal);
    if perpendicular.abs() >= caption.height / 2.0 + CAPTION_PADDING {
        return vec![DistanceSegment { from: start, to: end }];
    }
    let at = project_delta(start, caption.center, along);
    let half_gap = caption.width / 2.0 + CAPTION_PADDING;
    let gap_start = js::max(0.0, js::min(length, at - half_gap));
    let gap_end = js::max(0.0, js::min(length, at + half_gap));
    let mut segments = Vec::new();
    if gap_start > 0.0 {
        segments.push(DistanceSegment { from: start, to: offset_point(start, along, gap_start) });
    }
    if gap_end < length {
        segments.push(DistanceSegment { from: offset_point(start, along, gap_end), to: end });
    }
    segments
}

/// `distanceLayout(geometry, appearance, strokeWidth)`.
pub fn distance_layout(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    stroke_width: f64,
) -> Option<DistanceLayout> {
    let Shape::Line(line) = geometry else { return None };
    let (a, b) = drawn_line_of(geometry);
    let (along, normal, length) = line_axes(a, b);
    let leader_length = appearance.leader_length();
    let dimension_start = offset_point(a, normal, leader_length);
    let dimension_end = offset_point(b, normal, leader_length);
    let midpoint = offset_point(dimension_start, along, length / 2.0);
    let text = distance_label(geometry, appearance);
    let width = distance_caption_width(&text);
    let has_caption = appearance.caption_enabled() && !text.is_empty();
    let outside =
        has_caption && width + 2.0 * CAPTION_PADDING + ARROW_RESERVE * stroke_width > length;

    let caption = has_caption.then(|| {
        let reversed = along.x < 0.0 || (along.x == 0.0 && along.y > 0.0);
        let text_along = if reversed { Point { x: -along.x, y: -along.y } } else { along };
        let text_normal = Point { x: text_along.y, y: -text_along.x };
        let offset = appearance.caption_offset();
        let mut center = offset_point(midpoint, along, offset.map_or(0.0, |o| o.0));
        center = offset_point(center, normal, offset.map_or(0.0, |o| o.1));
        if appearance.caption_position() == Some("top") || outside {
            let side = if outside && leader_length < 0.0 { -1.0 } else { 1.0 };
            let padding = if outside { OUTSIDE_CAPTION_PADDING } else { CAPTION_PADDING };
            center = offset_point(center, normal, side * (DISTANCE_CAPTION_SIZE / 2.0 + padding));
        }
        CaptionLayout {
            text: text.clone(),
            center,
            along: text_along,
            normal: text_normal,
            width,
            height: DISTANCE_CAPTION_SIZE,
            bounds: caption_quad(center, text_along, text_normal, width, DISTANCE_CAPTION_SIZE),
        }
    });

    let mut leader_segments = Vec::new();
    if leader_length != 0.0 {
        let side = js::sign(leader_length);
        let leader_offset = side * appearance.leader_member("offset").unwrap_or(0.0);
        let leader_end =
            leader_length + side * appearance.leader_member("extension").unwrap_or(0.0);
        for endpoint in [a, b] {
            leader_segments.push(DistanceSegment {
                from: offset_point(endpoint, normal, leader_offset),
                to: offset_point(endpoint, normal, leader_end),
            });
        }
    }

    let segments = dimension_segments(
        dimension_start,
        dimension_end,
        along,
        normal,
        length,
        caption.as_ref(),
        appearance.caption_position(),
        outside,
        stroke_width,
    );
    let connector = match &caption {
        Some(c) => caption_connector(midpoint, c, along, normal, appearance.caption_offset()),
        None => Vec::new(),
    };

    let angle = along.y.atan2(along.x);
    let start_angle = if outside { angle } else { angle + std::f64::consts::PI };
    let end_angle = if outside { angle + std::f64::consts::PI } else { angle };
    let endings_of = line.line_endings.as_ref();
    let start_ending = endings_of.and_then(|e| e.start.as_deref());
    let end_ending = endings_of.and_then(|e| e.end.as_deref());
    let mut endings = ending_nodes(dimension_start, start_angle, start_ending, stroke_width);
    endings.extend(ending_nodes(dimension_end, end_angle, end_ending, stroke_width));

    let mut points = vec![a, b];
    for s in segments.iter().chain(leader_segments.iter()).chain(connector.iter()) {
        points.push(s.from);
        points.push(s.to);
    }
    points.extend(ending_points(dimension_start, start_angle, start_ending, stroke_width));
    points.extend(ending_points(dimension_end, end_angle, end_ending, stroke_width));
    if let Some(c) = &caption {
        points.extend(c.bounds.iter().copied());
    }
    let visual_bounds = expand_distance_bounds(union_rect(&points), stroke_width / 2.0 + 1.0);
    Some(DistanceLayout {
        along,
        normal,
        length,
        measured_start: a,
        measured_end: b,
        dimension_start,
        dimension_end,
        dimension_segments: segments,
        leader_segments,
        caption_connector: connector,
        caption,
        arrow_placement: if outside { "outside" } else { "inside" },
        endings,
        selection_points: points,
        visual_bounds,
    })
}

/// `expandDistanceBounds(bounds, padding)`.
pub fn expand_distance_bounds(bounds: Rect, padding: f64) -> Rect {
    Rect {
        x: bounds.x - padding,
        y: bounds.y - padding,
        width: bounds.width + 2.0 * padding,
        height: bounds.height + 2.0 * padding,
    }
}

/// `distanceSelectionQuad(geometry, appearance, strokeWidth)`.
pub fn distance_selection_quad(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    stroke_width: f64,
) -> [Point; 4] {
    let layout = distance_layout(geometry, appearance, stroke_width);
    let Some(layout) = layout.filter(|_| matches!(geometry, Shape::Line(_))) else {
        return selection_quad(geometry, Stroke::width(stroke_width));
    };
    let angle = geom_rotation(geometry);
    let origin = drawn_line_of(geometry).0;
    let points: Vec<Point> =
        layout.selection_points.iter().map(|p| rotate_point(*p, origin, -angle)).collect();
    let bounds = expand_distance_bounds(union_rect(&points), stroke_width / 2.0 + 1.0);
    let corner = |x: f64, y: f64| rotate_point(Point { x, y }, origin, angle);
    [
        corner(bounds.x, bounds.y),
        corner(bounds.x + bounds.width, bounds.y),
        corner(bounds.x + bounds.width, bounds.y + bounds.height),
        corner(bounds.x, bounds.y + bounds.height),
    ]
}

/// `distanceHandles(layout)`.
pub fn distance_handles(layout: &DistanceLayout) -> Vec<Handle> {
    vec![
        Handle::new("v0", layout.measured_start, "crosshair"),
        Handle::new("v1", layout.measured_end, "crosshair"),
        Handle::new("leader-start", layout.dimension_start, "move"),
        Handle::new("leader-end", layout.dimension_end, "move"),
    ]
}

/// `distanceCaptionHit(layout, point, margin)`.
pub fn distance_caption_hit(caption: Option<&CaptionLayout>, point: Point, margin: f64) -> bool {
    let Some(caption) = caption else { return false };
    let along = project_delta(caption.center, point, caption.along);
    let perpendicular = project_delta(caption.center, point, caption.normal);
    along.abs() <= caption.width / 2.0 + margin
        && perpendicular.abs() <= caption.height / 2.0 + margin
}

/// `captionPainted(caption)`.
pub fn caption_painted(caption: Option<&CaptionLayout>) -> Vec<PaintedPiece> {
    caption.map(|c| body_pieces(&c.bounds)).unwrap_or_default()
}

fn segments_of(layout: &DistanceLayout) -> Vec<DistanceSegment> {
    layout
        .dimension_segments
        .iter()
        .chain(layout.leader_segments.iter())
        .chain(layout.caption_connector.iter())
        .copied()
        .collect()
}

/// `distancePainted(layout, strokeWidth)`.
pub fn distance_painted(layout: &DistanceLayout, stroke_width: f64) -> Vec<PaintedPiece> {
    let half_width = stroke_width / 2.0;
    let mut out = caption_painted(layout.caption.as_ref());
    for s in segments_of(layout) {
        out.push(PaintedPiece::Stroke { points: vec![s.from, s.to], closed: false, half_width });
    }
    out.extend(ending_pieces(&layout.endings, stroke_width));
    out
}

/// `distanceCaptionAt(geometry, appearance, width)`.
pub fn distance_caption_at(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    width: f64,
) -> Option<Point> {
    distance_layout(geometry, appearance, width)?.caption.map(|c| c.center)
}

/// `distanceLeaderLength(geometry, point)`.
pub fn distance_leader_length(geometry: &Shape, point: Point) -> f64 {
    if !matches!(geometry, Shape::Line(_)) {
        return 0.0;
    }
    let (start, end) = drawn_line_of(geometry);
    let (_, normal, _) = line_axes(start, end);
    project_delta(start, point, normal)
}

/// `moveDistanceCaption(geometry, appearance, delta)`.
pub fn move_distance_caption(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    delta: Point,
) -> MeasurementAppearance {
    if !matches!(geometry, Shape::Line(_)) {
        return appearance.clone();
    }
    let (start, end) = drawn_line_of(geometry);
    let (along, normal, _) = line_axes(start, end);
    let previous = appearance.caption_offset();
    let offset = json::object([
        ("along", num(previous.map_or(0.0, |p| p.0) + delta.x * along.x + delta.y * along.y)),
        (
            "perpendicular",
            num(previous.map_or(0.0, |p| p.1) + delta.x * normal.x + delta.y * normal.y),
        ),
    ]);
    let mut out = appearance.0.clone();
    out.insert("captionOffset".into(), Value::Object(offset));
    MeasurementAppearance(out)
}

/// `distanceDrawnBounds(layout, strokeWidth)`.
pub fn distance_drawn_bounds(layout: &DistanceLayout, stroke_width: f64) -> Rect {
    let mut nodes: Vec<RenderNode> =
        segments_of(layout).iter().map(|s| RenderNode::line(s.from, s.to)).collect();
    nodes.extend(layout.endings.iter().cloned());
    let mut points = stroked_outline_of(&nodes, stroke_width);
    if let Some(c) = &layout.caption {
        points.extend(c.bounds.iter().copied());
    }
    union_rect(&points)
}

/// The paint a distance's lines and endings take.
fn line_paint(style: &Style) -> Paint {
    Paint {
        stroke: style.color.clone(),
        width: Some(style.stroke_width),
        opacity: style.opacity.clone(),
        dash: dash_of(style),
        ..Paint::default()
    }
    .ordered(&["stroke", "width", "opacity", "dash"])
}

/// `distanceScene(geometry, appearance, style)`.
pub fn distance_scene(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> Vec<SceneNode> {
    let Some(layout) = distance_layout(geometry, appearance, style.stroke_width) else {
        return Vec::new();
    };
    let paint = line_paint(style);
    let mut nodes: Vec<SceneNode> = segments_of(&layout)
        .iter()
        .map(|s| SceneNode::painted(&RenderNode::line(s.from, s.to), paint.clone()))
        .collect();
    for ending in &layout.endings {
        let filled = match ending {
            RenderNode::Ellipse(_) => true,
            RenderNode::Poly(p) => p.closed,
            _ => false,
        };
        // `{ ...paint, fill }`: the fill, when there is one, prints last.
        let mut ending_paint = paint.clone();
        ending_paint.fill = if filled {
            style.interior_color.as_option().map(|c| Value::String(c.clone()))
        } else {
            None
        };
        nodes.push(SceneNode::painted(ending, ending_paint));
    }
    nodes.extend(measurement_caption_scene(layout.caption.as_ref(), style));
    nodes
}

/// `measurementCaptionScene(caption, style)`.
pub fn measurement_caption_scene(caption: Option<&CaptionLayout>, style: &Style) -> Vec<SceneNode> {
    let Some(caption) = caption else { return Vec::new() };
    let baseline = offset_point(caption.center, caption.normal, -DISTANCE_CAPTION_SIZE * 0.33);
    let at = offset_point(baseline, caption.along, -caption.width / 2.0);
    vec![SceneNode::Text(TextScene {
        at,
        text: caption.text.clone(),
        font_size: DISTANCE_CAPTION_SIZE,
        font_family: Some("Helvetica, Arial, sans-serif".into()),
        rotation: Some((caption.along.y.atan2(caption.along.x) * 180.0) / std::f64::consts::PI),
        paint: Paint {
            fill: Some(Value::String("#000000".into())),
            opacity: style.opacity.clone(),
            ..Paint::default()
        }
        .ordered(&["fill", "opacity"]),
        clip: None,
        order: Default::default(),
    })]
}
