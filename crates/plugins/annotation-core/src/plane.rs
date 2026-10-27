//! `plane.ts`: the paint/conversation plane boundary.

use crate::json::{self, Object};

fn non_empty(annotation: &Object, name: &str) -> bool {
    json::get_str(annotation, name).is_some_and(|s| !s.is_empty())
}

fn reply_type(annotation: &Object) -> Option<Option<&str>> {
    json::get(annotation, "reply")
        .map(|reply| reply.get("type").and_then(serde_json::Value::as_str))
}

/// `isConversationOnly(record)`: a reply, a review state, or a popup.
pub fn is_conversation_only(annotation: &Object) -> bool {
    if let Some(ty) = reply_type(annotation)
        && ty != Some("group")
    {
        return true;
    }
    let subtype = json::get_str(annotation, "subtype");
    if subtype == Some("text")
        && (non_empty(annotation, "state") || non_empty(annotation, "stateModel"))
    {
        return true;
    }
    subtype == Some("popup")
}

/// `isAttachedLink(record)`: a `/Link` grouped under a parent.
pub fn is_attached_link(annotation: &Object) -> bool {
    json::get_str(annotation, "subtype") == Some("link")
        && reply_type(annotation) == Some(Some("group"))
}

/// `isSubstrateOnly(record)`: never a page visual of its own.
pub fn is_substrate_only(annotation: &Object) -> bool {
    is_conversation_only(annotation) || is_attached_link(annotation)
}
