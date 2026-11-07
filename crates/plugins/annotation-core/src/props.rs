//! `props.ts`: the kind table's style-panel properties over a selection.

use serde_json::Value;

use crate::editable::is_editable_by_hand;
use crate::engine::siblings::{can_change_kind, sibling_kinds_of};
use crate::json::{self, Object};
use crate::kinds::{AnnotationKind, flag_properties, kind_choice};
use crate::record::kind_of;
use crate::types::{EditableKinds, TextStyle};

/// `initialTextStyle`.
pub fn initial_text_style() -> TextStyle {
    TextStyle {
        font_family: Some(Value::String("helvetica".into())),
        font_size: Some(json::num(14.0)),
        font_color: Some(Value::String("#000000".into())),
        text_align: Some(Value::String("left".into())),
        bold: None,
        italic: None,
        underline: None,
        order: Default::default(),
    }
}

fn key_of(property: &Value) -> Option<&str> {
    property.get("key").and_then(Value::as_str)
}

/// `kindTakesLink(kind)`: does its property table declare `link`?
pub fn kind_takes_link(kind: &AnnotationKind) -> bool {
    kind.own_properties().iter().any(|p| key_of(p) == Some("link"))
}

/// `propertiesOf(kind)`: its own properties, then the flags.
pub fn properties_of(kind: &AnnotationKind) -> Vec<Value> {
    let mut out = kind.own_properties();
    out.extend(flag_properties());
    out
}

/// `sharedProperties(kinds)`: the properties every kind has, in the first's order.
pub fn shared_properties(kinds: &[AnnotationKind]) -> Vec<Value> {
    let mut unique: Vec<&AnnotationKind> = Vec::new();
    for kind in kinds {
        if !unique.contains(&kind) {
            unique.push(kind);
        }
    }
    let Some(first) = unique.first() else { return Vec::new() };
    let first = properties_of(first);
    if unique.len() == 1 {
        return first;
    }
    let rest: Vec<Vec<String>> = unique[1..]
        .iter()
        .map(|k| properties_of(k).iter().filter_map(|p| key_of(p).map(str::to_string)).collect())
        .collect();
    first
        .into_iter()
        .filter(|p| rest.iter().all(|keys| key_of(p).is_some_and(|k| keys.iter().any(|x| x == k))))
        .collect()
}

/// `kindChoiceOf(model, annotations)`: the kind choice over `annotations`
/// (`subtype`), offered when they are kinds of one sibling group and each can
/// change kind now, listing the kinds of the group people may change by hand.
fn kind_choice_of(editable: &EditableKinds, annotations: &[&Object]) -> Option<Value> {
    let first = annotations.first()?;
    let own = json::get_str(first, "subtype").unwrap_or("");
    let mut group = vec![own];
    group.extend(sibling_kinds_of(own));
    let changeable = annotations.iter().all(|annotation| {
        let subtype = json::get_str(annotation, "subtype").unwrap_or("");
        group.contains(&subtype)
            && sibling_kinds_of(subtype).iter().any(|kind| can_change_kind(annotation, kind))
    });
    if !changeable {
        return None;
    }
    let kinds: Vec<&str> = group
        .into_iter()
        .filter(|kind| {
            let mut retyped = (*first).clone();
            retyped.insert("subtype".into(), Value::String((*kind).into()));
            is_editable_by_hand(editable, &retyped)
        })
        .collect();
    (kinds.len() > 1).then(|| kind_choice(&kinds))
}

/// `selectionProperties(model, annotations)`: the properties the selected
/// annotations' kinds share, led by the kind they can become when they can.
pub fn selection_properties(editable: &EditableKinds, annotations: &[&Object]) -> Vec<Value> {
    let kinds: Vec<AnnotationKind> = annotations.iter().map(|a| kind_of(a)).collect();
    let shared = shared_properties(&kinds);
    match kind_choice_of(editable, annotations) {
        Some(choice) => std::iter::once(choice).chain(shared).collect(),
        None => shared,
    }
}
