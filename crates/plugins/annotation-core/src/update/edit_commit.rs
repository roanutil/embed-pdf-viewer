//! `update/edit-commit.ts`: the release of an edit gesture. The previewed
//! result becomes the records' new geometry; a grab that changed nothing
//! writes nothing.

use std::rc::Rc;

use serde_json::Value;

use crate::anchor::{anchor_mode_of, unanchored_geom};
use crate::error::CoreResult;
use crate::geometry::{geom_rotate_about, geom_scale_about, geom_translate, group_resize_factors};
use crate::js;
use crate::json::{self, num};
use crate::measurement::measurement_of;
use crate::measurement_shape::{move_measurement_caption, shape_measurement_readout};
use crate::record::{shape_of, style_of, with_shape, with_values};
use crate::rect::MIN_SIZE;
use crate::types::{Draft, Effect, Model, ModelAnnotation};
use crate::update::changes::{commit_view_gesture, geom_equal};
use crate::update::edit::rotate_draft_delta;

fn without_draft(model: &Model) -> Model {
    Model { draft: None, ..model.clone() }
}

/// `{ ...model, draft: null, byId: { ...model.byId, [id]: record } }`.
fn with_changed(model: &Model, id: &str, record: Option<ModelAnnotation>) -> Model {
    let mut by_id = (*model.by_id).clone();
    let next = match record {
        Some(r) => Rc::new(r),
        None => model.by_id.get(id).cloned().expect("a record"),
    };
    by_id.insert(id.to_string(), next);
    Model { draft: None, by_id: Rc::new(by_id), ..model.clone() }
}

/// `editUp(model)`.
pub fn edit_up(model: &Model) -> CoreResult<(Model, Vec<Effect>)> {
    let draft = model.draft().expect("a draft").clone();
    let done = |m: Model| Ok((m, Vec::new()));
    match &draft {
        Draft::Leader(d) => {
            let record = model.record(&d.id);
            let measure = record.and_then(|r| measurement_of(&r.annotation));
            let Some((record, measure)) = record.zip(measure).filter(|(_, m)| m.is_distance())
            else {
                return done(without_draft(model));
            };
            if d.delta == 0.0 {
                return done(without_draft(model));
            }
            let mut leader = json::get(&measure.0, "leader")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            leader.insert("length".into(), num(measure.leader_length() + d.delta));
            let values = json::object([("leader", Value::Object(leader))]);
            let updated = with_values(record, &values)?;
            done(with_changed(model, &record.id.clone(), updated))
        }
        Draft::Caption(d) => {
            let record = model.record(&d.id);
            let measure = record.and_then(|r| measurement_of(&r.annotation));
            let (Some(record), Some(measure)) = (record, measure) else {
                return done(without_draft(model));
            };
            if (d.delta.x == 0.0 || d.delta.x.is_nan()) && (d.delta.y == 0.0 || d.delta.y.is_nan())
            {
                return done(without_draft(model));
            }
            let style = style_of(&record.annotation);
            let (geometry, moved) =
                move_measurement_caption(&shape_of(&record.annotation), &measure, d.delta, &style);
            let updated = if moved.is_distance() {
                let offset = moved.0.get("captionOffset").cloned().unwrap_or(Value::Null);
                let mut values = json::Object::new();
                if !json::undefined(Some(&offset)) {
                    values.insert("captionOffset".into(), offset);
                }
                with_values(record, &values)?
            } else {
                with_shape(record, &geometry)?
            };
            done(with_changed(model, &record.id.clone(), updated))
        }
        Draft::Handle(d) => {
            if geom_equal(&d.base, &d.current) {
                return done(without_draft(model));
            }
            let before = model.record(&d.id).expect("a handle record");
            let stored =
                unanchored_geom(&d.current, anchor_mode_of(before).as_ref(), d.view.as_ref());
            if let Some(measure) = measurement_of(&before.annotation)
                && measure.intent() == "polygon-dimension"
                && shape_measurement_readout(&stored, &measure).unavailable()
                    == Some("invalid-geometry")
            {
                return done(without_draft(model));
            }
            let record = with_shape(before, &stored)?;
            done(with_changed(model, &d.id, record))
        }
        Draft::Rotate(d) => {
            let delta = rotate_draft_delta(model, d).delta;
            if delta.abs() < 0.01 {
                return done(without_draft(model));
            }
            let mut by_id = (*model.by_id).clone();
            for id in &d.ids {
                let Some(record) = by_id.get(id).cloned() else { continue };
                let rotated = commit_view_gesture(&record, d.view.as_ref(), |g| {
                    geom_rotate_about(g, d.pivot, delta)
                });
                if let Some(next) = with_shape(&record, &rotated)? {
                    by_id.insert(id.clone(), Rc::new(next));
                }
            }
            done(Model { by_id: Rc::new(by_id), draft: None, ..model.clone() })
        }
        Draft::Group(d) => {
            let (sx, sy) = group_resize_factors(d.base, d.current);
            if (sx - 1.0).abs() < 1e-4 && (sy - 1.0).abs() < 1e-4 {
                return done(without_draft(model));
            }
            let mut by_id = (*model.by_id).clone();
            for id in &d.ids {
                let Some(record) = by_id.get(id).cloned() else { continue };
                let scaled = commit_view_gesture(&record, d.view.as_ref(), |g| {
                    geom_scale_about(g, d.anchor, sx, sy, Some(MIN_SIZE))
                });
                if let Some(next) = with_shape(&record, &scaled)? {
                    by_id.insert(id.clone(), Rc::new(next));
                }
            }
            done(Model { by_id: Rc::new(by_id), draft: None, ..model.clone() })
        }
        Draft::Move(d) => {
            if js::hypot(d.delta.x, d.delta.y) < 0.01 {
                return done(without_draft(model));
            }
            let mut by_id = (*model.by_id).clone();
            for id in &d.ids {
                let record = by_id.get(id).cloned().expect("a moving record");
                let geometry = shape_of(&record.annotation);
                if let Some(next) = with_shape(&record, &geom_translate(&geometry, d.delta))? {
                    by_id.insert(id.clone(), Rc::new(next));
                }
            }
            done(Model { by_id: Rc::new(by_id), draft: None, ..model.clone() })
        }
        _ => done(without_draft(model)),
    }
}
