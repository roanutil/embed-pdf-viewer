//! `write/drafts.ts`: creation drafts. The polygon or polyline a click per
//! point builds (and the buffered ink strokes), its finish and cancel doors,
//! and the captured-draft seam the measurement plugin calibrates from.

use std::cell::RefCell;
use std::rc::Rc;

use epdf_annotation_core::object::Json;
use epdf_annotation_core::types::{CapturedEffect, Draft, Model, Shape};
use epdf_annotation_core::view::creation_draft_anchor;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Value, json};

use crate::controller::Core;
use crate::write::outcomes::created_ref_of;

#[derive(Default)]
pub struct DraftsState {
    anchor: RefCell<Option<(Rc<Model>, Value)>>,
}

impl Core {
    /// The `captured` effect: a capture-only tool reports its line.
    pub(crate) fn capture(&self, effect: &CapturedEffect) {
        if let Shape::Line(line) = &effect.geometry {
            self.events.draft_captured.emit(&json!({
                "tool": effect.tool,
                "page": effect.page.json(),
                "from": line.line_points.start.json(),
                "to": line.line_points.end.json(),
            }));
        }
    }

    /// `draft.get()`: the shape being drawn, the same object until it changes.
    pub fn draft_get(&self) -> Value {
        let model = self.model();
        if let Some((m, v)) = &*self.drafts.anchor.borrow()
            && Rc::ptr_eq(m, &model)
        {
            return v.clone();
        }
        let anchor = creation_draft_anchor(&model).map_or(Value::Null, |a| a.json());
        *self.drafts.anchor.borrow_mut() = Some((model, anchor.clone()));
        anchor
    }

    /// `draft.finish(options)`: end the draft as a double-click does.
    pub fn draft_finish(&self, signal: Option<AbortSignal>) -> Promise<Value> {
        let model = self.model();
        let Some(draft) = model.draft() else { return Promise::resolved(Value::Null) };
        if !draft.is_create() {
            return Promise::resolved(Value::Null);
        }
        let ink = matches!(draft, Draft::CreateInk(_));
        let tool_id = self.active_tool().map(|t| t.id.clone());
        let message = json!({ "type": if ink { "finishInkDraft" } else { "finishCreationDraft" } });
        let commit =
            match self.commit(message, Some(self.after_create_shape(tool_id.clone())), None) {
                Ok(c) => c,
                Err(e) => return Promise::rejected(e),
            };
        self.after_create_done(tool_id.as_deref(), &commit.effects);
        if !commit
            .effects
            .iter()
            .any(|e| matches!(e, epdf_annotation_core::types::Effect::Create(_)))
        {
            return Promise::resolved(Value::Null);
        }
        let created = self.cancellable(signal, created_ref_of(&commit));
        let me = self.weak();
        Promise::spawn(async move {
            let reference = created.await?;
            let Some(core) = me.upgrade() else { return Ok(Value::Null) };
            Ok(match core.get(&reference) {
                Some(annotation) => json!({ "annotation": annotation }),
                None => Value::Null,
            })
        })
    }

    /// `draft.cancel()` and the capability's `cancel()`.
    pub fn cancel(&self) {
        self.commit_or_log(json!({ "type": "cancel" }));
    }

    /// `finishInkDraft()`.
    pub fn finish_ink_draft(&self) {
        let tool_id = self.active_tool().map(|t| t.id.clone());
        if let Ok(commit) = self.commit(
            json!({ "type": "finishInkDraft" }),
            Some(self.after_create_shape(tool_id.clone())),
            None,
        ) {
            self.after_create_done(tool_id.as_deref(), &commit.effects);
        }
    }

    /// `distanceCreationPage()`: the page a distance draft's offset step is on.
    pub fn distance_creation_page(&self) -> Option<Value> {
        let model = self.model();
        match model.draft() {
            Some(Draft::CreateDistance(d)) if d.step == "offset" => Some(d.page.json()),
            _ => None,
        }
    }
}
