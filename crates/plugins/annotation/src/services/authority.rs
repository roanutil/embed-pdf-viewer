//! `services/authority.ts`: authorization, the engine's own collab resolver
//! mirrored. `canCreate` asks about the caller's own identity; the mutation
//! checks ask about the target's stamped owner. The twins answer "would the
//! verb succeed?" through the same fused predicates the gestures and chrome
//! consume (permissions.md).

use epdf_annotation_core::flags::{annot_deletable, annot_transformable};
use serde_json::Value;

use crate::controller::Core;
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

    /// `canDelete(ref)`: its permission and its own `locked` flag.
    pub fn can_delete(&self, reference: &Value) -> bool {
        let model = self.model();
        model.by_id.get(&key_or_none(reference)).is_some_and(|r| annot_deletable(&**r))
    }
}
