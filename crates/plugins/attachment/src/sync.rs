//! `sync/files.ts`: how the document's attachments change. `fold_files`
//! applies one confirmed event, the mirror's `fold`; `predict_files` applies
//! one op of a pending change by the engine's rules, the mirror's
//! `predict`; `undo_of` says what undoing ops looks like. Pure.
//!
//! An unchanged list comes back as the same `Rc`, as the TypeScript returns
//! the same array, so the mirror sees no change.

use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::mirror::Fold;
use epdf_kernel::{is_undefined, undefined};
use serde_json::{Value, json};

use crate::model::{js_greater, key_of, member, strict_equals, to_attachment_ref};

fn files_of(files: &Value) -> &[Value] {
    files.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// `indexed(files)`: each file's `index` its position, keeping the files
/// whose position held.
fn indexed(files: Vec<Value>) -> Value {
    Value::Array(
        files
            .into_iter()
            .enumerate()
            .map(|(index, mut file)| {
                let position = json!(index);
                if !file.get("index").is_some_and(|i| strict_equals(i, &position))
                    && let Value::Object(map) = &mut file
                {
                    map.insert("index".into(), position);
                }
                file
            })
            .collect(),
    )
}

/// `has(files, key)`.
fn has(files: &Value, key: &Value) -> bool {
    files_of(files).iter().any(|file| strict_equals(&key_of(file), key))
}

/// `withFile(files, attachment)`: the files with `attachment` among them,
/// before the first key that sorts after its own, as the name tree puts a
/// new entry; one with its key gives way to it.
fn with_file(files: &Value, attachment: &Value) -> Value {
    let key = key_of(attachment);
    let mut rest: Vec<Value> = files_of(files)
        .iter()
        .filter(|file| !strict_equals(&key_of(file), &key))
        .cloned()
        .collect();
    let at = rest.iter().position(|file| greater(&key_of(file), &key));
    match at {
        Some(at) => rest.insert(at, attachment.clone()),
        None => rest.push(attachment.clone()),
    }
    indexed(rest)
}

/// `file.ref.key > key`: strings compare by UTF-16 code units.
fn greater(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => js_greater(a, b),
        _ => false,
    }
}

/// `withoutFile(files, key)`: the same files when none has `key`.
fn without_file(files: &Rc<Value>, key: &Value) -> Rc<Value> {
    if !has(files, key) {
        return files.clone();
    }
    Rc::new(indexed(
        files_of(files).iter().filter(|file| !strict_equals(&key_of(file), key)).cloned().collect(),
    ))
}

/// `foldFiles(files, event)`: one confirmed event, applied with the record
/// it carries. Before the first load there is nothing to apply it to.
pub fn fold_files(files: Rc<Value>, event: &Value) -> Result<Fold, Error> {
    if files.is_null() {
        return Ok(Fold::Value(files));
    }
    let next = match event.get("type").and_then(Value::as_str) {
        Some("attachments.created" | "attachments.restored") => {
            Rc::new(with_file(&files, &member(event, "attachment")))
        }
        Some("attachments.deleted") => match event.get("deleted").filter(|d| truthy_object(d)) {
            Some(deleted) => without_file(&files, &member(deleted, "key")),
            None => files,
        },
        _ => files,
    };
    Ok(Fold::Value(next))
}

fn truthy_object(value: &Value) -> bool {
    crate::model::truthy(Some(value))
}

/// `predictedFile(op)`: a file a create adds, as it shows until the engine
/// answers: what the op says of it, and the size of its bytes.
fn predicted_file(op: &Value) -> Value {
    let file = member(op, "file");
    let bytes = op.get("resources").map_or_else(undefined, |r| member(r, "file"));
    let size = if is_undefined(&bytes) {
        Value::Null
    } else if let Some(bytes) = Bytes::from_json(&bytes) {
        json!(bytes.0.len())
    } else {
        // `bytes.size`: a `Blob`'s size; JSON has no `Blob`.
        member(&bytes, "size")
    };
    let or_null = |name: &str| match file.get(name) {
        None | Some(Value::Null) => Value::Null,
        Some(v) if is_undefined(v) => Value::Null,
        Some(v) => v.clone(),
    };
    json!({
        "ref": to_attachment_ref(member(&file, "name")),
        "index": 0,
        "name": member(&file, "name"),
        "mimeType": or_null("mimeType"),
        "description": or_null("description"),
        "size": size,
        "checksum": null,
        "createdAt": null,
        "modifiedAt": null,
    })
}

/// `predictFiles(files, op)`: one op of a change this session staged,
/// shown before the engine answers, by the engine's rules: a create of a
/// name a file has, a delete of a file that isn't there, and a restore of a
/// name a file has change nothing.
pub fn predict_files(files: Rc<Value>, op: &Value) -> Result<Rc<Value>, Error> {
    if files.is_null() {
        return Ok(files);
    }
    Ok(match op.get("type").and_then(Value::as_str) {
        Some("attachments.create") => {
            let name = op.get("file").map_or_else(undefined, |f| member(f, "name"));
            if has(&files, &name) { files } else { Rc::new(with_file(&files, &predicted_file(op))) }
        }
        Some("attachments.delete") => {
            let key = op.get("ref").map_or_else(undefined, |r| member(r, "key"));
            without_file(&files, &key)
        }
        Some("attachments.restore") => {
            let attachment = member(op, "attachment");
            if has(&files, &key_of(&attachment)) {
                files
            } else {
                Rc::new(with_file(&files, &attachment))
            }
        }
        _ => files,
    })
}

/// `undoOfOp(files, op)`: what undoing one op looks like, read off the
/// files it applies to.
fn undo_of_op(files: &Value, op: &Value) -> Vec<Value> {
    match op.get("type").and_then(Value::as_str) {
        Some("attachments.create") => {
            let name = op.get("file").map_or_else(undefined, |f| member(f, "name"));
            vec![json!({ "type": "attachments.delete", "ref": to_attachment_ref(name) })]
        }
        Some("attachments.delete") => {
            let key = op.get("ref").map_or_else(undefined, |r| member(r, "key"));
            files_of(files)
                .iter()
                .find(|file| strict_equals(&key_of(file), &key))
                .map(|attachment| {
                    vec![json!({ "type": "attachments.restore", "attachment": attachment })]
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

/// `undoOf(files, ops)`: what undoing `ops` looks like, read off the files
/// they apply to, each op on the files the ops before it leave; the last op
/// is undone first.
pub fn undo_of(files: &Rc<Value>, ops: &[Value]) -> Result<Vec<Value>, Error> {
    if files.is_null() {
        return Ok(Vec::new());
    }
    let mut undo: Vec<Vec<Value>> = Vec::with_capacity(ops.len());
    let mut current = files.clone();
    for op in ops {
        undo.push(undo_of_op(&current, op));
        current = predict_files(current, op)?;
    }
    Ok(undo.into_iter().rev().flatten().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(name: &str, index: usize) -> Value {
        json!({ "ref": { "kind": "key", "key": name }, "index": index, "name": name })
    }

    fn names(files: &Value) -> Vec<&str> {
        files_of(files).iter().map(|f| f["name"].as_str().unwrap()).collect()
    }

    #[test]
    fn a_file_goes_where_its_key_sorts_and_the_indices_follow() {
        let files = Rc::new(json!([att("a", 0), att("c", 1)]));
        let Fold::Value(next) = fold_files(
            files.clone(),
            &json!({ "type": "attachments.created", "attachment": att("b", 9) }),
        )
        .unwrap() else {
            panic!("a value")
        };
        assert_eq!(names(&next), ["a", "b", "c"]);
        assert_eq!(next[1]["index"], 1);
        assert_eq!(next[2]["index"], 2);
    }

    #[test]
    fn nothing_to_change_keeps_the_rc() {
        let files = Rc::new(json!([att("a", 0)]));
        let missing =
            json!({ "type": "attachments.deleted", "deleted": { "kind": "key", "key": "z" } });
        let Fold::Value(next) = fold_files(files.clone(), &missing).unwrap() else { panic!() };
        assert!(Rc::ptr_eq(&files, &next));
        let none = json!({ "type": "attachments.deleted", "deleted": null });
        let Fold::Value(next) = fold_files(files.clone(), &none).unwrap() else { panic!() };
        assert!(Rc::ptr_eq(&files, &next));
        let taken =
            json!({ "type": "attachments.create", "file": { "name": "a" }, "resources": {} });
        assert!(Rc::ptr_eq(&files, &predict_files(files.clone(), &taken).unwrap()));
    }

    #[test]
    fn a_predicted_file_has_the_size_of_its_bytes() {
        let op = json!({ "type": "attachments.create", "file": { "name": "n", "mimeType": "text/plain" },
                         "resources": { "file": { "$bytes": "aGVsbG8=" } } });
        let next = predict_files(Rc::new(json!([])), &op).unwrap();
        assert_eq!(
            next[0],
            json!({ "ref": { "kind": "key", "key": "n" }, "index": 0, "name": "n", "mimeType": "text/plain",
                    "description": null, "size": 5, "checksum": null, "createdAt": null, "modifiedAt": null })
        );
    }

    #[test]
    fn undo_reads_each_op_off_the_files_the_ops_before_it_leave() {
        let files = Rc::new(json!([att("a", 0), att("b", 1)]));
        let ops = [
            json!({ "type": "attachments.delete", "ref": { "kind": "key", "key": "a" } }),
            json!({ "type": "attachments.create", "file": { "name": "c" }, "resources": {} }),
            json!({ "type": "attachments.delete", "ref": { "kind": "key", "key": "a" } }),
        ];
        let undo = undo_of(&files, &ops).unwrap();
        assert_eq!(
            undo,
            vec![
                json!({ "type": "attachments.delete", "ref": { "kind": "key", "key": "c" } }),
                json!({ "type": "attachments.restore", "attachment": att("a", 0) }),
            ]
        );
        assert!(undo_of(&Rc::new(Value::Null), &ops).unwrap().is_empty());
    }
}
