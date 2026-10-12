//! `services/store.ts`: the model store, the one door the annotation model
//! changes through. The pure `update` runs here so it can emit effects; the
//! next model is dispatched to the kernel store, each effect is handed to
//! the area that performs it, and the transition is announced (selection,
//! draft and editing events).
//!
//! The TypeScript registers one runner per effect kind with `onEffect`; the
//! areas' runners are fixed, so [`AnnotationCapability::run_effect`] routes
//! each kind to its area directly.

use std::rc::Rc;

use epdf_annotation_core::{AnnotationRef, Effect, Id, Model, Msg, creation_draft_anchor, update};

use crate::contract::*;
use crate::controller::AnnotationCapability;
use crate::model::AnnotationAction;

/// `refsOfIn(m, ids)`: the refs behind a list of model ids (uncommitted
/// entries have none).
pub fn refs_of_in(m: &Model, ids: &[Id]) -> Vec<AnnotationRef> {
    ids.iter().filter_map(|id| m.by_id.get(id).and_then(|a| a.ref_.clone())).collect()
}

impl AnnotationCapability {
    /// `store.commit(msg)`: run one message through `update`, store the
    /// result, run its effects, then announce the transition.
    pub(crate) fn commit(&self, msg: Msg) -> Vec<Effect> {
        let prev = self.model();
        let (next, effects) = update(&prev, &msg);
        let next = next.map_or_else(|| prev.clone(), Rc::new);
        self.cx().dispatch(AnnotationAction::SetModel { model: next.clone() });
        for fx in &effects {
            self.run_effect(fx.clone(), &next);
        }
        self.announce_transition(&prev, &next);
        effects
    }

    fn run_effect(&self, fx: Effect, m: &Rc<Model>) {
        match fx {
            Effect::Create { id } => self.run_create_effect(id, m),
            Effect::CreateGroup { primary, members } => {
                self.run_create_group_effect(primary, members, m)
            }
            Effect::Flags { id } => self.run_flags_effect(id, m),
            Effect::Patch { id, scope, .. } => self.run_patch_effect(id, scope, m),
            Effect::Delete { ref_ } => self.run_delete_effect(ref_),
            Effect::SyncLink { id, target } => self.run_sync_link_effect(id, target),
            Effect::Captured { tool, page, geom } => self.run_captured_effect(tool, page, geom),
        }
    }

    /// `announceTransition(prev, next)`.
    fn announce_transition(&self, prev: &Model, next: &Model) {
        let events = &self.0.events;
        if prev.selected != next.selected {
            events.selection_changed.emit(&AnnotationSelectionChangedEvent {
                refs: refs_of_in(next, &next.selected),
                previous_refs: refs_of_in(prev, &prev.selected),
            });
        }
        // `prev.draft !== next.draft`: identity, so a step that builds a new
        // draft announces it even when its value did not change.
        let same_draft = match (&prev.draft, &next.draft) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        if !same_draft {
            events
                .draft_changed
                .emit(&AnnotationDraftChangedEvent { draft: creation_draft_anchor(next) });
        }
        if prev.editing != next.editing {
            let r = next
                .editing
                .as_ref()
                .and_then(|id| next.by_id.get(id))
                .and_then(|a| a.ref_.clone());
            events.editing_changed.emit(&AnnotationEditingChangedEvent { ref_: r });
        }
    }
}
