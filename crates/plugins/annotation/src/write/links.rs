//! `write/links.ts`: attached links (a link child riding an editable
//! annotation), the one reconciler that creates, retargets, re-rects and
//! deletes link children. Its changes join the change that made them
//! needed: a restyle that set a link, a move of the parent.

use epdf_annotation_core::links::{link_children_of, link_of};
use epdf_annotation_core::record::{
    kind_of, link_child_rects, shape_of, style_of, writable_target,
};
use epdf_annotation_core::types::Model;
use epdf_kernel::rt::Promise;
use epdf_kernel::signal::AbortSignal;
use serde_json::{Value, json};

use crate::controller::Core;
use crate::js::{Object, key_or_none};
use crate::services::staging::{StoreChange, op_of_stated};
use crate::write::outcomes::applied_or_throw;

impl Core {
    /// `childChanges(id, desired, model)`: the changes that bring the
    /// attached link children of `id` to `desired`, read from `model`.
    pub(crate) fn child_changes(
        &self,
        id: &str,
        desired: &Value,
        model: &Model,
    ) -> Vec<StoreChange> {
        let Some(record) = model.by_id.get(id) else { return Vec::new() };
        let Some(parent) = record.annotation.get("ref").cloned() else { return Vec::new() };
        if kind_of(&record.annotation).name == "link" {
            return Vec::new();
        }
        let target = writable_target(Some(desired).filter(|d| !d.is_null()));
        let rects = if desired.is_null() || epdf_kernel::is_undefined(desired) {
            Vec::new()
        } else {
            link_child_rects(&shape_of(&record.annotation), &style_of(&record.annotation))
        };
        let current = link_children_of(model, id);
        let mut changes = Vec::new();
        let paired = current.len().min(rects.len());
        for i in 0..paired {
            let mut patch = Object::new();
            patch.insert("subtype".into(), json!("link"));
            patch.insert("rect".into(), epdf_annotation_core::json::rect_value(rects[i]));
            if !target.is_null() {
                patch.insert("target".into(), target.clone());
            }
            changes.push(StoreChange::Update {
                reference: current[i].annotation.get("ref").cloned().unwrap_or(Value::Null),
                patch,
                resources: None,
            });
        }
        for rect in rects.iter().skip(current.len()) {
            let draft = json!({
                "subtype": "link",
                "rect": epdf_annotation_core::json::rect_value(*rect),
                "target": target,
                "reply": { "to": parent, "type": "group" },
            });
            changes.push(StoreChange::Create {
                page: record.annotation.get("page").cloned().unwrap_or(Value::Null),
                draft: draft.as_object().cloned().unwrap_or_default(),
                resources: None,
            });
        }
        for child in current.iter().skip(rects.len()) {
            changes.push(StoreChange::Delete {
                reference: child.annotation.get("ref").cloned().unwrap_or(Value::Null),
            });
        }
        changes
    }

    /// The store's follow-up: an update of a parent, from either door, brings
    /// its children along toward the target they hold.
    pub(crate) fn follow_up(&self, op: &Value, model: &Model) -> Vec<Value> {
        let id = key_or_none(op.get("ref").unwrap_or(&Value::Null));
        if link_children_of(model, &id).is_empty() {
            return Vec::new();
        }
        let target = link_of(model, &id);
        self.child_changes(&id, &target, model).iter().map(op_of_stated).collect()
    }

    /// `setLink(ref, target, options)`.
    fn set_link(
        &self,
        reference: &Value,
        target: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        let changes = self.child_changes(&key_or_none(reference), &target, &self.model());
        if changes.is_empty() {
            return Promise::resolved(());
        }
        let applied = self.apply_when_numbered(changes, false);
        let task = Promise::spawn(async move {
            let applied = applied.await?;
            applied_or_throw(&applied).await?;
            Ok(())
        });
        self.cancellable(signal, task)
    }

    /// `links.get(ref)`.
    pub fn links_get(&self, reference: &Value) -> Value {
        let model = self.model();
        let Some(record) = model.by_id.get(&key_or_none(reference)) else { return Value::Null };
        if record.annotation.get("subtype").and_then(Value::as_str) == Some("link") {
            return record
                .annotation
                .get("target")
                .cloned()
                .filter(|t| !epdf_kernel::is_undefined(t))
                .unwrap_or(Value::Null);
        }
        link_of(&model, &record.id)
    }

    /// `links.set(ref, target, options)`.
    pub fn links_set(
        &self,
        reference: &Value,
        target: Value,
        signal: Option<AbortSignal>,
    ) -> Promise<()> {
        self.set_link(reference, target, signal)
    }

    /// `links.clear(ref, options)`.
    pub fn links_clear(&self, reference: &Value, signal: Option<AbortSignal>) -> Promise<()> {
        self.set_link(reference, Value::Null, signal)
    }
}
