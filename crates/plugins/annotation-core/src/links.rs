//! `links.ts`: the attached-link lens. Link children stay first-class
//! annotations; these derivations present them as the parent's `link`.

use serde_json::Value;

use crate::plane::is_attached_link;
use crate::types::{Annot, Model, PdfLinkTarget};

/// Every attached link child of `parent_id`, in z-order.
pub fn link_children_of<'a>(m: &'a Model, parent_id: &str) -> Vec<&'a Annot> {
    m.order
        .iter()
        .filter_map(|id| m.by_id.get(id))
        .filter(|a| is_attached_link(*a) && a.group.as_deref() == Some(parent_id))
        .collect()
}

/// The parent's link target, from its first attached child's DTO; `None`
/// when there is no child, its DTO is not a link, or it has no target.
pub fn link_of(m: &Model, parent_id: &str) -> Option<PdfLinkTarget> {
    let data = link_children_of(m, parent_id).first()?.data.as_ref()?;
    if data.get("subtype").and_then(Value::as_str) != Some("link") {
        return None;
    }
    match data.get("target") {
        None | Some(Value::Null) => None,
        Some(target) => serde_json::from_value(target.clone()).ok(),
    }
}
