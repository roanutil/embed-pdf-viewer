//! `update/session.ts`: the session's initial value, the tool defaults new
//! annotations take, and dropping its references to records that leave the
//! view (`forget`).

use std::rc::Rc;

use indexmap::IndexMap;

use crate::json::{self, Object};
use crate::record::read_of_defaults;
use crate::types::{ById, Draft, EditableKinds, FieldValues, Id, Model, SnapSettings};

pub use crate::shapes::points::line_endings_of;

/// The session before any record or gesture (`initialSession`), as a model over no records (`initialModel`).
pub fn initial_model() -> Model {
    Model {
        selected: Rc::new(Vec::new()),
        hovered: None,
        draft: None,
        preview: None,
        object_numbers: Rc::new(Vec::new()),
        defaults: Rc::new(IndexMap::new()),
        hit_margin: 6.0,
        editing: None,
        snap: Rc::new(SnapSettings {
            alignment: true,
            alignment_threshold: 6.0,
            rotation: true,
            rotation_angles: vec![0.0, 90.0, 180.0, 270.0],
            rotation_threshold: 4.0,
            order: Default::default(),
        }),
        editable: Rc::new(EditableKinds::All(true)),
        by_id: Rc::new(ById::new()),
        order: Rc::new(Vec::new()),
    }
}

/// `defaultsFor(model, preset)`: a tool's defaults, by its preset.
pub fn defaults_for(model: &Model, preset: &str) -> FieldValues {
    model.defaults.get(preset).cloned().unwrap_or_default()
}

/// `toolAnnotation(model, kind, preset)`: what a create from a tool starts from.
pub fn tool_annotation(model: &Model, kind: &str, preset: Option<&str>) -> Object {
    read_of_defaults(kind, &defaults_for(model, preset.unwrap_or(kind)))
}

/// `setDefaults(model, preset, patch)`.
pub fn set_defaults(model: &Model, preset: &str, patch: &FieldValues) -> Model {
    let next = json::spread(&defaults_for(model, preset), patch);
    let mut defaults = (*model.defaults).clone();
    defaults.insert(preset.to_string(), next);
    Model { defaults: Rc::new(defaults), ..model.clone() }
}

fn draft_ids(draft: Option<&Draft>) -> Vec<Id> {
    draft.map(|d| d.ids().into_iter().cloned().collect()).unwrap_or_default()
}

/// `forget(model, ids)`: drop every session reference to these records; a
/// gesture on one of them ends. The same model when none is referenced.
pub fn forget(model: &Model, ids: &[Id]) -> Model {
    // A set, as the TypeScript's `new Set(ids)`: forgetting many records stays linear.
    let gone_set: std::collections::HashSet<&str> = ids.iter().map(String::as_str).collect();
    let gone = |id: &str| gone_set.contains(id);
    let selected: Vec<Id> = model.selected.iter().filter(|id| !gone(id)).cloned().collect();
    let hovered = model.hovered.clone().filter(|h| !gone(h));
    let editing = model.editing.clone().filter(|e| !gone(e));
    let draft_gone = draft_ids(model.draft()).iter().any(|id| gone(id));
    if selected.len() == model.selected.len()
        && hovered == model.hovered
        && editing == model.editing
        && !draft_gone
    {
        return model.clone();
    }
    Model {
        selected: Rc::new(selected),
        hovered,
        editing,
        draft: if draft_gone { None } else { model.draft.clone() },
        ..model.clone()
    }
}
