//! `services/authority.ts`: authorization, the engine's own collab resolver
//! mirrored. `canCreate` asks about the caller's own identity; the mutation
//! checks ask about the target's stamped owner. The twins answer "would the
//! verb succeed?" through the same fused predicates the gestures and chrome
//! consume (permissions.md).

use epdf_annotation_core::engine::copy::plan_annotation_copies;
use epdf_annotation_core::flags::{annot_deletable, annot_transformable};
use serde_json::Value;

use crate::controller::Core;
use crate::engine::deleted_with;
use crate::js::key_or_none;
use crate::read::view::owner_of;

impl Core {
    /// `canRead()`: `doc.annotate.read`.
    pub fn can_read(&self) -> bool {
        self.allows("doc.annotate.read")
    }

    /// `canCreate()`: what `assertAllowed('annotations:create', …)` refuses on.
    pub fn can_create(&self) -> bool {
        self.allows("annotations:create")
    }

    /// `allowsMutation(action, ref)`: the target's stamped owner, asked of the engine's rules.
    pub(crate) fn allows_mutation(&self, action: &str, reference: &Value) -> bool {
        let model = self.model();
        let target = model
            .by_id
            .get(&key_or_none(reference))
            .map(|r| owner_of(&Value::Object((*r.annotation).clone())))
            .unwrap_or_else(|| serde_json::json!({}));
        self.security().is_some_and(|s| s.allows_annotation(action, &target))
    }

    /// `canUpdate(ref)`: its permission and its own `locked` flag.
    pub fn can_update(&self, reference: &Value) -> bool {
        let model = self.model();
        model.by_id.get(&key_or_none(reference)).is_some_and(|r| annot_transformable(&**r))
    }

    /// `blockersOf(ref)`: what keeps `ref` from being deleted; none when the
    /// delete goes through. Deleting an annotation deletes what goes with it
    /// (`deletedWith`), and the engine checks each of them: the annotation
    /// itself as anything deleted by hand is (`annotDeletable`), what goes
    /// with it by its lock and the session's right to delete it.
    pub(crate) fn blockers_of(&self, reference: &Value) -> Vec<Value> {
        let model = self.model();
        let key = key_or_none(reference);
        if !model.by_id.contains_key(&key) {
            return vec![reference.clone()];
        }
        let page = reference.get("page").and_then(|p| p.get("objectNumber")).cloned();
        let annotations: Vec<Value> = model
            .order
            .iter()
            .filter_map(|id| model.by_id.get(id))
            .map(|r| Value::Object((*r.annotation).clone()))
            .filter(|a| a.get("page").and_then(|p| p.get("objectNumber")).cloned() == page)
            .collect();
        let refs: Vec<&Value> = annotations.iter().collect();
        deleted_with(&refs, reference)
            .into_iter()
            .map(|i| annotations[i].get("ref").cloned().unwrap_or(Value::Null))
            .filter(|member| {
                let member_key = key_or_none(member);
                let Some(record) = model.by_id.get(&member_key) else { return true };
                if member_key == key { !annot_deletable(&**record) } else { !goes_along(record) }
            })
            .collect()
    }

    /// `canDelete(ref)`: nothing keeps it, or what goes with it, from being deleted.
    pub fn can_delete(&self, reference: &Value) -> bool {
        self.blockers_of(reference).is_empty()
    }

    /// `canDuplicate(refs)`: whether a person may copy these annotations:
    /// the right to create, each one here and copied whole by the engine's
    /// own rule (`planAnnotationCopies`), and none locked. A copy of a locked
    /// annotation is locked too, so a person could neither move nor delete it.
    pub fn can_duplicate(&self, refs: &[Value]) -> bool {
        if refs.is_empty() || !self.can_create() {
            return false;
        }
        let model = self.model();
        let mut sources = Vec::with_capacity(refs.len());
        for reference in refs {
            let Some(record) = model.by_id.get(&key_or_none(reference)) else { return false };
            if epdf_annotation_core::json::truthy(record.annotation.get("locked")) {
                return false;
            }
            sources.push((*record.annotation).clone());
        }
        plan_annotation_copies(&sources, None, None, |_| false).dropped.is_empty()
    }
}

/// `goesAlong(record)`: deleted with the annotation it belongs to: unlocked,
/// and the session's to delete.
fn goes_along(record: &epdf_annotation_core::types::ModelAnnotation) -> bool {
    record.annotation.get("locked").and_then(Value::as_bool) != Some(true)
        && record.authority.as_ref().is_none_or(|a| a.delete)
}
