//! `measurement-shape.ts`: a perimeter's or area's caption: its readout, its
//! automatic place, its layout, and dragging it.

use crate::engine::measure::{DimensionInput, Readout, measurement_readout};
use crate::geometry::{geom_rotation, selection_quad};
use crate::js;
use crate::measurement::{
    CaptionLayout, DISTANCE_CAPTION_SIZE, DistanceLayout, MeasurementAppearance,
    distance_caption_width, distance_layout, distance_selection_quad, move_distance_caption,
};
use crate::object::Nullable;
use crate::rect::{point_in_poly, rotate_point, union_rect};
use crate::shapes::points::{drawn_point_of, drawn_vertices_of, upright_point_of};
use crate::types::{Point, Rect, Shape, Style};

const ORIGIN: Point = Point { x: 0.0, y: 0.0 };

/// A perimeter's or area's caption layout (`ShapeMeasurementLayout`).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeMeasurementLayout {
    pub caption: Option<CaptionLayout>,
    pub visual_bounds: Rect,
    pub selection_points: Vec<Point>,
}

/// `shapeMeasurementReadout(geometry, appearance)`.
pub fn shape_measurement_readout(geometry: &Shape, appearance: &MeasurementAppearance) -> Readout {
    let vertices = match geometry {
        Shape::Poly(p) => p.vertices.clone(),
        _ => Vec::new(),
    };
    let input = DimensionInput {
        subtype: if appearance.intent() == "polygon-dimension" { "polygon" } else { "polyline" },
        intent: Some(appearance.intent()),
        measure: appearance.measure(),
        line_points: None,
        vertices,
    };
    measurement_readout(&input)
}

/// `shapeMeasurementLabel(geometry, appearance)`.
pub fn shape_measurement_label(geometry: &Shape, appearance: &MeasurementAppearance) -> String {
    let readout = shape_measurement_readout(geometry, appearance);
    match readout {
        Readout::Readout { label, .. } => label,
        Readout::Unavailable("invalid-geometry") => "—".into(),
        Readout::Unavailable(_) => appearance.contents().unwrap_or("").into(),
    }
}

/// `automaticShapeCaptionCenter(points, closed)`.
pub fn automatic_shape_caption_center(points: &[Point], closed: bool) -> Point {
    if points.is_empty() {
        return ORIGIN;
    }
    if !closed {
        let lengths: Vec<f64> =
            points.windows(2).map(|w| js::hypot(w[1].x - w[0].x, w[1].y - w[0].y)).collect();
        let mut remaining = lengths.iter().sum::<f64>() / 2.0;
        for (i, length) in lengths.iter().enumerate() {
            if *length > 0.0 && remaining <= *length {
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
    let mut area = 0.0;
    let mut moment_x = 0.0;
    let mut moment_y = 0.0;
    for i in 0..points.len() {
        let start = Point { x: points[i].x - origin.x, y: points[i].y - origin.y };
        let next = points[(i + 1) % points.len()];
        let end = Point { x: next.x - origin.x, y: next.y - origin.y };
        let cross = start.x * end.y - end.x * start.y;
        area += cross;
        moment_x += (start.x + end.x) * cross;
        moment_y += (start.y + end.y) * cross;
    }
    if area.abs() > 1e-8 {
        let center =
            Point { x: origin.x + moment_x / (3.0 * area), y: origin.y + moment_y / (3.0 * area) };
        if point_in_poly(center, points) {
            return center;
        }
    }
    let bounds = union_rect(points);
    let mut fallback =
        Point { x: bounds.x + bounds.width / 2.0, y: bounds.y + bounds.height / 2.0 };
    let mut intersections: Vec<f64> = Vec::new();
    for i in 0..points.len() {
        let start = points[i];
        let end = points[(i + 1) % points.len()];
        if (start.y > fallback.y) != (end.y > fallback.y) {
            intersections
                .push(start.x + ((fallback.y - start.y) * (end.x - start.x)) / (end.y - start.y));
        }
    }
    js::sort_by(&mut intersections, |l, r| l - r);
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

/// `shapeMeasurementLayout(geometry, appearance, style)`.
pub fn shape_measurement_layout(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> Option<ShapeMeasurementLayout> {
    let Shape::Poly(poly) = geometry else { return None };
    if poly.vertices.is_empty() {
        return None;
    }
    let angle = geom_rotation(geometry);
    let local: Vec<Point> =
        drawn_vertices_of(geometry).iter().map(|p| rotate_point(*p, ORIGIN, -angle)).collect();
    let center = match &poly.caption_center {
        Some(Nullable::Value(c)) => drawn_point_of(geometry, *c),
        _ => rotate_point(automatic_shape_caption_center(&local, poly.closed), ORIGIN, angle),
    };
    let text = shape_measurement_label(geometry, appearance);
    let width = distance_caption_width(&text);
    let height = DISTANCE_CAPTION_SIZE;
    let along = rotate_point(Point { x: 1.0, y: 0.0 }, ORIGIN, angle);
    let normal = Point { x: along.y, y: -along.x };
    let corner = |x: f64, y: f64| Point {
        x: center.x + x * along.x + y * normal.x,
        y: center.y + x * along.y + y * normal.y,
    };
    let caption = (appearance.caption_enabled() && !text.is_empty()).then(|| CaptionLayout {
        text: text.clone(),
        center,
        along,
        normal,
        width,
        height,
        bounds: [
            corner(-width / 2.0, -height / 2.0),
            corner(width / 2.0, -height / 2.0),
            corner(width / 2.0, height / 2.0),
            corner(-width / 2.0, height / 2.0),
        ],
    });
    let mut selection_points = selection_quad(geometry, style.stroke()).to_vec();
    if let Some(c) = &caption {
        selection_points.extend(c.bounds.iter().copied());
    }
    let visual_bounds = union_rect(&selection_points);
    Some(ShapeMeasurementLayout { caption, visual_bounds, selection_points })
}

/// A distance's or a shape's measurement layout (`measurementLayout`).
#[derive(Clone, Debug, PartialEq)]
pub enum MeasurementLayout {
    Distance(Box<DistanceLayout>),
    Shape(ShapeMeasurementLayout),
}

impl MeasurementLayout {
    pub fn caption(&self) -> Option<&CaptionLayout> {
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

/// `measurementLayout(geometry, appearance, style)`.
pub fn measurement_layout(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> Option<MeasurementLayout> {
    if appearance.is_distance() {
        distance_layout(geometry, appearance, style.stroke_width)
            .map(|d| MeasurementLayout::Distance(Box::new(d)))
    } else {
        shape_measurement_layout(geometry, appearance, style).map(MeasurementLayout::Shape)
    }
}

/// `measurementSelectionQuad(geometry, appearance, style)`.
pub fn measurement_selection_quad(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    style: &Style,
) -> [Point; 4] {
    if appearance.is_distance() {
        return distance_selection_quad(geometry, appearance, style.stroke_width);
    }
    let Some(layout) = shape_measurement_layout(geometry, appearance, style) else {
        return selection_quad(geometry, style.stroke());
    };
    let angle = geom_rotation(geometry);
    let bounds = union_rect(
        &layout
            .selection_points
            .iter()
            .map(|p| rotate_point(*p, ORIGIN, -angle))
            .collect::<Vec<_>>(),
    );
    [
        Point { x: bounds.x, y: bounds.y },
        Point { x: bounds.x + bounds.width, y: bounds.y },
        Point { x: bounds.x + bounds.width, y: bounds.y + bounds.height },
        Point { x: bounds.x, y: bounds.y + bounds.height },
    ]
    .map(|p| rotate_point(p, ORIGIN, angle))
}

/// `moveMeasurementCaption(geometry, appearance, delta, style)`: a
/// distance's caption offset changes, a perimeter's or area's caption
/// center (its shape's).
pub fn move_measurement_caption(
    geometry: &Shape,
    appearance: &MeasurementAppearance,
    delta: Point,
    style: &Style,
) -> (Shape, MeasurementAppearance) {
    if appearance.is_distance() {
        return (geometry.clone(), move_distance_caption(geometry, appearance, delta));
    }
    let center = shape_measurement_layout(geometry, appearance, style)
        .and_then(|l| l.caption)
        .map(|c| c.center);
    let (Some(center), Shape::Poly(poly)) = (center, geometry) else {
        return (geometry.clone(), appearance.clone());
    };
    let moved = Point { x: center.x + delta.x, y: center.y + delta.y };
    let mut next = poly.clone();
    // `{ ...geometry, captionCenter }`: a center the shape didn't have prints last.
    next.seal();
    next.caption_center = Some(Nullable::Value(upright_point_of(geometry, moved)));
    (Shape::Poly(next), appearance.clone())
}
