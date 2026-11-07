//! `annotation/siblings.ts`: kinds that take the same fields, so an update
//! turns one into another by its `subtype` and the annotation stays itself.

use serde_json::Value;

use super::facts::kind_facts_of;
use crate::json::{self, Object};

/// `SIBLING_KINDS`: ISO 32000-2 describes each group with one table of
/// entries: the text markup subtypes (12.5.6.10), and Square and Circle
/// (12.5.6.8). Every other kind keeps its subtype.
pub const SIBLING_KINDS: [&[&str]; 2] =
    [&["highlight", "underline", "squiggly", "strikeout"], &["square", "circle"]];

/// `siblingKindsOf(subtype)`: the kinds an annotation of `subtype` can
/// become, the others of its group, or none.
pub fn sibling_kinds_of(subtype: &str) -> Vec<&'static str> {
    SIBLING_KINDS
        .iter()
        .find(|kinds| kinds.contains(&subtype))
        .map(|kinds| kinds.iter().copied().filter(|kind| *kind != subtype).collect())
        .unwrap_or_default()
}

/// `fieldTheKindCannotHold(annotation, subtype)`: the first field
/// `annotation` holds a value in that the kind `subtype` has no field for.
pub fn field_the_kind_cannot_hold(annotation: &Object, subtype: &str) -> Option<&'static str> {
    let kept = kind_facts_of(subtype);
    let own = kind_facts_of(json::get_str(annotation, "subtype").unwrap_or(""))?;
    own.owners.iter().map(|(name, _)| *name).find(|name| {
        !kept.is_some_and(|k| k.declares(name)) && !json::nullish(annotation.get(*name))
    })
}

/// `canChangeKind(annotation, subtype)`: a sibling kind, with no value it
/// can't hold.
pub fn can_change_kind(annotation: &Object, subtype: &str) -> bool {
    sibling_kinds_of(json::get_str(annotation, "subtype").unwrap_or("")).contains(&subtype)
        && field_the_kind_cannot_hold(annotation, subtype).is_none()
}

/// `canChangeKind` over a JSON value: one that isn't an annotation object
/// can change to nothing.
pub fn can_change_kind_value(annotation: &Value, subtype: &str) -> bool {
    annotation.as_object().is_some_and(|a| can_change_kind(a, subtype))
}
