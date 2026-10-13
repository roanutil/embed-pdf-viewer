//! `measurement-shape.ts`: the polyline perimeter and polygon area captions,
//! and the dispatch between them and the distance layout.

use serde::{Deserialize, Serialize};

use crate::geometry::{geom_rotation, point_in_poly, rotate_point, selection_quad, union_rect};
use crate::js;
use crate::measure::{DimensionInput, PdfMeasurement, Readout, Unavailable, measurement_readout};
use crate::measurement::{
    DISTANCE_CAPTION_SIZE, DistanceCaptionLayout, DistanceLayout, MeasurementAppearance,
    distance_caption_width, distance_layout, distance_selection_quad, move_distance_caption,
};
use crate::types::{Geom, PdfPoint, PdfRect, Point, Quad, Rect, Style};

/// `ShapeMeasurementAppearance['intent']`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeIntent {
    PolyLineDimension,
    PolygonDimension,
}

/// `ShapeDimensionCaption`: an absolute PDF centre, or automatic.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeDimensionCaption {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<PdfPoint>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShapeMeasurementAppearance {
    pub intent: ShapeIntent,
    #[serde(default)]
    pub measure: Option<PdfMeasurement>,
    pub caption: ShapeDimensionCaption,
    pub crop: PdfRect,
    pub text: String,
}

impl ShapeMeasurementAppearance {
    pub fn is_polygon(&self) -> bool {
        self.intent == ShapeIntent::PolygonDimension
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeMeasurementLayout {
    pub caption: Option<DistanceCaptionLayout>,
    pub selection_points: Vec<Point>,
    pub visual_bounds: Rect,
}

/// `DistanceLayout | ShapeMeasurementLayout`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MeasurementLayout {
    Distance(Box<DistanceLayout>),
    Shape(ShapeMeasurementLayout),
}

impl MeasurementLayout {
    pub fn caption(&self) -> Option<&DistanceCaptionLayout> {
        match self {
            MeasurementLayout::Distance(d) => d.caption.as_ref(),
            MeasurementLayout::Shape(s) => s.caption.as_ref(),
        }
    }

    pub fn visual_bounds(&self) -> Rect {
        match self {
            MeasurementLayout::Distance(d) => d.visual_bounds,
            MeasurementLayout::Shape(s) => s.visual_bounds,
        }
    }
}

const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

/// The caption centre in content space, when the appearance pins one.
pub fn shape_caption_point(appearance: &ShapeMeasurementAppearance) -> Option<Point> {
    let crop = appearance.crop;
    appearance.caption.center.map(|c| Point { x: c.x - crop.left, y: crop.top - c.y })
}

/// The appearance with its caption pinned at a content-space centre.
pub fn with_shape_caption_point(
    appearance: &ShapeMeasurementAppearance,
    center: Point,
) -> ShapeMeasurementAppearance {
    let crop = appearance.crop;
    let pdf = PdfPoint { x: center.x + crop.left, y: crop.top - center.y };
    ShapeMeasurementAppearance {
        caption: ShapeDimensionCaption { center: Some(pdf), ..appearance.caption },
        ..appearance.clone()
    }
}

/// A pinned shape caption carried through a content-space transform; a
/// distance or an automatic caption is left as it is.
pub fn transform_measurement_caption(
    appearance: Option<&MeasurementAppearance>,
    transform: impl Fn(Point) -> Point,
) -> Option<MeasurementAppearance> {
    let shape = match appearance? {
        MeasurementAppearance::Shape(s) => s,
        distance => return Some(distance.clone()),
    };
    Some(MeasurementAppearance::Shape(match shape_caption_point(shape) {
        Some(center) => with_shape_caption_point(shape, transform(center)),
        None => shape.clone(),
    }))
}

/// The engine readout of a shape measurement.
pub fn shape_measurement_readout(geom: &Geom, appearance: &ShapeMeasurementAppearance) -> Readout {
    let crop = appearance.crop;
    let vertices = match geom {
        Geom::Poly { points, .. } => {
            points.iter().map(|p| PdfPoint { x: p.x + crop.left, y: crop.top - p.y }).collect()
        }
        _ => Vec::new(),
    };
    let (subtype, intent) = if appearance.is_polygon() {
        ("polygon", "PolygonDimension")
    } else {
        ("polyline", "PolyLineDimension")
    };
    measurement_readout(&DimensionInput {
        subtype,
        intent,
        measure: appearance.measure.as_ref(),
        line_points: None,
        vertices,
    })
}

/// The caption text: the readout's label, an em dash for a shape it
/// cannot measure, else the stored text.
pub fn shape_measurement_label(geom: &Geom, appearance: &ShapeMeasurementAppearance) -> String {
    let readout = shape_measurement_readout(geom, appearance);
    if let Some(r) = readout.readout() {
        return r.label.clone();
    }
    if readout.unavailable() == Some(Unavailable::InvalidGeometry) {
        "—".into()
    } else {
        appearance.text.clone()
    }
}

/// The native shape-caption anchor: an open path's half-length point, lifted
/// above the line; a closed one's centroid, or the middle of the widest span
/// through the bounds' centre when the centroid falls outside.
pub fn automatic_shape_caption_center(points: &[Point], closed: bool) -> Point {
    if points.is_empty() {
        return ORIGIN;
    }
    if !closed {
        let lengths: Vec<f64> =
            points.windows(2).map(|w| js::hypot(w[1].x - w[0].x, w[1].y - w[0].y)).collect();
        let mut remaining = lengths.iter().fold(0.0, |sum, l| sum + l) / 2.0;
        for (i, &length) in lengths.iter().enumerate() {
            if length > 0.0 && remaining <= length {
                let fraction = remaining / length;
                return Point {
                    x: points[i].x + fraction * (points[i + 1].x - points[i].x),
                    y: points[i].y + fraction * (points[i + 1].y - points[i].y)
                        - DISTANCE_CAPTION_SIZE / 2.0
                        - 2.0,
                };
            }
            remaining -= length;
        }
        return points[0];
    }

    let origin = points[0];
    let n = points.len();
    let (mut area, mut x, mut y) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let a = Point { x: points[i].x - origin.x, y: points[i].y - origin.y };
        let next = points[(i + 1) % n];
        let b = Point { x: next.x - origin.x, y: next.y - origin.y };
        let cross = a.x * b.y - b.x * a.y;
        area += cross;
        x += (a.x + b.x) * cross;
        y += (a.y + b.y) * cross;
    }
    if area.abs() > 1e-8 {
        let center = Point { x: origin.x + x / (3.0 * area), y: origin.y + y / (3.0 * area) };
        if point_in_poly(center, points) {
            return center;
        }
    }

    let bounds = union_rect(points);
    let mut fallback =
        Point { x: bounds.x + bounds.width / 2.0, y: bounds.y + bounds.height / 2.0 };
    let mut intersections: Vec<f64> = Vec::new();
    for i in 0..n {
        let a = points[i];
        let b = points[(i + 1) % n];
        if (a.y > fallback.y) != (b.y > fallback.y) {
            intersections.push(a.x + ((fallback.y - a.y) * (b.x - a.x)) / (b.y - a.y));
        }
    }
    // V8's sort: a NaN crossing makes `a - b` NaN, which is no total order.
    crate::js::sort_by(&mut intersections, |a, b| a - b);
    let mut widest = -1.0;
    let mut i = 1;
    while i < intersections.len() {
        let width = intersections[i] - intersections[i - 1];
        if width > widest {
            widest = width;
            fallback.x = (intersections[i] + intersections[i - 1]) / 2.0;
        }
        i += 2;
    }
    fallback
}

/// A shape measurement's caption box, selection points and visual bounds,
/// `None` for anything but a poly with points.
pub fn shape_measurement_layout(
    geom: &Geom,
    appearance: &ShapeMeasurementAppearance,
    style: &Style,
) -> Option<ShapeMeasurementLayout> {
    let Geom::Poly { points, closed, .. } = geom else {
        return None;
    };
    if points.is_empty() {
        return None;
    }
    let angle = geom_rotation(geom);
    let local: Vec<Point> = points.iter().map(|p| rotate_point(*p, ORIGIN, -angle)).collect();
    let center = shape_caption_point(appearance).unwrap_or_else(|| {
        rotate_point(automatic_shape_caption_center(&local, *closed), ORIGIN, angle)
    });
    let text = shape_measurement_label(geom, appearance);
    let width = distance_caption_width(&text);
    let height = DISTANCE_CAPTION_SIZE;
    let along = rotate_point(Point { x: 1.0, y: 0.0 }, ORIGIN, angle);
    let normal = Point { x: along.y, y: -along.x };
    let corner = |x: f64, y: f64| Point {
        x: center.x + x * along.x + y * normal.x,
        y: center.y + x * along.y + y * normal.y,
    };
    let caption = (appearance.caption.enabled && !text.is_empty()).then(|| DistanceCaptionLayout {
        bounds: [
            corner(-width / 2.0, -height / 2.0),
            corner(width / 2.0, -height / 2.0),
            corner(width / 2.0, height / 2.0),
            corner(-width / 2.0, height / 2.0),
        ],
        text,
        center,
        along,
        normal,
        width,
        height,
    });
    let mut selection_points: Vec<Point> =
        selection_quad(geom, style.stroke_width, Some(&style.border)).to_vec();
    if let Some(c) = &caption {
        selection_points.extend(c.bounds);
    }
    Some(ShapeMeasurementLayout {
        caption,
        visual_bounds: union_rect(&selection_points),
        selection_points,
    })
}

/// The layout for either kind of measurement.
pub fn measurement_layout(
    geom: &Geom,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> Option<MeasurementLayout> {
    match appearance {
        MeasurementAppearance::Distance(d) => distance_layout(geom, d, style.stroke_width)
            .map(|l| MeasurementLayout::Distance(Box::new(l))),
        MeasurementAppearance::Shape(s) => {
            shape_measurement_layout(geom, s, style).map(MeasurementLayout::Shape)
        }
    }
}

/// The oriented selection quad of a measured annotation, caption included.
pub fn measurement_selection_quad(
    geom: &Geom,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> Quad {
    let shape = match appearance {
        MeasurementAppearance::Distance(d) => {
            return distance_selection_quad(geom, d, style.stroke_width);
        }
        MeasurementAppearance::Shape(s) => s,
    };
    let Some(layout) = shape_measurement_layout(geom, shape, style) else {
        return selection_quad(geom, style.stroke_width, Some(&style.border));
    };
    let angle = geom_rotation(geom);
    let local: Vec<Point> =
        layout.selection_points.iter().map(|p| rotate_point(*p, ORIGIN, -angle)).collect();
    let b = union_rect(&local);
    [
        Point { x: b.x, y: b.y },
        Point { x: b.x + b.width, y: b.y },
        Point { x: b.x + b.width, y: b.y + b.height },
        Point { x: b.x, y: b.y + b.height },
    ]
    .map(|p| rotate_point(p, ORIGIN, angle))
}

/// The appearance with its caption moved by a content-space delta; a shape
/// caption becomes pinned where it was dragged.
pub fn move_measurement_caption(
    geom: &Geom,
    appearance: &MeasurementAppearance,
    delta: Point,
    style: &Style,
) -> MeasurementAppearance {
    let shape = match appearance {
        MeasurementAppearance::Distance(d) => {
            return MeasurementAppearance::Distance(move_distance_caption(geom, d, delta));
        }
        MeasurementAppearance::Shape(s) => s,
    };
    match shape_measurement_layout(geom, shape, style).and_then(|l| l.caption) {
        Some(c) => MeasurementAppearance::Shape(with_shape_caption_point(
            shape,
            Point { x: c.center.x + delta.x, y: c.center.y + delta.y },
        )),
        None => appearance.clone(),
    }
}
