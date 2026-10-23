//! `props.ts`: the kind table's style-panel properties over a selection.

use serde_json::Value;

use crate::json;
use crate::kinds::{AnnotationKind, flag_properties};
use crate::types::TextStyle;

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
