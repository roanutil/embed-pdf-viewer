//! `update/`: the pure annotation core. `update(model, message)` returns
//! the next session, the records the message changed, and the engine work
//! to do. The core never stores a record.
//!
//! H3: a record that leaves the view mid-gesture is dropped from the
//! session by `forget` (`session.rs`), which ends a gesture on it, so no
//! later message works on a record that is gone. A record this session
//! creates has its final ref from the start (the object number it takes),
//! so nothing is renamed when the engine confirms it.

pub mod changes;
pub mod draw;
pub mod draw_callout;
pub mod draw_distance;
pub mod edit;
pub mod edit_commit;
pub mod marquee;
pub mod page_bound;
pub mod selection_edits;
pub mod session;
pub mod text;
pub mod text_markup;

use std::rc::Rc;

use indexmap::IndexMap;

use crate::appearance::apply_change;
use crate::editable::is_contents_editable_by_hand;
use crate::engine::resolve::annotation_patch_between;
use crate::error::CoreResult;
use crate::group::expand_groups;
use crate::hit::is_selectable;
use crate::json::{self, Object};
use crate::object::Json;
use crate::types::{ChangeSet, Effect, Id, Message, Model, SnapSettings, UpdateResult};

pub use draw::{draft_placement, finish_ink_create, finish_poly_create};
pub use draw_callout::{callout_box, callout_upright_rot};
pub use edit::{RotateDelta, rotate_draft_delta};
pub use marquee::{annots_in_box, selection_in_box};
pub use session::{defaults_for, forget, initial_model, line_endings_of, tool_annotation};

/// `update(model, message)`.
pub fn update(model: &Model, message: &Message) -> CoreResult<UpdateResult> {
    let (next, effects) = transition(model, message)?;
    let change = change_between(model, &next)?;
    let mut all = effects.clone();
    all.extend(writes_of(&change, &effects));
    Ok(UpdateResult { session: next.session(), change, effects: all })
}

/// `newRecordsAtMost(message)`: the object numbers a message may need.
pub fn new_records_at_most(message: &Message) -> usize {
    match message {
        Message::CreateReplaceText { .. } => 2,
        Message::CreatePointer(m) => usize::from(m.phase != crate::types::Phase::Move),
        Message::FinishInkDraft
        | Message::FinishCreationDraft
        | Message::CreateCaret { .. }
        | Message::CreateMarkup { .. } => 1,
        _ => 0,
    }
}

fn writes_of(change: &ChangeSet, effects: &[Effect]) -> Vec<Effect> {
    let typed: Vec<&Id> = effects
        .iter()
        .filter_map(|e| if let Effect::Text(t) = e { Some(&t.id) } else { None })
        .collect();
    change
        .put
        .iter()
        .filter_map(|record| {
            let patch = change.patches.get(&record.id)?;
            (!typed.contains(&&record.id)).then(|| Effect::patch(&record.id, patch.clone()))
        })
        .collect()
}

/// `changeBetween(before, after)`: the records a transition changed.
fn change_between(before: &Model, after: &Model) -> CoreResult<ChangeSet> {
    if Rc::ptr_eq(&before.by_id, &after.by_id) {
        return Ok(ChangeSet::default());
    }
    let mut put = Vec::new();
    let mut patches = IndexMap::new();
    for id in after.order.iter() {
        let Some(record) = after.by_id.get(id) else { continue };
        let previous = before.by_id.get(id);
        if previous.is_some_and(|p| Rc::ptr_eq(p, record)) {
            continue;
        }
        let Some(previous) = previous else {
            put.push(record.clone());
            continue;
        };
        let fields = annotation_patch_between(&previous.annotation, &record.annotation);
        if fields.is_empty() {
            continue;
        }
        let mut patch = fields;
        patch.insert("subtype".into(), serde_json::Value::String(previous.subtype().into()));
        put.push(Rc::new(apply_change(previous, &patch)?));
        patches.insert(id.clone(), patch);
    }
    let drop: Vec<Id> = before
        .order
        .iter()
        .filter(|id| before.by_id.contains_key(*id) && !after.by_id.contains_key(*id))
        .cloned()
        .collect();
    if put.is_empty() && drop.is_empty() {
        return Ok(ChangeSet::default());
    }
    Ok(ChangeSet { put, drop, patches })
}

fn none(model: Model) -> CoreResult<(Model, Vec<Effect>)> {
    Ok((model, Vec::new()))
}

fn transition(model: &Model, message: &Message) -> CoreResult<(Model, Vec<Effect>)> {
    match message {
        Message::EditPointer { phase, input } => edit::edit_pointer(model, *phase, input),
        Message::MarqueePointer { phase, input } => {
            Ok(marquee::marquee_pointer(model, *phase, input))
        }
        Message::CreatePointer(m) => draw::create_pointer(model, m),
        Message::FinishInkDraft => draw::finish_ink_create(model),
        Message::FinishCreationDraft => draw::finish_poly_create(model),
        Message::CreateCaret { page, anchor, flags } => {
            text_markup::create_caret(model, page, anchor, flags.as_ref())
        }
        Message::CreateReplaceText { page, quads, anchor, preset } => {
            text_markup::create_replace_text(model, page, quads, anchor, preset.as_deref())
        }
        Message::CreateMarkup { subtype, page, quads, preset, flags } => {
            text_markup::create_markup(
                model,
                subtype,
                page,
                quads,
                preset.as_deref(),
                flags.as_ref(),
            )
        }
        Message::SetMarkupPreview { subtype, quads_by_page, preset } => {
            none(text_markup::set_markup_preview(model, subtype, quads_by_page, preset.as_deref()))
        }
        Message::ClearMarkupPreview => none(if model.preview.is_some() {
            Model { preview: None, ..model.clone() }
        } else {
            model.clone()
        }),
        Message::Select { ids, add } => {
            let selectable: Vec<Id> =
                ids.iter().filter(|id| is_selectable(model, id)).cloned().collect();
            let ids = expand_groups(model, &selectable);
            if ids.is_empty() {
                return none(model.clone());
            }
            let selected = if *add == Some(true) {
                let mut out: Vec<Id> = Vec::new();
                for id in model.selected.iter().chain(ids.iter()) {
                    if !out.contains(id) {
                        out.push(id.clone());
                    }
                }
                out
            } else {
                ids
            };
            none(Model { selected: Rc::new(selected), ..model.clone() })
        }
        Message::Deselect { ids } => {
            if model.selected.is_empty() {
                return none(model.clone());
            }
            let Some(ids) = ids else {
                return none(Model { selected: Rc::new(Vec::new()), ..model.clone() });
            };
            let selected: Vec<Id> =
                model.selected.iter().filter(|id| !ids.contains(id)).cloned().collect();
            none(if selected.len() == model.selected.len() {
                model.clone()
            } else {
                Model { selected: Rc::new(selected), ..model.clone() }
            })
        }
        Message::SetFields { patches } => none(selection_edits::set_fields(model, patches)?),
        Message::SetTextFormat { format, on } => {
            none(selection_edits::set_text_format(model, format, *on)?)
        }
        Message::SetLink { target } => selection_edits::set_link(model, target),
        Message::SetFlags { patch, ids } => {
            none(selection_edits::set_flags(model, patch, ids.as_deref())?)
        }
        Message::SetDefaults { preset, patch } => none(session::set_defaults(model, preset, patch)),
        Message::SetSnap { patch } => {
            let merged: Object = json::spread(&model.snap.to_object(), patch);
            let mut snap = SnapSettings::read(&serde_json::Value::Object(merged))
                .unwrap_or_else(|_| (*model.snap).clone());
            snap.order = model.snap.order.clone();
            none(Model { snap: Rc::new(snap), ..model.clone() })
        }
        Message::RotateSelection { degrees } => {
            none(selection_edits::rotate_selection(model, *degrees)?)
        }
        Message::ResetRotation => none(selection_edits::reset_rotation(model)?),
        Message::Delete => Ok(selection_edits::delete_selection(model)),
        Message::Cancel => none(Model { draft: None, ..model.clone() }),
        Message::Forget { ids } => none(session::forget(model, ids)),
        Message::Hover { id } => none(if model.hovered == *id {
            model.clone()
        } else {
            Model { hovered: id.clone(), ..model.clone() }
        }),
        Message::BeginTextEdit { id } => {
            let editable =
                model.record(id).is_some_and(|r| is_contents_editable_by_hand(&model.editable, r));
            none(if editable {
                Model {
                    editing: Some(id.clone()),
                    selected: Rc::new(vec![id.clone()]),
                    draft: None,
                    ..model.clone()
                }
            } else {
                model.clone()
            })
        }
        Message::SetText { id, text } => Ok(text::set_text(model, id, text)),
        Message::SetRichText { id, doc } => Ok(text::set_rich_text(model, id, doc)),
        Message::EndTextEdit => none(if model.editing.is_some() {
            Model { editing: None, ..model.clone() }
        } else {
            model.clone()
        }),
    }
}
