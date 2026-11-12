//! `engine-core/src/transfer/annotationCopy.ts` (`planAnnotationCopies`) and
//! the two helpers it reads, `fieldMarkersOf` (`transfer/annotationImport.ts`)
//! and `translatedAnnotation` (`annotation/appearance.ts`): what a copy of
//! annotations inside one document is, decided once. The engine applies it,
//! and a viewer shows it before the engine answers. Annotations are the
//! engine's records as JSON, in page space.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use crate::json::{self, Object};
use crate::record::annotation_key;

/// One copy a plan makes (`PlannedAnnotationCopy`).
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedCopy {
    /// The annotation it copies, by its place among the ones given.
    pub source: usize,
    /// The page the copy goes on (a `PageRef`).
    pub page: Value,
    /// The source's data, moved, without its name, group and links, and
    /// without a field that marks something the engine can't write. Its
    /// `ref` is still the source's.
    pub data: Object,
    /// The copy it replies to or is grouped with, by its place in `copies`,
    /// and the reply type.
    pub reply_to: Option<(usize, Value)>,
    /// For a popup: the copy it shows, by its place in `copies`.
    pub parent: Option<usize>,
}

/// `AnnotationCopyPlan`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CopyPlan {
    /// In the order of the annotations given.
    pub copies: Vec<PlannedCopy>,
    /// `{ ref, reason }` for the annotations left out whole, in the order given.
    pub dropped: Vec<Value>,
}

/// `isWritableLinkTarget(target)`: whether a write can make a link's target.
fn is_writable_link_target(target: &Value) -> bool {
    match target.get("kind").and_then(Value::as_str) {
        Some("named") => matches!(
            target.get("name").and_then(Value::as_str),
            Some("NextPage" | "PrevPage" | "FirstPage" | "LastPage")
        ),
        Some("goto" | "uri") => true,
        _ => false,
    }
}

/// `writtenAsIs(tree)`: whether a link's `/A` is what writing its target
/// writes again.
fn written_as_is(tree: Option<&Value>) -> bool {
    let Some(tree) = tree.filter(|t| !json::nullish(Some(t))) else { return false };
    if json::truthy(tree.get("incomplete")) {
        return false;
    }
    let Some(root) = tree.get("root").filter(|r| json::truthy(Some(r))) else { return false };
    if root.get("next").and_then(Value::as_array).is_some_and(|next| !next.is_empty()) {
        return false;
    }
    !(root.get("type").and_then(Value::as_str) == Some("uri") && json::truthy(root.get("isMap")))
}

/// `fieldMarkersOf(data)`: the fields of `data` that mark something the
/// engine can't write, each with the reason a copy or import gives.
pub fn field_markers_of(data: &Object) -> Vec<(String, &'static str)> {
    let mut markers = Vec::new();
    let measure = data.get("measure").filter(|m| !json::nullish(Some(m)));
    match measure.and_then(|m| m.get("subtype")).and_then(Value::as_str) {
        Some("geospatial") => markers.push(("measure".to_string(), "geospatial")),
        Some("unknown") => markers.push(("measure".to_string(), "unknown-measure")),
        _ => {}
    }
    let link = json::get_str(data, "subtype") == Some("link");
    let target = data.get("target").filter(|t| link && json::truthy(Some(t)));
    let carried_target = target.is_some_and(is_writable_link_target);
    if target.is_some() && !carried_target {
        markers.push(("target".to_string(), "unsupported-action"));
    }
    // `actions` reads `/A` and `/AA`, and no write takes it. A copy has only
    // the `/A` a link's target writes: one go-to or URI action, without a
    // chain or an `/IsMap`.
    let lost = data.get("actions").and_then(Value::as_object).is_some_and(|actions| {
        actions.iter().any(|(trigger, tree)| {
            if !link || trigger != "activate" {
                true
            } else {
                carried_target && !written_as_is(Some(tree))
            }
        })
    });
    if lost {
        markers.push(("actions".to_string(), "unsupported-action"));
    }
    markers
}

/// `TRANSLATABLE_GEOMETRY[subtype]`: the geometry fields a kind moves by.
fn translatable_geometry(subtype: &str) -> &'static [&'static str] {
    match subtype {
        "square" | "circle" | "caret" | "stamp" | "widget" => &["box"],
        "free-text" => &["box", "calloutLine"],
        "line" => &["linePoints"],
        "polygon" | "polyline" => &["vertices"],
        "ink" => &["inkList"],
        "highlight" | "underline" | "squiggly" | "strikeout" => &["quadPoints"],
        "redact" => &["rect", "quadPoints"],
        "text" | "file-attachment" | "link" => &["rect"],
        _ => &[],
    }
}

/// `shifted(value, dx, dy)`: every number under an `x`, `left` or `right`
/// key moved by `dx`, under a `y`, `top` or `bottom` key by `dy`.
fn shifted(value: &Value, dx: f64, dy: f64) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(|each| shifted(each, dx, dy)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| {
                    let moved = match json::number(field) {
                        Some(n) => {
                            let delta = match key.as_str() {
                                "left" | "right" | "x" => dx,
                                "bottom" | "top" | "y" => dy,
                                _ => 0.0,
                            };
                            // `field + 0` is the same number.
                            if delta == 0.0 { field.clone() } else { json::num(n + delta) }
                        }
                        None => shifted(field, dx, dy),
                    };
                    (key.clone(), moved)
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `translatedAnnotation(annotation, by)`: the annotation moved by `by`:
/// `rect`, a shape caption's centre and the geometry its kind moves by. The
/// same object for no move.
pub fn translated_annotation(annotation: &Object, dx: f64, dy: f64) -> Object {
    if dx == 0.0 && dy == 0.0 {
        return annotation.clone();
    }
    let subtype = json::get_str(annotation, "subtype").unwrap_or("");
    let mut keys: Vec<&str> = vec!["rect", "captionCenter"];
    for key in translatable_geometry(subtype) {
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    let mut moved = annotation.clone();
    for key in keys {
        if let Some(field) = annotation.get(key).filter(|f| !json::nullish(Some(f))) {
            moved.insert(key.to_string(), shifted(field, dx, dy));
        }
    }
    moved
}

/// `encodePageKey(page)`, for comparing pages: the page's object number.
fn page_key(page: &Value) -> Option<String> {
    page.get("objectNumber").and_then(json::number).map(crate::js::number_to_string)
}

/// `planAnnotationCopies(sources, placement, unwritable)`: the copies of
/// `sources`, in the order given.
///
/// - A copy carries its source's data, moved by `offset`, on `page` when one
///   is named.
/// - A reply to an annotation copied too replies to that one's copy, so a
///   copied group is a group of its own; a reply to any other stands alone.
/// - A popup comes with the annotation it shows, or not at all.
/// - A field that marks something the engine can't write is left out of its
///   copy.
/// - A kind the engine doesn't know, a widget, and a source `unwritable`
///   says a write can't make, are left out.
///
/// A source named twice is copied twice; a link to it points at its first copy.
pub fn plan_annotation_copies(
    sources: &[Object],
    page: Option<&Value>,
    offset: Option<(f64, f64)>,
    unwritable: impl Fn(usize) -> bool,
) -> CopyPlan {
    let mut drops: HashMap<usize, &'static str> = HashMap::new();
    for (index, data) in sources.iter().enumerate() {
        match json::get_str(data, "subtype") {
            Some("unsupported") => {
                drops.insert(index, "unsupported-kind");
            }
            Some("widget") => {
                drops.insert(index, "widget");
            }
            _ if unwritable(index) => {
                drops.insert(index, "unsupported-value");
            }
            _ => {}
        }
    }
    let key = |reference: Option<&Value>| reference.and_then(annotation_key);
    let mut first: HashMap<String, usize> = HashMap::new();
    for (index, data) in sources.iter().enumerate() {
        if let Some(k) = key(data.get("ref")) {
            first.entry(k).or_insert(index);
        }
    }
    let page_of = |index: usize| -> Value {
        page.cloned().unwrap_or_else(|| {
            sources[index].get("ref").and_then(|r| r.get("page")).cloned().unwrap_or(Value::Null)
        })
    };
    // A reply and a popup are on their parent's page (ISO 32000-2 §12.5.6.2),
    // so a link holds only between copies that land on one page.
    let copied = |drops: &HashMap<usize, &'static str>, reference: &Value, from: usize| {
        let found = *first.get(&annotation_key(reference)?)?;
        if drops.contains_key(&found) {
            return None;
        }
        (page_key(&page_of(found)) == page_key(&page_of(from))).then_some(found)
    };

    let mut popups: HashSet<usize> = HashSet::new();
    for (index, data) in sources.iter().enumerate() {
        if drops.contains_key(&index) || json::get_str(data, "subtype") != Some("popup") {
            continue;
        }
        let parent_ref = data.get("parent").filter(|p| json::truthy(Some(p)));
        let named = parent_ref.and_then(|p| annotation_key(p)).and_then(|k| first.get(&k).copied());
        let parent = parent_ref.and_then(|p| copied(&drops, p, index));
        match parent {
            None => {
                let reason = if named.is_some_and(|n| drops.contains_key(&n)) {
                    "parent-dropped"
                } else {
                    "parent-missing"
                };
                drops.insert(index, reason);
            }
            Some(parent) if popups.contains(&parent) => {
                drops.insert(index, "popup-taken");
            }
            Some(parent) => {
                popups.insert(parent);
            }
        }
    }

    let mut place: Vec<usize> = Vec::new();
    let mut place_of: HashMap<usize, usize> = HashMap::new();
    for index in 0..sources.len() {
        if !drops.contains_key(&index) {
            place_of.insert(index, place.len());
            place.push(index);
        }
    }
    let (dx, dy) = offset.unwrap_or((0.0, 0.0));
    let copies = place
        .iter()
        .map(|&index| {
            let source = &sources[index];
            let reply = source.get("reply").filter(|r| json::truthy(Some(r)));
            let replied = reply.and_then(|r| r.get("to")).and_then(|to| copied(&drops, to, index));
            let parent = if json::get_str(source, "subtype") == Some("popup") {
                source
                    .get("parent")
                    .filter(|p| json::truthy(Some(p)))
                    .and_then(|p| copied(&drops, p, index))
            } else {
                None
            };
            let mut data: Map<String, Value> = source.clone();
            for name in ["nm", "groupId", "reply", "popup"] {
                data.shift_remove(name);
            }
            for (field, _) in field_markers_of(source) {
                data.shift_remove(&field);
            }
            PlannedCopy {
                source: index,
                page: page_of(index),
                data: translated_annotation(&data, dx, dy),
                reply_to: replied.map(|r| {
                    (
                        place_of[&r],
                        reply.and_then(|r| r.get("type")).cloned().unwrap_or(Value::Null),
                    )
                }),
                parent: parent.map(|p| place_of[&p]),
            }
        })
        .collect();

    let dropped = sources
        .iter()
        .enumerate()
        .filter_map(|(index, data)| {
            drops.get(&index).map(|reason| {
                json!({ "ref": data.get("ref").cloned().unwrap_or(Value::Null), "reason": reason })
            })
        })
        .collect();
    CopyPlan { copies, dropped }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: u32, page: u32) -> Value {
        json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": page }, "objectNumber": n })
    }

    fn obj(v: Value) -> Object {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn a_reply_follows_its_parents_copy_and_a_popup_needs_its_parent() {
        let note = obj(
            json!({ "subtype": "text", "ref": r(1, 1), "nm": "a", "rect": { "x": 1, "y": 2, "width": 3, "height": 4 } }),
        );
        let reply = obj(
            json!({ "subtype": "text", "ref": r(2, 1), "reply": { "to": r(1, 1), "type": "group" }, "rect": { "x": 0, "y": 0, "width": 1, "height": 1 } }),
        );
        let orphan = obj(json!({ "subtype": "popup", "ref": r(3, 1), "parent": r(9, 1) }));
        let widget = obj(json!({ "subtype": "widget", "ref": r(4, 1) }));
        let plan =
            plan_annotation_copies(&[note, reply, orphan, widget], None, Some((10.0, 5.0)), |_| {
                false
            });
        assert_eq!(plan.copies.len(), 2);
        assert_eq!(plan.copies[0].data.get("nm"), None);
        assert_eq!(
            plan.copies[0].data["rect"],
            json!({ "x": 11.0, "y": 7.0, "width": 3, "height": 4 })
        );
        assert_eq!(plan.copies[1].reply_to, Some((0, json!("group"))));
        assert_eq!(
            plan.dropped,
            vec![
                json!({ "ref": r(3, 1), "reason": "parent-missing" }),
                json!({ "ref": r(4, 1), "reason": "widget" }),
            ]
        );
    }

    #[test]
    fn a_link_holds_only_between_copies_on_one_page() {
        let parent = obj(json!({ "subtype": "text", "ref": r(1, 1) }));
        let reply = obj(
            json!({ "subtype": "text", "ref": r(2, 2), "reply": { "to": r(1, 1), "type": "reply" } }),
        );
        let plan = plan_annotation_copies(&[parent, reply], None, None, |_| false);
        assert_eq!(plan.copies[1].reply_to, None);
    }
}
