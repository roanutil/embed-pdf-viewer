//! `model.ts`: the document's attachments as the mirror holds them, how an
//! attachment and a page's file become one `AttachedFile`, and the name a
//! new file takes. Pure.
//!
//! Records are the engine's JSON, as the plugin carries them: an
//! `Attachment` is `{ref, index, name, mimeType, description, size,
//! checksum, createdAt, modifiedAt}` with `ref` `{kind: 'key', key}`, and
//! the mirror holds the array of them, or `null` before the first read.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::{is_undefined, undefined};
use serde::Serialize;
use serde_json::{Map, Value, json};

/// `AttachmentState`: the session's part, the selected files by id, in the
/// order they were selected.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AttachmentState {
    pub selected: Rc<Vec<Value>>,
}

/// `initialAttachmentState()`.
pub fn initial_attachment_state() -> AttachmentState {
    AttachmentState { selected: Rc::new(Vec::new()) }
}

/// `selected(state, ids)`: make these files the selection, each id once in
/// the order given; the same ids leave the state as it is (the same `Rc`).
pub fn selected(state: Rc<AttachmentState>, ids: &[Value]) -> Rc<AttachmentState> {
    let unique: Vec<Value> = ids
        .iter()
        .enumerate()
        .filter(|(i, id)| ids.iter().position(|other| strict_equals(other, id)) == Some(*i))
        .map(|(_, id)| id.clone())
        .collect();
    let same = unique.len() == state.selected.len()
        && unique.iter().zip(state.selected.iter()).all(|(a, b)| strict_equals(a, b));
    if same {
        return state;
    }
    Rc::new(AttachmentState { selected: Rc::new(unique) })
}

/// `===` between two JSON values: primitives by value. Objects and arrays
/// have no identity in JSON, so two of them compare unequal, as two
/// distinct JavaScript objects do.
pub(crate) fn strict_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Object(_) | Value::Array(_), _) if !is_undefined(a) => false,
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// JavaScript truthiness of a member that may be absent.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(v) if is_undefined(v) => false,
        Some(_) => true,
    }
}

/// `value.member`, `undefined` when it is absent.
pub(crate) fn member(value: &Value, name: &str) -> Value {
    value.get(name).cloned().unwrap_or_else(undefined)
}

/// The members of an object spread (`{...value}`): an object's own members
/// in order, nothing for anything else.
pub(crate) fn spread(value: &Value) -> Map<String, Value> {
    match value {
        Value::Object(map) if !is_undefined(value) => map.clone(),
        _ => Map::new(),
    }
}

/// `a > b` for two strings: JavaScript compares UTF-16 code units, so a
/// character outside the BMP (a surrogate pair, from U+D800) sorts before
/// U+E000 to U+FFFF, where `str`'s order puts it after.
pub(crate) fn js_greater(a: &str, b: &str) -> bool {
    a.encode_utf16().cmp(b.encode_utf16()) == std::cmp::Ordering::Greater
}

/// `DocumentFiles`: the document's attachments in name-tree order, or
/// `null` before the first read lands.
pub type DocumentFiles = Value;

/// An attachment's key: `ref.key`.
pub(crate) fn key_of(attachment: &Value) -> Value {
    attachment.get("ref").map_or_else(undefined, |r| member(r, "key"))
}

/// `toAttachmentRef(key)`.
pub fn to_attachment_ref(key: Value) -> Value {
    json!({ "kind": "key", "key": key })
}

/// `documentFileId(ref)`: `doc:<key>`.
pub fn document_file_id(attachment_ref: &Value) -> String {
    format!("doc:{}", js_string(&member(attachment_ref, "key")))
}

/// `pageFileId(ref)`: `page:<annotationKey>`.
pub fn page_file_id(annotation_ref: &Value) -> String {
    let key = epdf_annotation_core::record::annotation_key(annotation_ref);
    format!("page:{}", key.as_deref().unwrap_or("undefined"))
}

/// A value as a template literal prints it.
pub(crate) fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => epdf_kernel::js::number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        v if is_undefined(v) => "undefined".into(),
        Value::Array(_) => "".into(),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `documentFileOf(attachment)`: an attachment as the list shows it, its
/// fields without `ref` and `index`, then `id` and `home`.
pub fn document_file_of(attachment: &Value) -> Value {
    let attachment_ref = member(attachment, "ref");
    let mut file = spread(attachment);
    file.shift_remove("ref");
    file.shift_remove("index");
    file.insert("id".into(), Value::String(document_file_id(&attachment_ref)));
    file.insert("home".into(), json!({ "kind": "document", "ref": attachment_ref }));
    Value::Object(file)
}

/// `pageFileOf(annotation)`: a file attachment annotation's file as the
/// list shows it; `None` for another kind, or one without a file.
pub fn page_file_of(annotation: &Value) -> Option<Value> {
    if annotation.get("subtype").and_then(Value::as_str) != Some("file-attachment")
        || !truthy(annotation.get("file"))
    {
        return None;
    }
    let annotation_ref = member(annotation, "ref");
    let mut file = spread(&annotation["file"]);
    file.insert("id".into(), Value::String(page_file_id(&annotation_ref)));
    file.insert("home".into(), json!({ "kind": "page", "ref": annotation_ref }));
    Some(Value::Object(file))
}

/// `freeName(name, taken)`: `name`, or, when a file has it already, the
/// name with the first number that makes it free: `notes (2).txt`, then
/// `notes (3).txt`. The number goes before the extension, as a file manager
/// puts it.
pub fn free_name(name: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(name) {
        return name.to_string();
    }
    // `.` is one UTF-16 unit and one byte, so `lastIndexOf('.') > 0` is
    // the same test on either index.
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (2u64..)
        .map(|number| format!("{stem} ({number}){extension}"))
        .find(|candidate| !taken.contains(candidate))
        .expect("an unbounded range finds a free name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(names: &[&str]) -> HashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn a_free_name_numbers_before_the_extension() {
        let t = taken(&["notes.txt", "notes (2).txt", "README", ".env", "a.b.c"]);
        assert_eq!(free_name("other.txt", &t), "other.txt");
        assert_eq!(free_name("notes.txt", &t), "notes (3).txt");
        assert_eq!(free_name("README", &t), "README (2)");
        assert_eq!(free_name(".env", &t), ".env (2)");
        assert_eq!(free_name("a.b.c", &t), "a.b (2).c");
    }

    #[test]
    fn keys_compare_as_utf16() {
        assert!(js_greater("\u{ff26}.bin", "\u{1f4c4}.txt"));
        assert!(!js_greater("\u{1f4c4}.txt", "\u{ff26}.bin"));
        assert!(js_greater("notes.txt", "notes (2).txt"));
    }

    #[test]
    fn the_same_ids_keep_the_state() {
        let state = Rc::new(initial_attachment_state());
        let next = selected(state.clone(), &[json!("a"), json!("b"), json!("a")]);
        assert_eq!(*next.selected, vec![json!("a"), json!("b")]);
        assert!(Rc::ptr_eq(&next, &selected(next.clone(), &[json!("a"), json!("b")])));
        assert!(Rc::ptr_eq(&state, &selected(state.clone(), &[])));
    }

    #[test]
    fn files_of_both_homes() {
        let attachment = json!({ "ref": { "kind": "key", "key": "a.txt" }, "index": 0, "name": "a.txt", "size": 1 });
        assert_eq!(
            document_file_of(&attachment),
            json!({ "name": "a.txt", "size": 1, "id": "doc:a.txt",
                    "home": { "kind": "document", "ref": { "kind": "key", "key": "a.txt" } } })
        );
        let r = json!({ "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": 1 }, "objectNumber": 20 });
        let annotation = json!({ "subtype": "file-attachment", "ref": r, "file": { "name": "f" } });
        assert_eq!(
            page_file_of(&annotation),
            Some(json!({ "name": "f", "id": "page:obj:20", "home": { "kind": "page", "ref": r } }))
        );
        assert_eq!(
            page_file_of(&json!({ "subtype": "file-attachment", "ref": r, "file": null })),
            None
        );
        assert_eq!(
            page_file_of(&json!({ "subtype": "square", "ref": r, "file": { "name": "f" } })),
            None
        );
    }
}
