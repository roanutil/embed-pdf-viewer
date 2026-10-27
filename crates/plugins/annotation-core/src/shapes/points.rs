//! `shapes/points.ts`: lines, polylines, polygons and ink. The points are
//! kept upright with the turn that draws them (`rotation`, about the middle
//! of their box); where they are drawn is worked out from these.

use serde_json::Value;

use crate::cloudy::{cloudy_poly_bounds, cloudy_poly_outline, cloudy_poly_path};
use crate::endings::{ending_nodes, ending_pieces};
use crate::engine::geom::{
    PointTurn, page_point_turned, page_point_unturned, page_turn_of_drawn, page_turn_of_upright,
};
use crate::js;
use crate::json::{self, Object};
use crate::object::{Json, Nullable};
use crate::painted::PaintedPiece;
use crate::rect::{
    expand_rect, normalize_deg, rect_center, rect_corner_points, rotate_point, union_rect,
};
use crate::types::{
    Handle, InkShape, LineEndings, LinePoints, LineShape, Placement, Point, PolyShape, Rect,
    RenderNode, Shape, Stroke,
};

/// The miter limit shared by the bounds math and the renderer.
pub const MITER_LIMIT: f64 = 10.0;

fn points_member(annotation: &Object, name: &str) -> Vec<Point> {
    annotation
        .get(name)
        .and_then(Value::as_array)
        .map(|a| a.iter().map(json::point_loose).collect())
        .unwrap_or_default()
}

/// A line's or polyline's endings, as the annotation keeps them.
fn endings_member(annotation: &Object) -> Option<LineEndings> {
    json::get(annotation, "lineEndings")
        .filter(|v| json::truthy(Some(v)))
        .and_then(|v| LineEndings::read(v).ok())
}

/// A points kind's shape, read off its annotation.
pub fn read_points(annotation: &Object) -> Shape {
    let rotation = json::get_num(annotation, "rotation").unwrap_or(0.0);
    let caption_center = || match annotation.get("captionCenter") {
        None => None,
        Some(Value::Null) => Some(Nullable::Null),
        Some(v) => Some(Nullable::Value(json::point_loose(v))),
    };
    match json::get_str(annotation, "subtype").unwrap_or("") {
        "line" => {
            let lp = annotation
                .get("linePoints")
                .and_then(|v| LinePoints::read(v).ok())
                .unwrap_or_else(|| {
                    LinePoints::new(
                        Point { x: f64::NAN, y: f64::NAN },
                        Point { x: f64::NAN, y: f64::NAN },
                    )
                });
            Shape::Line(LineShape {
                line_points: lp,
                line_endings: endings_member(annotation),
                rotation,
                order: Default::default(),
            })
        }
        "polyline" => Shape::Poly(PolyShape {
            vertices: points_member(annotation, "vertices"),
            closed: false,
            line_endings: endings_member(annotation),
            caption_center: caption_center(),
            rotation,
            order: Default::default(),
        }),
        "polygon" => Shape::Poly(PolyShape {
            vertices: points_member(annotation, "vertices"),
            closed: true,
            line_endings: None,
            caption_center: caption_center(),
            rotation,
            order: Default::default(),
        }),
        _ => {
            let ink = annotation
                .get("inkList")
                .and_then(Value::as_array)
                .map(|strokes| {
                    strokes
                        .iter()
                        .map(|s| {
                            s.as_array()
                                .map(|a| a.iter().map(json::point_loose).collect())
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            Shape::Ink(InkShape { ink_list: ink, rotation, order: Default::default() })
        }
    }
}

/// `{ start: 'none', end: 'none' }`.
pub fn no_endings() -> LineEndings {
    LineEndings::new("none", "none")
}

/// `lineEndingsOf(annotation)`: a line's or polyline's endings, none for the rest.
pub fn line_endings_of(annotation: &Object) -> LineEndings {
    endings_member(annotation).unwrap_or_else(no_endings)
}

/// A line where a create gesture places a segment.
pub fn place_points(placement: &Placement, annotation: &Object) -> Option<Shape> {
    let Placement::Segment(p) = placement else { return None };
    if json::get_str(annotation, "subtype") != Some("line") {
        return None;
    }
    Some(Shape::Line(LineShape {
        line_points: LinePoints::new(p.a, p.b),
        line_endings: Some(line_endings_of(annotation)),
        rotation: 0.0,
        order: Default::default(),
    }))
}

fn rotation_field(rotation: f64) -> Value {
    if rotation == 0.0 || rotation.is_nan() { Value::Null } else { json::num(rotation) }
}

/// The engine fields that state a points shape's points and turn.
pub fn write_points(shape: &Shape) -> Object {
    match shape {
        Shape::Line(s) => json::object([
            ("linePoints", s.line_points.json()),
            ("rotation", rotation_field(s.rotation)),
        ]),
        Shape::Poly(s) => {
            let mut out = json::object([("vertices", s.vertices.json())]);
            if let Some(center) = &s.caption_center {
                out.insert("captionCenter".into(), center.json());
            }
            out.insert("rotation".into(), rotation_field(s.rotation));
            out
        }
        Shape::Ink(s) => {
            json::object([("inkList", s.ink_list.json()), ("rotation", rotation_field(s.rotation))])
        }
        _ => Object::new(),
    }
}

fn rotation_of(shape: &Shape) -> f64 {
    match shape {
        Shape::Line(s) => s.rotation,
        Shape::Poly(s) => s.rotation,
        Shape::Ink(s) => s.rotation,
        _ => 0.0,
    }
}

/// The upright points, one list per stroke.
pub fn upright_strokes_of(shape: &Shape) -> Vec<Vec<Point>> {
    match shape {
        Shape::Line(s) => vec![vec![s.line_points.start, s.line_points.end]],
        Shape::Poly(s) => vec![s.vertices.clone()],
        Shape::Ink(s) => s.ink_list.clone(),
        _ => Vec::new(),
    }
}

/// The shape with `strokes` as its upright points.
fn with_upright_strokes(shape: &Shape, strokes: Vec<Vec<Point>>) -> Shape {
    match shape {
        Shape::Line(s) => {
            let first = strokes.into_iter().next().unwrap_or_default();
            let mut next = s.clone();
            next.line_points = LinePoints::new(first[0], first[1]);
            Shape::Line(next)
        }
        Shape::Poly(s) => {
            let mut next = s.clone();
            next.vertices = strokes.into_iter().next().unwrap_or_default();
            Shape::Poly(next)
        }
        Shape::Ink(s) => {
            let mut next = s.clone();
            next.ink_list = strokes;
            Shape::Ink(next)
        }
        other => other.clone(),
    }
}

/// The shape with every upright point, a caption center included, moved by `map`.
fn map_upright(shape: &Shape, map: impl Fn(Point) -> Point) -> Shape {
    let strokes =
        upright_strokes_of(shape).into_iter().map(|s| s.into_iter().map(&map).collect()).collect();
    let mut moved = with_upright_strokes(shape, strokes);
    if let Shape::Poly(p) = &mut moved
        && let Some(Nullable::Value(center)) = p.caption_center
    {
        p.caption_center = Some(Nullable::Value(map(center)));
    }
    moved
}

/// The turn that draws the upright points, or `None` upright.
fn turn_of(shape: &Shape) -> Option<PointTurn> {
    let rotation = rotation_of(shape);
    if rotation == 0.0 || rotation.is_nan() {
        return None;
    }
    let flat: Vec<Point> = upright_strokes_of(shape).concat();
    Some(page_turn_of_upright(&flat, rotation))
}

/// `drawnPointOf(shape, point)`: where an upright point is drawn.
pub fn drawn_point_of(shape: &Shape, point: Point) -> Point {
    match turn_of(shape) {
        Some(turn) => page_point_turned(point, turn),
        None => point,
    }
}

/// `uprightPointOf(shape, point)`: the upright point drawn at `point`.
pub fn upright_point_of(shape: &Shape, point: Point) -> Point {
    match turn_of(shape) {
        Some(turn) => page_point_unturned(point, turn),
        None => point,
    }
}

/// `drawnStrokesOf(shape)`: where the points are drawn.
pub fn drawn_strokes_of(shape: &Shape) -> Vec<Vec<Point>> {
    let strokes = upright_strokes_of(shape);
    match turn_of(shape) {
        Some(turn) => strokes
            .into_iter()
            .map(|s| s.into_iter().map(|p| page_point_turned(p, turn)).collect())
            .collect(),
        None => strokes,
    }
}

/// `drawnLineOf(shape)`: a line's two ends where they are drawn.
pub fn drawn_line_of(shape: &Shape) -> (Point, Point) {
    let strokes = drawn_strokes_of(shape);
    let first = strokes.first().cloned().unwrap_or_default();
    (first.first().copied().unwrap_or_default(), first.get(1).copied().unwrap_or_default())
}

/// `drawnVerticesOf(shape)`.
pub fn drawn_vertices_of(shape: &Shape) -> Vec<Point> {
    drawn_strokes_of(shape).into_iter().next().unwrap_or_default()
}

/// Where the shape turns: the middle of the box around its upright points.
pub fn points_middle_of(shape: &Shape) -> Point {
    rect_center(union_rect(&upright_strokes_of(shape).concat()))
}

/// The box around the drawn points.
pub fn points_bounds(shape: &Shape) -> Rect {
    union_rect(&drawn_strokes_of(shape).concat())
}

pub fn points_translate(shape: &Shape, delta: Point) -> Shape {
    // `!delta.x && !delta.y`: zero or NaN on both axes is no move.
    if (delta.x == 0.0 || delta.x.is_nan()) && (delta.y == 0.0 || delta.y.is_nan()) {
        return shape.clone();
    }
    map_upright(shape, |p| Point { x: p.x + delta.x, y: p.y + delta.y })
}

const NOISE: f64 = 1e-9;

fn with_rotation(shape: Shape, rotation: f64) -> Shape {
    match shape {
        Shape::Line(mut s) => {
            s.rotation = rotation;
            Shape::Line(s)
        }
        Shape::Poly(mut s) => {
            s.rotation = rotation;
            Shape::Poly(s)
        }
        Shape::Ink(mut s) => {
            s.rotation = rotation;
            Shape::Ink(s)
        }
        other => other,
    }
}

pub fn points_rotate_about(shape: &Shape, pivot: Point, degrees: f64) -> Shape {
    let middle = points_middle_of(shape);
    let moved = rotate_point(middle, pivot, degrees);
    let delta = Point { x: moved.x - middle.x, y: moved.y - middle.y };
    let shifted = if delta.x.abs() < NOISE && delta.y.abs() < NOISE {
        shape.clone()
    } else {
        points_translate(shape, delta)
    };
    with_rotation(shifted, normalize_deg(rotation_of(shape) + degrees))
}

pub fn points_scale_about(shape: &Shape, anchor: Point, sx: f64, sy: f64) -> Shape {
    let rotation = rotation_of(shape);
    let turned = rotation != 0.0 && !rotation.is_nan();
    let about = if turned { points_middle_of(shape) } else { anchor };
    let middle = if turned {
        Point { x: anchor.x + (about.x - anchor.x) * sx, y: anchor.y + (about.y - anchor.y) * sy }
    } else {
        anchor
    };
    map_upright(shape, |p| Point {
        x: middle.x + (p.x - about.x) * sx,
        y: middle.y + (p.y - about.y) * sy,
    })
}

pub fn points_upright(shape: &Shape, pivot: Option<Point>) -> Shape {
    let rotation = rotation_of(shape);
    if rotation == 0.0 || rotation.is_nan() {
        return shape.clone();
    }
    points_rotate_about(shape, pivot.unwrap_or_else(|| points_middle_of(shape)), -rotation)
}

pub fn points_handles(shape: &Shape) -> Vec<Handle> {
    if matches!(shape, Shape::Ink(_)) {
        return Vec::new();
    }
    drawn_strokes_of(shape)
        .into_iter()
        .next()
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .map(|(i, at)| Handle::new(format!("v{i}"), at, "crosshair"))
        .collect()
}

pub fn points_drag(shape: &Shape, handle: &str, to: Point) -> Shape {
    if matches!(shape, Shape::Ink(_)) {
        return shape.clone();
    }
    let index = crate::js::number_after_first_unit(handle);
    let points = drawn_strokes_of(shape).into_iter().next().unwrap_or_default();
    if !index.is_finite() || index.fract() != 0.0 || index < 0.0 || index >= points.len() as f64 {
        return shape.clone();
    }
    let mut moved = points;
    moved[index as usize] = to;
    let rotation = rotation_of(shape);
    if rotation == 0.0 || rotation.is_nan() {
        return with_upright_strokes(shape, vec![moved]);
    }
    let turn = page_turn_of_drawn(&moved, rotation);
    with_upright_strokes(
        shape,
        vec![moved.into_iter().map(|p| page_point_unturned(p, turn)).collect()],
    )
}

// ── drawing ─────────────────────────────────────────────────────────────────

/// `strokeOutlinePoints(points, closed, strokeWidth)`.
fn stroke_outline_points(points: &[Point], closed: bool, stroke_width: f64) -> Vec<Point> {
    let half_width = stroke_width / 2.0;
    let count = points.len();
    if count == 0 {
        return Vec::new();
    }
    if count == 1 || half_width == 0.0 {
        return points.to_vec();
    }
    let seg_count = if closed { count } else { count - 1 };
    let mut dir = Vec::with_capacity(seg_count);
    let mut nrm = Vec::with_capacity(seg_count);
    for i in 0..seg_count {
        let start = points[i];
        let end = points[(i + 1) % count];
        let len = js::hypot(end.x - start.x, end.y - start.y);
        let len = if len == 0.0 || len.is_nan() { 1.0 } else { len };
        let ux = (end.x - start.x) / len;
        let uy = (end.y - start.y) / len;
        dir.push(Point { x: ux, y: uy });
        nrm.push(Point { x: -uy, y: ux });
    }
    let mut out = Vec::new();
    for i in 0..seg_count {
        let start = points[i];
        let end = points[(i + 1) % count];
        let nx = nrm[i].x * half_width;
        let ny = nrm[i].y * half_width;
        out.push(Point { x: start.x + nx, y: start.y + ny });
        out.push(Point { x: start.x - nx, y: start.y - ny });
        out.push(Point { x: end.x + nx, y: end.y + ny });
        out.push(Point { x: end.x - nx, y: end.y - ny });
    }
    let join_start = if closed { 0 } else { 1 };
    let join_end = if closed { count } else { count - 1 };
    #[allow(clippy::needless_range_loop)] // the index picks `dir` and `nrm` entries on both sides
    for vertex_index in join_start..join_end {
        let in_idx = if closed { (vertex_index + count - 1) % count } else { vertex_index - 1 };
        let out_idx = vertex_index;
        let incoming = dir[in_idx];
        let outgoing = dir[out_idx];
        let n1 = nrm[in_idx];
        let n2 = nrm[out_idx];
        let bx = incoming.x - outgoing.x;
        let by = incoming.y - outgoing.y;
        if js::hypot(bx, by) < 1e-9 {
            continue;
        }
        let mut mhx = n1.x + n2.x;
        let mut mhy = n1.y + n2.y;
        let ml = js::hypot(mhx, mhy);
        if ml < 1e-9 {
            continue;
        }
        mhx /= ml;
        mhy /= ml;
        if mhx * bx + mhy * by < 0.0 {
            mhx = -mhx;
            mhy = -mhy;
        }
        let cos_half = (mhx * n1.x + mhy * n1.y).abs();
        if cos_half < 1e-9 {
            continue;
        }
        let miter_len = half_width / cos_half;
        if miter_len > MITER_LIMIT * half_width {
            continue;
        }
        let vertex = points[vertex_index];
        out.push(Point { x: vertex.x + mhx * miter_len, y: vertex.y + mhy * miter_len });
    }
    out
}

/// `strokedOutlineOf(nodes, strokeWidth)`: the outline points of `nodes` stroked.
pub fn stroked_outline_of(nodes: &[RenderNode], stroke_width: f64) -> Vec<Point> {
    let mut out = Vec::new();
    for node in nodes {
        match node {
            RenderNode::Line(n) => {
                out.extend(stroke_outline_points(&[n.a, n.b], false, stroke_width))
            }
            RenderNode::Poly(n) => {
                out.extend(stroke_outline_points(&n.points, n.closed, stroke_width))
            }
            RenderNode::Ellipse(n) | RenderNode::Rect(n) => {
                out.extend(rect_corner_points(expand_rect(n.rect, stroke_width / 2.0)))
            }
            RenderNode::Path(_) => {}
        }
    }
    out
}

struct EndingSeg {
    tip: Point,
    angle: f64,
    ending: Option<String>,
}

fn ending_segs(shape: &Shape) -> Vec<EndingSeg> {
    let endings = match shape {
        Shape::Line(s) => s.line_endings.as_ref(),
        Shape::Poly(s) if !s.closed => s.line_endings.as_ref(),
        _ => None,
    };
    let Some(endings) = endings else { return Vec::new() };
    let points = drawn_strokes_of(shape).into_iter().next().unwrap_or_default();
    let count = points.len();
    if count < 2 {
        return Vec::new();
    }
    let (first, second, last, before_last) =
        (points[0], points[1], points[count - 1], points[count - 2]);
    vec![
        EndingSeg {
            tip: first,
            angle: (first.y - second.y).atan2(first.x - second.x),
            ending: endings.start.clone(),
        },
        EndingSeg {
            tip: last,
            angle: (last.y - before_last.y).atan2(last.x - before_last.x),
            ending: endings.end.clone(),
        },
    ]
}

fn ending_nodes_of(seg: &EndingSeg, stroke_width: f64) -> Vec<RenderNode> {
    ending_nodes(seg.tip, seg.angle, seg.ending.as_deref(), stroke_width)
}

/// What a points shape draws, as the engine's `rect` takes it in.
pub fn points_drawn_bounds(shape: &Shape, stroke: Stroke) -> Rect {
    let strokes = drawn_strokes_of(shape);
    if matches!(shape, Shape::Ink(_)) {
        return expand_rect(union_rect(&strokes.concat()), stroke.stroke_width / 2.0);
    }
    let points = strokes.into_iter().next().unwrap_or_default();
    let closed = matches!(shape, Shape::Poly(p) if p.closed);
    if let Some(intensity) = stroke.cloud().filter(|_| closed) {
        return cloudy_poly_bounds(&points, intensity, stroke.stroke_width);
    }
    let endings: Vec<RenderNode> =
        ending_segs(shape).iter().flat_map(|s| ending_nodes_of(s, stroke.stroke_width)).collect();
    let mut all = stroke_outline_points(&points, closed, stroke.stroke_width);
    all.extend(stroked_outline_of(&endings, stroke.stroke_width));
    union_rect(&all)
}

/// `pointsCorners`: the oriented box around what the shape draws.
pub fn points_corners(shape: &Shape, stroke: Stroke) -> [Point; 4] {
    let upright = with_rotation(shape.clone(), 0.0);
    let corners = rect_corner_points(points_drawn_bounds(&upright, stroke));
    let rotation = rotation_of(shape);
    if rotation == 0.0 || rotation.is_nan() {
        return corners;
    }
    let middle = points_middle_of(shape);
    corners.map(|c| rotate_point(c, middle, rotation))
}

/// What a points shape paints.
pub fn points_painted(shape: &Shape, stroke: Stroke, filled: bool) -> Vec<PaintedPiece> {
    let half_width = stroke.stroke_width / 2.0;
    let strokes = drawn_strokes_of(shape);
    let closed = matches!(shape, Shape::Poly(p) if p.closed);
    let outline = strokes.first().cloned();
    let mut pieces = Vec::new();
    if let (Some(outline), true, Some(intensity)) = (&outline, closed, stroke.cloud())
        && outline.len() >= 3
    {
        let curves = cloudy_poly_outline(outline, intensity, stroke.stroke_width);
        if filled {
            pieces.push(PaintedPiece::Area { ring: outline.clone() });
            for ring in &curves {
                pieces.push(PaintedPiece::Area { ring: ring.clone() });
            }
        }
        for points in curves {
            pieces.push(PaintedPiece::Stroke { points, closed: true, half_width });
        }
        return pieces;
    }
    if let (Some(outline), true, true) = (&outline, filled, closed) {
        pieces.push(PaintedPiece::Area { ring: outline.clone() });
    }
    for points in strokes {
        pieces.push(PaintedPiece::Stroke { points, closed, half_width });
    }
    for seg in ending_segs(shape) {
        pieces.extend(ending_pieces(
            &ending_nodes_of(&seg, stroke.stroke_width),
            stroke.stroke_width,
        ));
    }
    pieces
}

/// What a points shape draws, where it is drawn.
pub fn points_scene(shape: &Shape, stroke: Stroke) -> Vec<RenderNode> {
    let strokes = drawn_strokes_of(shape);
    if matches!(shape, Shape::Ink(_)) {
        return strokes.into_iter().map(|s| RenderNode::poly(s, false)).collect();
    }
    let points = strokes.into_iter().next().unwrap_or_default();
    if let Shape::Poly(p) = shape
        && let Some(intensity) = stroke.cloud().filter(|_| p.closed && points.len() >= 3)
    {
        return vec![RenderNode::path(cloudy_poly_path(&points, intensity, stroke.stroke_width))];
    }
    let mut nodes = match shape {
        Shape::Line(_) => vec![RenderNode::line(
            points.first().copied().unwrap_or(Point { x: f64::NAN, y: f64::NAN }),
            points.get(1).copied().unwrap_or(Point { x: f64::NAN, y: f64::NAN }),
        )],
        Shape::Poly(p) => vec![RenderNode::poly(points.clone(), p.closed)],
        _ => Vec::new(),
    };
    for seg in ending_segs(shape) {
        nodes.extend(ending_nodes_of(&seg, stroke.stroke_width));
    }
    nodes
}
