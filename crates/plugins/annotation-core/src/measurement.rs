//! `measurement.ts` and `measurement-font.ts`: the distance annotation's
//! layout (dimension line, leaders, caption) and the one layout's uses:
//! preview scene, selection, handles and hit testing.

use serde::{Deserialize, Serialize};

use crate::endings::{ending_nodes, ending_points};
use crate::geometry::{geom_hit, geom_rotation, rotate_point, selection_quad, union_rect};
use crate::js;
use crate::measure::{DimensionInput, PdfMeasurement, measurement_readout};
use crate::measurement_shape::ShapeMeasurementAppearance;
use crate::types::{
    Border, Geom, Handle, Paint, PdfPoint, PdfRect, Point, Quad, Rect, RenderNode, SceneNode, Style,
};

// ── the appearance ──────────────────────────────────────────────────────────

/// `'LineDimension'`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceIntent {
    #[default]
    LineDimension,
}

/// `LineDimensionCaption['position']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptionPosition {
    Inline,
    Top,
}

/// A caption displacement in directed line axes, PDF units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CaptionOffset {
    pub along: f64,
    pub perpendicular: f64,
}

/// `LineDimensionCaption`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineDimensionCaption {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<CaptionPosition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<CaptionOffset>,
}

/// `LineLeader`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LineLeader {
    pub length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

/// A distance annotation's render projection. Offsets stay in directed PDF
/// line axes; the crop keeps rounding in the PDF frame.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DistanceAppearance {
    pub intent: DistanceIntent,
    #[serde(default)]
    pub measure: Option<PdfMeasurement>,
    pub caption: LineDimensionCaption,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<LineLeader>,
    pub crop: PdfRect,
    pub text: String,
}

impl DistanceAppearance {
    /// `{ ...a, leader: { ...a.leader, length } }`.
    pub fn with_leader_length(&self, length: f64) -> DistanceAppearance {
        let leader = LineLeader { length, ..self.leader.unwrap_or_default() };
        DistanceAppearance { leader: Some(leader), ..self.clone() }
    }
}

/// `MeasurementAppearance = DistanceAppearance | ShapeMeasurementAppearance`,
/// told apart by `intent`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MeasurementAppearance {
    Distance(DistanceAppearance),
    Shape(ShapeMeasurementAppearance),
}

impl MeasurementAppearance {
    /// `intent === 'LineDimension'`.
    pub fn as_distance(&self) -> Option<&DistanceAppearance> {
        match self {
            MeasurementAppearance::Distance(d) => Some(d),
            MeasurementAppearance::Shape(_) => None,
        }
    }

    pub fn as_shape(&self) -> Option<&ShapeMeasurementAppearance> {
        match self {
            MeasurementAppearance::Shape(s) => Some(s),
            MeasurementAppearance::Distance(_) => None,
        }
    }

    /// `intent === 'PolygonDimension'`.
    pub fn is_polygon(&self) -> bool {
        self.as_shape().is_some_and(|s| s.is_polygon())
    }
}

// ── the layout ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DistanceSegment {
    pub from: Point,
    pub to: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DistanceCaptionLayout {
    pub text: String,
    pub center: Point,
    pub along: Point,
    pub normal: Point,
    pub width: f64,
    pub height: f64,
    pub bounds: Quad,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArrowPlacement {
    Inside,
    Outside,
}

/// One layout drives the preview, selection, handles and hit testing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    pub caption: Option<DistanceCaptionLayout>,
    pub arrow_placement: ArrowPlacement,
    pub endings: Vec<RenderNode>,
    pub selection_points: Vec<Point>,
    pub visual_bounds: Rect,
}

// ── measurement-font.ts ─────────────────────────────────────────────────────

/// The caption font size (points).
pub const DISTANCE_CAPTION_SIZE: f64 = 9.0;

/// Standard Helvetica advances in 1/1000 em, ASCII 32 to 126.
const HELVETICA_ADVANCES: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn extra_advance(c: char) -> f64 {
    match c {
        '²' | '³' => 333.0,
        '′' => 191.0,
        '″' => 355.0,
        '−' => 584.0,
        '\u{a0}' => 278.0,
        _ => 556.0,
    }
}

/// `String#trimEnd`: JavaScript's white space and line terminators.
fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(|c: char| {
        matches!(
            c,
            '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                ..='\u{200a}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{202f}'
                    | '\u{205f}'
                    | '\u{3000}'
                    | '\u{feff}'
        )
    })
}

/// The caption's width in points, from the Helvetica advances.
pub fn distance_caption_width(text: &str) -> f64 {
    let mut advance = 0.0;
    for c in js_trim_end(text).chars() {
        let code = c as u32;
        advance += match code.checked_sub(32).and_then(|i| HELVETICA_ADVANCES.get(i as usize)) {
            Some(a) => f64::from(*a),
            None => extra_advance(c),
        };
    }
    (advance * DISTANCE_CAPTION_SIZE) / 1000.0
}

// ── measurement.ts ──────────────────────────────────────────────────────────

const CAPTION_PADDING: f64 = 2.0;
const OUTSIDE_CAPTION_PADDING: f64 = 7.0;
const ARROW_RESERVE: f64 = 24.0;
const OUTSIDE_STUB_LENGTH: f64 = 20.0;

pub(crate) fn offset_point(point: Point, direction: Point, distance: f64) -> Point {
    Point { x: point.x + direction.x * distance, y: point.y + direction.y * distance }
}

fn project_delta(from: Point, to: Point, direction: Point) -> f64 {
    (to.x - from.x) * direction.x + (to.y - from.y) * direction.y
}

struct Axes {
    along: Point,
    normal: Point,
    length: f64,
}

fn line_axes(start: Point, end: Point) -> Axes {
    let length = js::hypot(end.x - start.x, end.y - start.y);
    let along = if length > 0.0 {
        Point { x: (end.x - start.x) / length, y: (end.y - start.y) / length }
    } else {
        Point { x: 1.0, y: 0.0 }
    };
    // PDF's +90° normal, in content coordinates (y down).
    let normal = Point { x: along.y, y: -along.x };
    Axes { along, normal, length }
}

/// The caption text: the readout's label for a line, else the stored text.
pub fn distance_label(geom: &Geom, appearance: &DistanceAppearance) -> String {
    let Geom::Line { a, b, .. } = geom else {
        return appearance.text.clone();
    };
    let crop = appearance.crop;
    let to_pdf = |p: Point| PdfPoint { x: p.x + crop.left, y: crop.top - p.y };
    let readout = measurement_readout(&DimensionInput {
        subtype: "line",
        intent: "LineDimension",
        measure: appearance.measure.as_ref(),
        line_points: Some((to_pdf(*a), to_pdf(*b))),
        vertices: Vec::new(),
    });
    readout.readout().map_or_else(|| appearance.text.clone(), |r| r.label.clone())
}

fn caption_quad(center: Point, along: Point, normal: Point, width: f64, height: f64) -> Quad {
    let corner = |x: f64, y: f64| offset_point(offset_point(center, along, x), normal, y);
    [
        corner(-width / 2.0, -height / 2.0),
        corner(width / 2.0, -height / 2.0),
        corner(width / 2.0, height / 2.0),
        corner(-width / 2.0, height / 2.0),
    ]
}

/// A displaced caption connects to the midpoint, stopping clear of its text.
fn caption_connector(
    midpoint: Point,
    caption: &DistanceCaptionLayout,
    along: Point,
    normal: Point,
    offset: Option<CaptionOffset>,
) -> Vec<DistanceSegment> {
    let Some(offset) = offset else {
        return Vec::new();
    };
    if offset.along == 0.0 && offset.perpendicular == 0.0 {
        return Vec::new();
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
    caption: Option<&DistanceCaptionLayout>,
    position: Option<CaptionPosition>,
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
    let Some(caption) = caption.filter(|_| position != Some(CaptionPosition::Top)) else {
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

fn segment_points(segments: &[DistanceSegment]) -> impl Iterator<Item = Point> + '_ {
    segments.iter().flat_map(|s| [s.from, s.to])
}

/// The distance layout of a line geom, `None` for any other.
pub fn distance_layout(
    geom: &Geom,
    appearance: &DistanceAppearance,
    stroke_width: f64,
) -> Option<DistanceLayout> {
    let Geom::Line { a, b, ends, .. } = geom else {
        return None;
    };
    let (a, b) = (*a, *b);
    let Axes { along, normal, length } = line_axes(a, b);
    let leader_length = appearance.leader.map_or(0.0, |l| l.length);
    let dimension_start = offset_point(a, normal, leader_length);
    let dimension_end = offset_point(b, normal, leader_length);
    let midpoint = offset_point(dimension_start, along, length / 2.0);
    let text = distance_label(geom, appearance);
    let width = distance_caption_width(&text);
    let has_caption = appearance.caption.enabled && !text.is_empty();
    let outside =
        has_caption && width + 2.0 * CAPTION_PADDING + ARROW_RESERVE * stroke_width > length;

    let caption = has_caption.then(|| {
        let reversed = along.x < 0.0 || (along.x == 0.0 && along.y > 0.0);
        let text_along = if reversed { Point { x: -along.x, y: -along.y } } else { along };
        let text_normal = Point { x: text_along.y, y: -text_along.x };
        let offset = appearance.caption.offset;
        let mut center = offset_point(midpoint, along, offset.map_or(0.0, |o| o.along));
        center = offset_point(center, normal, offset.map_or(0.0, |o| o.perpendicular));
        if appearance.caption.position == Some(CaptionPosition::Top) || outside {
            let side = if outside && leader_length < 0.0 { -1.0 } else { 1.0 };
            let padding = if outside { OUTSIDE_CAPTION_PADDING } else { CAPTION_PADDING };
            center = offset_point(center, normal, side * (DISTANCE_CAPTION_SIZE / 2.0 + padding));
        }
        DistanceCaptionLayout {
            bounds: caption_quad(center, text_along, text_normal, width, DISTANCE_CAPTION_SIZE),
            text,
            center,
            along: text_along,
            normal: text_normal,
            width,
            height: DISTANCE_CAPTION_SIZE,
        }
    });

    let mut leader_segments = Vec::new();
    if leader_length != 0.0 {
        let side = js::sign(leader_length);
        let leader = appearance.leader.unwrap_or_default();
        let leader_offset = side * leader.offset.unwrap_or(0.0);
        let leader_end = leader_length + side * leader.extension.unwrap_or(0.0);
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
        appearance.caption.position,
        outside,
        stroke_width,
    );
    let connector = caption.as_ref().map_or_else(Vec::new, |c| {
        caption_connector(midpoint, c, along, normal, appearance.caption.offset)
    });

    let angle = along.y.atan2(along.x);
    let pi = std::f64::consts::PI;
    let start_angle = if outside { angle } else { angle + pi };
    let end_angle = if outside { angle + pi } else { angle };
    let start_ending = ends.map(|e| e.start);
    let end_ending = ends.map(|e| e.end);
    let mut endings = ending_nodes(dimension_start, start_angle, start_ending, stroke_width);
    endings.extend(ending_nodes(dimension_end, end_angle, end_ending, stroke_width));
    let mut bounds_points = vec![a, b];
    bounds_points.extend(segment_points(&segments));
    bounds_points.extend(segment_points(&leader_segments));
    bounds_points.extend(segment_points(&connector));
    bounds_points.extend(ending_points(dimension_start, start_angle, start_ending, stroke_width));
    bounds_points.extend(ending_points(dimension_end, end_angle, end_ending, stroke_width));
    if let Some(c) = &caption {
        bounds_points.extend(c.bounds);
    }
    let visual_bounds =
        expand_distance_bounds(union_rect(&bounds_points), stroke_width / 2.0 + 1.0);

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
        arrow_placement: if outside { ArrowPlacement::Outside } else { ArrowPlacement::Inside },
        endings,
        selection_points: bounds_points,
        visual_bounds,
    })
}

/// A rect grown by `padding` on every side.
pub fn expand_distance_bounds(bounds: Rect, padding: f64) -> Rect {
    Rect {
        x: bounds.x - padding,
        y: bounds.y - padding,
        width: bounds.width + 2.0 * padding,
        height: bounds.height + 2.0 * padding,
    }
}

/// The oriented selection quad of a distance annotation: every component,
/// boxed in the line's advisory rotation frame about its start.
pub fn distance_selection_quad(
    geom: &Geom,
    appearance: &DistanceAppearance,
    stroke_width: f64,
) -> Quad {
    let (Some(layout), Geom::Line { a: origin, .. }) =
        (distance_layout(geom, appearance, stroke_width), geom)
    else {
        return selection_quad(geom, stroke_width, None);
    };
    let angle = geom_rotation(geom);
    let points: Vec<Point> =
        layout.selection_points.iter().map(|p| rotate_point(*p, *origin, -angle)).collect();
    let bounds = expand_distance_bounds(union_rect(&points), stroke_width / 2.0 + 1.0);
    let corner = |x: f64, y: f64| rotate_point(Point { x, y }, *origin, angle);
    [
        corner(bounds.x, bounds.y),
        corner(bounds.x + bounds.width, bounds.y),
        corner(bounds.x + bounds.width, bounds.y + bounds.height),
        corner(bounds.x, bounds.y + bounds.height),
    ]
}

/// The endpoint and leader handles.
pub fn distance_handles(layout: &DistanceLayout) -> Vec<Handle> {
    let h = |id: &str, at: Point, cursor: &str| Handle { id: id.into(), at, cursor: cursor.into() };
    vec![
        h("v0", layout.measured_start, "crosshair"),
        h("v1", layout.measured_end, "crosshair"),
        h("leader-start", layout.dimension_start, "move"),
        h("leader-end", layout.dimension_end, "move"),
    ]
}

/// Is the point on the caption box, grown by `margin`?
pub fn distance_caption_hit(
    caption: Option<&DistanceCaptionLayout>,
    point: Point,
    margin: f64,
) -> bool {
    let Some(caption) = caption else {
        return false;
    };
    let along = project_delta(caption.center, point, caption.along);
    let perpendicular = project_delta(caption.center, point, caption.normal);
    along.abs() <= caption.width / 2.0 + margin
        && perpendicular.abs() <= caption.height / 2.0 + margin
}

fn line_geom(from: Point, to: Point) -> Geom {
    Geom::Line { a: from, b: to, ends: None, rot: None }
}

/// Is the point on the distance annotation: caption, any segment, or an
/// ending?
pub fn distance_hit(layout: &DistanceLayout, point: Point, stroke_width: f64, margin: f64) -> bool {
    if distance_caption_hit(layout.caption.as_ref(), point, margin) {
        return true;
    }
    let segments = layout
        .dimension_segments
        .iter()
        .chain(&layout.leader_segments)
        .chain(&layout.caption_connector);
    let on_segment = segments
        .clone()
        .any(|s| geom_hit(&line_geom(s.from, s.to), point, margin, false, stroke_width));
    on_segment
        || layout.endings.iter().any(|ending| match ending {
            RenderNode::Poly { points, closed } => geom_hit(
                &Geom::Poly { points: points.clone(), closed: *closed, ends: None, rot: None },
                point,
                margin,
                true,
                stroke_width,
            ),
            RenderNode::Ellipse { rect } => geom_hit(
                &Geom::Rect { rect: *rect, ellipse: true, rot: None },
                point,
                margin,
                true,
                stroke_width,
            ),
            _ => false,
        })
}

/// The caption's centre, or `None` without a line or a caption.
pub fn distance_caption_at(
    geom: &Geom,
    appearance: &DistanceAppearance,
    width: f64,
) -> Option<Point> {
    distance_layout(geom, appearance, width)?.caption.map(|c| c.center)
}

/// The leader length that puts the dimension line through `point`.
pub fn distance_leader_length(geom: &Geom, point: Point) -> f64 {
    let Geom::Line { a, b, .. } = geom else {
        return 0.0;
    };
    project_delta(*a, point, line_axes(*a, *b).normal)
}

/// The appearance with its caption displaced by a content-space delta.
pub fn move_distance_caption(
    geom: &Geom,
    appearance: &DistanceAppearance,
    delta: Point,
) -> DistanceAppearance {
    let Geom::Line { a, b, .. } = geom else {
        return appearance.clone();
    };
    let Axes { along, normal, .. } = line_axes(*a, *b);
    let previous = appearance.caption.offset;
    let offset = CaptionOffset {
        along: previous.map_or(0.0, |o| o.along) + delta.x * along.x + delta.y * along.y,
        perpendicular: previous.map_or(0.0, |o| o.perpendicular)
            + delta.x * normal.x
            + delta.y * normal.y,
    };
    DistanceAppearance {
        caption: LineDimensionCaption { offset: Some(offset), ..appearance.caption },
        ..appearance.clone()
    }
}

/// The painted distance annotation.
pub fn distance_scene(
    geom: &Geom,
    appearance: &DistanceAppearance,
    style: &Style,
) -> Vec<SceneNode> {
    let Some(layout) = distance_layout(geom, appearance, style.stroke_width) else {
        return Vec::new();
    };
    let paint = Paint {
        stroke: Some(style.color.clone()),
        width: Some(style.stroke_width),
        opacity: Some(style.opacity),
        dash: match &style.border {
            Border::Dashed { dash } => Some(dash.clone()),
            _ => None,
        },
        ..Paint::default()
    };
    let mut nodes: Vec<SceneNode> = layout
        .dimension_segments
        .iter()
        .chain(&layout.leader_segments)
        .chain(&layout.caption_connector)
        .map(|s| SceneNode::Line { a: s.from, b: s.to, paint: paint.clone() })
        .collect();
    for ending in &layout.endings {
        let filled =
            matches!(ending, RenderNode::Ellipse { .. } | RenderNode::Poly { closed: true, .. });
        let fill = if filled { style.interior_color.clone() } else { None };
        nodes.push(SceneNode::painted(ending.clone(), Paint { fill, ..paint.clone() }));
    }
    nodes.extend(measurement_caption_scene(layout.caption.as_ref(), style));
    nodes
}

/// The caption as painted text, baseline-anchored and turned with the line.
pub fn measurement_caption_scene(
    caption: Option<&DistanceCaptionLayout>,
    style: &Style,
) -> Vec<SceneNode> {
    let Some(caption) = caption else {
        return Vec::new();
    };
    let baseline = offset_point(caption.center, caption.normal, -DISTANCE_CAPTION_SIZE * 0.33);
    let at = offset_point(baseline, caption.along, -caption.width / 2.0);
    vec![SceneNode::Text {
        at,
        text: caption.text.clone(),
        font_size: DISTANCE_CAPTION_SIZE,
        font_family: Some("Helvetica, Arial, sans-serif".into()),
        rotation: Some((caption.along.y.atan2(caption.along.x) * 180.0) / std::f64::consts::PI),
        paint: Paint {
            fill: Some("#000000".into()),
            opacity: Some(style.opacity),
            ..Paint::default()
        },
    }]
}
