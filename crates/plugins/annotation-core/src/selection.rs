//! `selection.ts`: the complete annotation frame shared by selection and
//! transform gestures.

use crate::anchor::{anchor_mode_of, anchored_geom, anchored_stroke_width};
use crate::geometry::{geom_rotation, is_rotatable_geom, selection_quad, turn_pivot_of};
use crate::measurement::{MeasurementAppearance, measurement_of};
use crate::measurement_shape::measurement_selection_quad;
use crate::record::{shape_of, style_of};
use crate::types::{ModelAnnotation, Point, Shape, Style, ViewEnv};

/// The frame a selection outlines (`SelectionFrame`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionFrame {
    pub corners: [Point; 4],
    pub center: Point,
    pub angle: f64,
}

/// What a gesture lays over a record's own fields (`live`).
#[derive(Clone, Debug, Default)]
pub struct Live {
    pub geometry: Option<Shape>,
    pub style: Option<Style>,
    pub measure: Option<Option<MeasurementAppearance>>,
}

/// `annotationSelectionFrame(record, view, live)`.
pub fn annotation_selection_frame(
    record: &ModelAnnotation,
    view: Option<&ViewEnv>,
    live: &Live,
) -> SelectionFrame {
    let style = live.style.clone().unwrap_or_else(|| style_of(&record.annotation));
    let measure = match &live.measure {
        Some(Some(m)) => Some(m.clone()),
        _ => measurement_of(&record.annotation),
    };
    let mode = anchor_mode_of(record);
    let base = live.geometry.clone().unwrap_or_else(|| shape_of(&record.annotation));
    let geometry = anchored_geom(&base, mode.as_ref(), view);
    let stroke_width = anchored_stroke_width(style.stroke_width, mode.as_ref(), view);
    let mut stroked = style.clone();
    stroked.stroke_width = stroke_width;
    let corners = match &measure {
        Some(m) => measurement_selection_quad(&geometry, m, &stroked),
        None => selection_quad(&geometry, stroked.stroke()),
    };
    SelectionFrame {
        corners,
        center: Point {
            x: (corners[0].x + corners[1].x + corners[2].x + corners[3].x) / 4.0,
            y: (corners[0].y + corners[1].y + corners[2].y + corners[3].y) / 4.0,
        },
        angle: if is_rotatable_geom(&geometry) { geom_rotation(&geometry) } else { 0.0 },
    }
}

/// `annotationTurnPivot(record, view)`.
pub fn annotation_turn_pivot(record: &ModelAnnotation, view: Option<&ViewEnv>) -> Point {
    turn_pivot_of(&anchored_geom(
        &shape_of(&record.annotation),
        anchor_mode_of(record).as_ref(),
        view,
    ))
}
