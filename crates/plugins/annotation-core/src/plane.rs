//! `plane.ts`: the paint/conversation plane boundary, one predicate every
//! page surface filters through.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::Annot;

/// The members of an annotation the plane predicates read.
pub trait PlaneFields {
    fn irt(&self) -> Option<&str>;
    fn group(&self) -> Option<&str>;
    fn subtype(&self) -> &str;
    fn data(&self) -> Option<&Value>;
}

impl PlaneFields for Annot {
    fn irt(&self) -> Option<&str> {
        self.irt.as_deref()
    }
    fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }
    fn subtype(&self) -> &str {
        &self.subtype
    }
    fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }
}

/// `Pick<Annot, 'irt' | 'group' | 'subtype' | 'data'>` as a value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlaneParts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub irt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default)]
    pub subtype: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl PlaneFields for PlaneParts {
    fn irt(&self) -> Option<&str> {
        self.irt.as_deref()
    }
    fn group(&self) -> Option<&str> {
        self.group.as_deref()
    }
    fn subtype(&self) -> &str {
        &self.subtype
    }
    fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }
}

fn non_empty(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::String(s)) if !s.is_empty())
}

/// A reply (`irt` without `group`) or an ISO 32000 state annotation (a text
/// annotation whose DTO carries a non-empty `/State` or `/StateModel`).
pub fn is_conversation_only(a: &impl PlaneFields) -> bool {
    if a.irt().is_some() && a.group().is_none() {
        return true;
    }
    if a.subtype() == "text"
        && let Some(data) = a.data()
        && data.get("subtype").and_then(Value::as_str) == Some("text")
        && (non_empty(data.get("state")) || non_empty(data.get("stateModel")))
    {
        return true;
    }
    false
}

/// A link child grouped under a non-link parent.
pub fn is_attached_link(a: &impl PlaneFields) -> bool {
    a.subtype() == "link" && a.group().is_some()
}

/// The one page-surface cull: conversation members and attached links.
pub fn is_substrate_only(a: &impl PlaneFields) -> bool {
    is_conversation_only(a) || is_attached_link(a)
}
