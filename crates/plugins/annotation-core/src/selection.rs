//! `selection.ts`: the annotation frame selection chrome and transform
//! gestures share.

use serde::{Deserialize, Serialize};

use crate::anchor::{anchor_mode_of, anchored_geom, anchored_stroke_width};
use crate::geometry::{geom_rotation, is_rotatable_geom, selection_quad};
use crate::measurement_shape::measurement_selection_quad;
use crate::types::{Annot, Point, Quad, Style, ViewEnv};

/// The oriented corners, their centre and the frame's angle.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectionFrame {
    pub corners: Quad,
    pub center: Point,
    pub angle: f64,
}

/// The complete frame of an annotation in view space: its projected
/// geometry, measurement caption included. A PDF `/Rect`'s padding never
/// changes it.
pub fn annotation_selection_frame(annotation: &Annot, view: Option<ViewEnv>) -> SelectionFrame {
    let mode = anchor_mode_of(annotation);
    let geometry = anchored_geom(&annotation.geom, mode, view);
    let stroke_width = anchored_stroke_width(annotation.style.stroke_width, mode, view);
    let corners = match &annotation.measure {
        Some(measure) => measurement_selection_quad(
            &geometry,
            measure,
            &Style { stroke_width, ..annotation.style.clone() },
        ),
        None => selection_quad(&geometry, stroke_width, Some(&annotation.style.border)),
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
