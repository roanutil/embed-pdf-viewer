//! `update/selection-edits.ts`: edits the toolbar applies to the whole
//! selection: fields, text formats, links, flags, quarter turns, reset
//! rotation, delete. A record the `editable` rule freezes is skipped.

use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::Value;

use crate::editable::{is_deletable_by_hand, is_editable_by_hand, is_transformable_by_hand};
use crate::error::CoreResult;
use crate::geometry::{geom_reset_rotation, geom_rotate_about, geom_rotation};
use crate::hit::group_union_bounds;
use crate::json::{self, Object, num};
use crate::links::link_children_of;
use crate::props::kind_takes_link;
use crate::record::{kind_of, shape_of, with_shape, with_values, writable_target};
use crate::selection::annotation_turn_pivot;
use crate::types::{Effect, FieldValues, Id, Model, ModelAnnotation, Point};
use crate::update::changes::without_records;

/// The model with `change` applied to each id people may change by hand.
fn change_records(
    model: &Model,
    ids: &[Id],
    mut change: impl FnMut(&ModelAnnotation) -> CoreResult<Option<ModelAnnotation>>,
) -> CoreResult<Model> {
    let mut by_id: Option<crate::types::ById> = None;
    for id in ids {
        let record = by_id.as_ref().unwrap_or(&model.by_id).get(id).cloned();
        let Some(record) = record else { continue };
        if !is_editable_by_hand(&model.editable, &record.annotation) {
            continue;
        }
        let Some(next) = change(&record)? else { continue };
        by_id.get_or_insert_with(|| (*model.by_id).clone()).insert(id.clone(), Rc::new(next));
    }
    Ok(match by_id {
        Some(by_id) => Model { by_id: Rc::new(by_id), ..model.clone() },
        None => model.clone(),
    })
}

/// `setFields(model, patches)`.
pub fn set_fields(model: &Model, patches: &IndexMap<Id, FieldValues>) -> CoreResult<Model> {
    let ids: Vec<Id> = patches.keys().cloned().collect();
    change_records(model, &ids, |record| with_values(record, &patches[&record.id]))
}

fn body_has(body: &Object, format: &str) -> bool {
    match format {
        "bold" => json::get_num(body, "weight").is_some_and(|w| w >= 600.0),
        "italic" => json::truthy(body.get("italic")),
        _ => body
            .get("decoration")
            .and_then(Value::as_array)
            .is_some_and(|d| d.iter().any(|x| x.as_str() == Some("underline"))),
    }
}

fn body_with(body: &Object, format: &str, on: bool) -> Object {
    let mut out = body.clone();
    match format {
        "bold" => {
            out.insert("weight".into(), num(if on { 700.0 } else { 400.0 }));
        }
        "italic" => {
            out.insert("italic".into(), Value::Bool(on));
        }
        _ => {
            let mut lines: Vec<Value> = body
                .get("decoration")
                .and_then(Value::as_array)
                .map(|d| d.iter().filter(|x| x.as_str() != Some("underline")).cloned().collect())
                .unwrap_or_default();
            if on {
                lines.push(Value::String("underline".into()));
            }
            out.insert("decoration".into(), Value::Array(lines));
        }
    }
    out
}

/// `setTextFormat(model, format, on)`.
pub fn set_text_format(model: &Model, format: &str, on: bool) -> CoreResult<Model> {
    let ids = (*model.selected).clone();
    change_records(model, &ids, |record| {
        if record.subtype() != "free-text" {
            return Ok(None);
        }
        let rich = json::get(&record.annotation, "richText")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let body = rich.get("body").and_then(Value::as_object).cloned().unwrap_or_default();
        if body_has(&body, format) == on {
            return Ok(None);
        }
        let paragraphs = rich.get("paragraphs").cloned().unwrap_or(Value::Null);
        let values = json::object([(
            "richText",
            Value::Object(json::object([
                ("body", Value::Object(body_with(&body, format, on))),
                ("paragraphs", paragraphs),
            ])),
        )]);
        with_values(record, &values)
    })
}

/// `setLink(model, target)`.
pub fn set_link(model: &Model, target: &Value) -> CoreResult<(Model, Vec<Effect>)> {
    let mut effects = Vec::new();
    let mut links = Vec::new();
    for id in model.selected.iter() {
        let Some(record) = model.record(id) else { continue };
        if !is_transformable_by_hand(&model.editable, record)
            || !kind_takes_link(&kind_of(&record.annotation))
        {
            continue;
        }
        if record.subtype() == "link" {
            links.push(id.clone());
        } else {
            effects.push(Effect::sync_link(id, target.clone()));
        }
    }
    if json::truthy(Some(target)) && writable_target(Some(target)).is_null() {
        return Ok((model.clone(), effects));
    }
    let values = json::object([("target", target.clone())]);
    Ok((change_records(model, &links, |record| with_values(record, &values))?, effects))
}

/// `setFlags(model, patch, ids)`.
pub fn set_flags(model: &Model, patch: &Object, ids: Option<&[Id]>) -> CoreResult<Model> {
    let ids: Vec<Id> = ids.map(<[Id]>::to_vec).unwrap_or_else(|| (*model.selected).clone());
    change_records(model, &ids, |record| with_values(record, patch))
}

/// `rotateSelection(model, deltaDeg)`.
pub fn rotate_selection(model: &Model, delta_deg: f64) -> CoreResult<Model> {
    let ids: Vec<Id> = model
        .selected
        .iter()
        .filter(|id| {
            model.record(id).is_some_and(|r| {
                is_transformable_by_hand(&model.editable, r)
                    && kind_of(&r.annotation).caps.rotatable
            })
        })
        .cloned()
        .collect();
    if ids.is_empty() {
        return Ok(model.clone());
    }
    let pivot = if ids.len() == 1 {
        annotation_turn_pivot(model.record(&ids[0]).expect("a record"), None)
    } else {
        let page = model.record(&ids[0]).expect("a record").page();
        let narrowed = Model { selected: Rc::new(ids.clone()), ..model.clone() };
        let Some(union) = group_union_bounds(&narrowed, &page, None) else {
            return Ok(model.clone());
        };
        Point { x: union.x + union.width / 2.0, y: union.y + union.height / 2.0 }
    };
    let mut by_id = (*model.by_id).clone();
    for id in &ids {
        let record = by_id.get(id).cloned().expect("a record");
        let geometry = shape_of(&record.annotation);
        if let Some(next) = with_shape(&record, &geom_rotate_about(&geometry, pivot, delta_deg))? {
            by_id.insert(id.clone(), Rc::new(next));
        }
    }
    Ok(Model { by_id: Rc::new(by_id), ..model.clone() })
}

/// `resetRotation(model)`.
pub fn reset_rotation(model: &Model) -> CoreResult<Model> {
    let mut by_id = (*model.by_id).clone();
    let mut turned = false;
    for id in model.selected.iter() {
        let Some(record) = by_id.get(id).cloned() else { continue };
        if !is_transformable_by_hand(&model.editable, &record)
            || !kind_of(&record.annotation).caps.rotatable
        {
            continue;
        }
        let geometry = shape_of(&record.annotation);
        if geom_rotation(&geometry) == 0.0 {
            continue;
        }
        let pivot = annotation_turn_pivot(&record, None);
        if let Some(next) = with_shape(&record, &geom_reset_rotation(&geometry, Some(pivot)))? {
            by_id.insert(id.clone(), Rc::new(next));
        }
        turned = true;
    }
    Ok(if turned { Model { by_id: Rc::new(by_id), ..model.clone() } } else { model.clone() })
}

/// `deleteSelection(model)`.
pub fn delete_selection(model: &Model) -> (Model, Vec<Effect>) {
    let deletable: Vec<Id> = model
        .selected
        .iter()
        .filter(|id| model.record(id).is_some_and(|r| is_deletable_by_hand(&model.editable, r)))
        .cloned()
        .collect();
    if deletable.is_empty() {
        return (model.clone(), Vec::new());
    }
    let mut with_children = deletable.clone();
    for id in &deletable {
        with_children.extend(link_children_of(model, id).into_iter().map(|r| r.id.clone()));
    }
    let effects = with_children
        .iter()
        .filter(|id| model.record(id).is_some())
        .map(|id| Effect::delete(id))
        .collect();
    (Model { draft: None, ..without_records(model, &with_children) }, effects)
}
