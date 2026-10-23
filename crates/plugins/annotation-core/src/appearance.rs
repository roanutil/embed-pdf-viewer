//! `appearance.ts`: how an annotation is drawn, from the engine's raster of
//! its appearance (`baked`) or live from its fields (`vector`). Everything
//! that chooses between the two is here.

use std::rc::Rc;

use epdf_geometry::{Size, contained_rect};

use crate::anchor::{anchor_mode_of, anchor_of, anchored_box};
use crate::engine::appearance::{AppearanceChange, appearance_change_of};
use crate::engine::resolve::resolve_annotation_patch;
use crate::error::CoreResult;
use crate::geometry::geom_rotation;
use crate::json::{self, Object};
use crate::record::{annotation_after, kind_of, shape_of};
use crate::rect::normalize_deg;
use crate::types::{AnnotationPatch, Draft, Model, ModelAnnotation, Rect, Shape, Source, ViewEnv};

/// `drawsLive(record)`: its kind's live drawing, or its lender's.
pub fn draws_live(annotation: &Object, lent: bool) -> bool {
    lent || !kind_of(annotation).caps.raster_only
}

/// `sourceOfNew(annotation)`.
pub fn source_of_new(annotation: &Object) -> Source {
    if draws_live(annotation, false) { Source::Vector } else { Source::Baked }
}

/// `sourceOfConfirmed(annotation, vector)`.
pub fn source_of_confirmed(annotation: &Object, vector: bool) -> Source {
    let has_appearance = json::truthy(annotation.get("hasAppearance"));
    if draws_live(annotation, false) && (vector || !has_appearance) {
        Source::Vector
    } else {
        Source::Baked
    }
}

/// What changes in how a record is drawn once a patch is written (`DrawState`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawnAfter {
    pub source: Option<Source>,
    pub ap_box: Option<Rect>,
}

/// `drawnAfter(record, patch)`.
pub fn drawn_after(record: &ModelAnnotation, patch: &AnnotationPatch) -> CoreResult<DrawnAfter> {
    if !draws_live(&record.annotation, record.painter.is_some()) {
        return Ok(DrawnAfter::default());
    }
    let change = appearance_change_of(
        &record.annotation,
        &resolve_annotation_patch(&record.annotation, patch, None)?,
    );
    Ok(match change {
        AppearanceChange::Inert => DrawnAfter::default(),
        AppearanceChange::Translation { x, y } => match record.ap_box {
            Some(b) => {
                DrawnAfter { source: None, ap_box: Some(Rect { x: b.x + x, y: b.y + y, ..b }) }
            }
            None => DrawnAfter::default(),
        },
        AppearanceChange::Regenerate => {
            if record.source == Source::Vector {
                DrawnAfter::default()
            } else {
                DrawnAfter { source: Some(Source::Vector), ap_box: None }
            }
        }
    })
}

/// `applyChange(record, patch)`: the record after `patch`, drawn as the rule says.
pub fn apply_change(
    record: &ModelAnnotation,
    patch: &AnnotationPatch,
) -> CoreResult<ModelAnnotation> {
    let mut next = record.clone();
    next.annotation = Rc::new(annotation_after(&record.annotation, patch)?);
    let drawn = drawn_after(record, patch)?;
    if let Some(source) = drawn.source {
        next.source = source;
    }
    if let Some(ap_box) = drawn.ap_box {
        next.ap_box = Some(ap_box);
    }
    Ok(next)
}

/// `sourceDuring(model, id)`: how a record is drawn right now, its gesture included.
pub fn source_during(model: &Model, id: &str) -> Source {
    let Some(record) = model.record(id) else { return Source::Baked };
    if !draws_live(&record.annotation, record.painter.is_some()) {
        return record.source;
    }
    if model.editing.as_deref() == Some(id)
        && matches!(shape_of(&record.annotation), Shape::TextBox(_))
    {
        return Source::Vector;
    }
    match model.draft() {
        Some(Draft::Handle(d)) if d.id == id => Source::Vector,
        Some(Draft::Caption(d)) if d.id == id => Source::Vector,
        Some(Draft::Leader(d)) if d.id == id => Source::Vector,
        Some(Draft::Rotate(d)) if d.ids.iter().any(|i| i == id) => Source::Vector,
        Some(Draft::Group(d)) if d.ids.iter().any(|i| i == id) => Source::Vector,
        _ => record.source,
    }
}

fn raster_in_shape(record: &ModelAnnotation, rect: Rect) -> Rect {
    let annotation = &record.annotation;
    match record.ap_box {
        Some(ap)
            if json::get_str(annotation, "subtype") == Some("stamp")
                && json::get_str(annotation, "fit") == Some("contain") =>
        {
            contained_rect(Size { width: ap.width, height: ap.height }, rect)
        }
        _ => rect,
    }
}

/// Where a record's raster is drawn right now (`rasterPlacement`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RasterPlacement {
    pub rect: Option<Rect>,
    pub rot: Option<f64>,
}

/// `rasterPlacement(model, id, view, shapeNow)`.
pub fn raster_placement(
    model: &Model,
    id: &str,
    view: Option<&ViewEnv>,
    shape_now: &Shape,
) -> RasterPlacement {
    let Some(record) = model.record(id) else { return RasterPlacement::default() };
    let truthy = |x: f64| x != 0.0 && !x.is_nan();
    if kind_of(&record.annotation).caps.raster_only {
        let rot = geom_rotation(shape_now);
        return RasterPlacement {
            rect: match shape_now {
                Shape::Box(s) => Some(raster_in_shape(record, s.box_)),
                _ => record.ap_box,
            },
            rot: truthy(rot).then_some(rot),
        };
    }
    let Some(ap_box) = record.ap_box else {
        return RasterPlacement { rect: None, rot: record.ap_rot };
    };
    let projected = anchored_box(
        ap_box,
        anchor_of(&shape_of(&record.annotation)),
        anchor_mode_of(record).as_ref(),
        view,
    );
    let mut rect = projected.map_or(ap_box, |(b, _)| b);
    let rot = normalize_deg(record.ap_rot.unwrap_or(0.0) + projected.map_or(0.0, |(_, r)| r));
    if let Some(Draft::Move(d)) = model.draft()
        && d.ids.iter().any(|i| i == id)
    {
        rect = Rect { x: rect.x + d.delta.x, y: rect.y + d.delta.y, ..rect };
    }
    RasterPlacement { rect: Some(rect), rot: truthy(rot).then_some(rot) }
}
