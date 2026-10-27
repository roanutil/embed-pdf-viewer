//! `links.ts`: attached link children presented as their parent's `link`.

use serde_json::Value;

use crate::json;
use crate::plane::is_attached_link;
use crate::record::group_of;
use crate::types::{Model, ModelAnnotation};

/// `linkChildrenOf(model, parentId)`: in z-order.
pub fn link_children_of<'a>(model: &'a Model, parent_id: &str) -> Vec<&'a ModelAnnotation> {
    model
        .order
        .iter()
        .filter_map(|id| model.record(id))
        .filter(|r| {
            is_attached_link(&r.annotation) && group_of(&r.annotation).as_deref() == Some(parent_id)
        })
        .collect()
}

/// `linkOf(model, parentId)`: the target of its first attached child.
pub fn link_of(model: &Model, parent_id: &str) -> Value {
    match link_children_of(model, parent_id).first() {
        Some(child) if json::get_str(&child.annotation, "subtype") == Some("link") => {
            json::get(&child.annotation, "target").cloned().unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}
