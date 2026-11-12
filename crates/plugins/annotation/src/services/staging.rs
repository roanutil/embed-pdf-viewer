//! `services/staging.ts`: how the store puts one change together from ops,
//! whichever door they came through: a change stated in code, checked, as
//! its op; what follows an update; the deletes another delete covers; the
//! records it names and what it is called. Pure functions over JSON ops.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_annotation_core::appearance::{drawn_after, source_of_new};
use epdf_annotation_core::engine::resolve::resolve_annotation_patch;
use epdf_annotation_core::record::from_dto;
use epdf_annotation_core::types::{ById, Id, Model, ModelAnnotation, Source, UpdateResult};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Value, json};

use crate::controller::CAPABILITY;
use crate::engine::deleted_with;
use crate::js::{Object, key_of, key_or_none, page_number};
use crate::sync::records::{annotation_of_create, by_key, core_error, dto_of, order};

/// A change stated in code, in the engine's own terms (`StoreChange`).
#[derive(Clone, Debug)]
pub enum StoreChange {
    Create {
        page: Value,
        draft: Object,
        resources: Option<Value>,
    },
    Update {
        reference: Value,
        patch: Object,
        resources: Option<Value>,
    },
    Delete {
        reference: Value,
    },
    /// Drawing order: the annotations go together to `position` among the page's others.
    Reorder {
        page: Value,
        refs: Vec<Value>,
        position: Value,
    },
    /// Copies of these annotations, which the engine makes in one op: moved
    /// by `offset`, on `page` when one is named. `along` is copied after
    /// them (an annotation's attached link), and its copies aren't among the
    /// records the change names.
    Copy {
        refs: Vec<Value>,
        along: Vec<Value>,
        page: Option<Value>,
        offset: Option<Value>,
    },
}

impl StoreChange {
    pub fn is_create(&self) -> bool {
        matches!(self, StoreChange::Create { .. })
    }
}

/// `copiedBy(change)`: the annotations a stated copy copies: the ones it
/// names, then what comes along.
fn copied_by(refs: &[Value], along: &[Value]) -> Vec<Value> {
    refs.iter().chain(along).cloned().collect()
}

/// The `annotations.copy` op of a stated copy, `objectNumbers` aside.
fn copy_op(refs: &[Value], along: &[Value], page: &Option<Value>, offset: &Option<Value>) -> Value {
    let mut op = json!({ "type": "annotations.copy", "refs": copied_by(refs, along) });
    if let Some(page) = page {
        op["page"] = page.clone();
    }
    if let Some(offset) = offset {
        op["offset"] = offset.clone();
    }
    op
}

/// `opOfStated(change)`: a stated change as its op, as given.
pub fn op_of_stated(change: &StoreChange) -> Value {
    match change {
        StoreChange::Create { page, draft, resources } => {
            let mut op = json!({ "type": "annotations.create", "page": page, "data": draft });
            if let Some(r) = resources {
                op["resources"] = r.clone();
            }
            op
        }
        StoreChange::Update { reference, patch, resources } => {
            let mut op = json!({ "type": "annotations.update", "ref": reference, "patch": patch });
            if let Some(r) = resources {
                op["resources"] = r.clone();
            }
            op
        }
        StoreChange::Delete { reference } => {
            json!({ "type": "annotations.delete", "ref": reference })
        }
        StoreChange::Reorder { page, refs, position } => {
            json!({ "type": "annotations.reorder", "page": page, "refs": refs, "position": position })
        }
        StoreChange::Copy { refs, along, page, offset } => copy_op(refs, along, page, offset),
    }
}

/// The record keys an op names.
fn keys_of(op: &Value) -> Vec<String> {
    let refs = |key: &str| -> Vec<String> {
        op.get(key).and_then(Value::as_array).into_iter().flatten().map(key_or_none).collect()
    };
    match op.get("type").and_then(Value::as_str).unwrap_or("") {
        "annotations.create" => {
            match op.get("objectNumber").filter(|n| !epdf_kernel::is_undefined(n)) {
                Some(n) => vec![format!("obj:{}", crate::js::stringify(Some(n)))],
                None => Vec::new(),
            }
        }
        // The copies: the annotations it copies stay as they are.
        "annotations.copy" => op
            .get("objectNumbers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|n| format!("obj:{}", crate::js::stringify(Some(n))))
            .collect(),
        "annotations.update" | "annotations.delete" => {
            vec![key_or_none(op.get("ref").unwrap_or(&Value::Null))]
        }
        "annotations.reorder" => refs("refs"),
        "forms.updateWidget" | "forms.deleteWidget" => {
            vec![key_or_none(op.get("widget").unwrap_or(&Value::Null))]
        }
        "forms.reorderWidgets" => refs("widgets"),
        _ => Vec::new(),
    }
}

/// `idsOf(ops)`: the records a change names, each once.
pub fn ids_of(ops: &[Value]) -> Vec<Id> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for op in ops {
        for key in keys_of(op) {
            if seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    out
}

/// `labelOf(ops)`: what a change is called.
pub fn label_of(ops: &[Value]) -> Value {
    let mut kinds: Vec<&str> = Vec::new();
    for op in ops {
        let ty = op.get("type").and_then(Value::as_str).unwrap_or("undefined");
        if !kinds.contains(&ty) {
            kinds.push(ty);
        }
    }
    let (plane, verb) = if kinds.len() == 1 {
        let mut parts = kinds[0].splitn(3, '.');
        let plane = parts.next().unwrap_or("").to_string();
        let verb = parts.next().map_or("undefined".to_string(), str::to_string);
        (plane, verb)
    } else {
        ("annotations".to_string(), "change".to_string())
    };
    let owner = match plane.as_str() {
        "annotations" => "annotation".to_string(),
        "forms" => "form".to_string(),
        other => other.to_string(),
    };
    // `LABEL_VERBS`: the verb of this plugin that makes an op, where the
    // engine names the op differently: `duplicate()` makes copies.
    let verb = match kinds.as_slice() {
        ["annotations.copy"] => "duplicate".to_string(),
        _ => verb,
    };
    json!({ "key": format!("{owner}.{verb}"), "count": ids_of(ops).len() })
}

/// `withoutCoveredDeletes(records, ops)`: the ops without the deletes
/// another delete of them already covers.
pub fn without_covered_deletes(records: &Value, ops: Vec<Value>) -> Vec<Value> {
    let deleted: Vec<&Value> = ops
        .iter()
        .filter(|op| op.get("type").and_then(Value::as_str) == Some("annotations.delete"))
        .map(|op| op.get("ref").unwrap_or(&Value::Null))
        .collect();
    if deleted.len() < 2 {
        return ops;
    }
    let byk = by_key(records);
    let all = order(records);
    let mut covered: HashSet<String> = HashSet::new();
    for reference in &deleted {
        let page = page_number(reference).unwrap_or(f64::NAN);
        let on_page: Vec<&Value> = all
            .iter()
            .filter_map(|k| byk.get(k))
            .map(dto_of)
            .filter(|dto| dto.get("page").and_then(page_number) == Some(page))
            .collect();
        let own = key_or_none(reference);
        for i in deleted_with(&on_page, reference) {
            let key = on_page[i].get("ref").map(key_or_none).unwrap_or_default();
            if key != own {
                covered.insert(key);
            }
        }
    }
    ops.into_iter()
        .filter(|op| {
            op.get("type").and_then(Value::as_str) != Some("annotations.delete")
                || !covered.contains(&key_or_none(op.get("ref").unwrap_or(&Value::Null)))
        })
        .collect()
}

/// `modelOver(before, previous, shown)`: `before` with the records as
/// `shown` holds them. A record it didn't touch keeps its identity.
pub fn model_over(before: &Model, previous: &Value, shown: &Value) -> Model {
    let mut by_id = ById::new();
    let mut kept_order = Vec::new();
    let prev = by_key(previous);
    for key in order(shown) {
        let Some(stored) = by_key(shown).get(&key) else { continue };
        let kept = prev
            .get(&key)
            .filter(|p| crate::sync::records::identity(p) == crate::sync::records::identity(stored))
            .and_then(|_| before.by_id.get(&key).cloned());
        let record = match kept {
            Some(r) => r,
            None => {
                let dto = Rc::new(dto_of(stored).as_object().cloned().unwrap_or_default());
                let Some(mut record) = from_dto(dto) else { continue };
                record.source = Source::Vector;
                Rc::new(record)
            }
        };
        kept_order.push(key.clone());
        by_id.insert(key, record);
    }
    // A record `from_dto` cannot read is skipped from both, so `order` names
    // only records `by_id` has. The engine's records always read, so this
    // matches the TypeScript, which has no such record.
    Model { by_id: Rc::new(by_id), order: Rc::new(kept_order), ..before.clone() }
}

/// `modelAfter(before, result)`: `before`'s records with a message's change
/// set laid over them, and its session.
pub fn model_after(before: &Model, result: &UpdateResult) -> Model {
    if result.change.put.is_empty() && result.change.drop.is_empty() {
        return Model::of(result.session.clone(), before.by_id.clone(), before.order.clone());
    }
    let mut by_id = (*before.by_id).clone();
    for id in &result.change.drop {
        by_id.shift_remove(id);
    }
    for record in &result.change.put {
        by_id.insert(record.id.clone(), record.clone());
    }
    let added: Vec<Id> = result
        .change
        .put
        .iter()
        .map(|r| r.id.clone())
        .filter(|id| !before.by_id.contains_key(id))
        .collect();
    let mut order: Vec<Id> =
        before.order.iter().filter(|id| by_id.contains_key(*id)).cloned().collect();
    order.extend(added);
    Model::of(result.session.clone(), Rc::new(by_id), Rc::new(order))
}

fn not_found(reference: &Value) -> Error {
    plugin_error(
        code::NOT_FOUND,
        CAPABILITY,
        format!("no annotation {}", key_or_none(reference)),
        None,
        None,
    )
}

/// `numbersFor(changes)`: how many object numbers stated changes take: one
/// per create, and one per annotation a copy copies.
pub fn numbers_for(changes: &[StoreChange]) -> usize {
    changes
        .iter()
        .map(|change| match change {
            StoreChange::Create { .. } => 1,
            StoreChange::Copy { refs, along, .. } => refs.len() + along.len(),
            _ => 0,
        })
        .sum()
}

/// What [`checked_copy_of`] made of a stated copy.
pub struct CheckedCopy {
    /// The copies of the annotations it names, in their order (one the
    /// engine leaves out has none), each by the object number it gets.
    pub ids: Vec<Id>,
    pub op: Value,
    /// The copies that draw live from now on.
    pub live: Vec<Id>,
    /// Every copy it makes, each with the annotation it copies.
    pub copies: Vec<crate::sync::records::CopyOf>,
}

/// `checkedCopyOf(change, current, taken)`: a stated copy as its op, with an
/// object number for each annotation it copies, so the copies show at once
/// under them. Throws for an annotation the view doesn't have.
pub fn checked_copy_of(
    refs: &[Value],
    along: &[Value],
    page: &Option<Value>,
    offset: &Option<Value>,
    current: &Value,
    taken: &mut Vec<u32>,
) -> Result<CheckedCopy, Error> {
    let all = copied_by(refs, along);
    for reference in &all {
        if !by_key(current).contains_key(&key_or_none(reference)) {
            return Err(not_found(reference));
        }
    }
    let count = all.len().min(taken.len());
    let numbers: Vec<u32> = taken.drain(..count).collect();
    let mut op = copy_op(refs, along, page, offset);
    op["objectNumbers"] = json!(numbers);
    let copies = crate::sync::records::copies_of(current, &op);
    let ids = copies
        .iter()
        .filter(|c| c.source < refs.len())
        .map(|c| key_or_none(&c.reference))
        .collect();
    let live = copies
        .iter()
        .filter(|c| c.copy.as_ref().is_some_and(|copy| source_of_new(copy) == Source::Vector))
        .map(|c| key_or_none(&c.reference))
        .collect();
    Ok(CheckedCopy { ids, op, live, copies })
}

/// What [`checked_op_of`] made of one stated change.
pub struct CheckedOp {
    pub id: Id,
    pub op: Option<Value>,
    /// The record draws live from now on.
    pub live: bool,
}

/// `checkedOpOf(change, current, taken, recordOf)`: one stated change as its
/// op, resolved by the engine's own rules against `current`; a change those
/// rules refuse throws.
pub fn checked_op_of(
    change: &StoreChange,
    current: &Value,
    taken: &mut Vec<u32>,
    record_of: &dyn Fn(&str) -> Option<Rc<ModelAnnotation>>,
) -> Result<CheckedOp, Error> {
    match change {
        StoreChange::Create { page, draft, resources } => {
            let object_number = if taken.is_empty() { None } else { Some(taken.remove(0)) };
            let number = object_number.map_or(Value::Null, |n| json!(n));
            let reference = json!({ "kind": "objectNumber", "page": page, "objectNumber": number });
            let annotation = annotation_of_create(draft, &reference)?;
            let mut op = json!({ "type": "annotations.create", "page": page, "data": draft, "objectNumber": number });
            if let Some(r) = resources {
                op["resources"] = r.clone();
            }
            Ok(CheckedOp {
                id: key_or_none(&reference),
                op: Some(op),
                live: source_of_new(&annotation) == Source::Vector,
            })
        }
        StoreChange::Reorder { page, refs, position } => {
            for reference in refs {
                if !by_key(current).contains_key(&key_or_none(reference)) {
                    return Err(not_found(reference));
                }
            }
            Ok(CheckedOp {
                id: refs.first().map(key_or_none).unwrap_or_default(),
                op: Some(
                    json!({ "type": "annotations.reorder", "page": page, "refs": refs, "position": position }),
                ),
                live: false,
            })
        }
        StoreChange::Delete { reference } => {
            let id = key_or_none(reference);
            if !by_key(current).contains_key(&id) {
                return Err(not_found(reference));
            }
            Ok(CheckedOp {
                id,
                op: Some(json!({ "type": "annotations.delete", "ref": reference })),
                live: false,
            })
        }
        StoreChange::Copy { refs, along, page, offset } => {
            let copy = checked_copy_of(refs, along, page, offset, current, taken)?;
            Ok(CheckedOp {
                id: copy.ids.first().cloned().unwrap_or_default(),
                op: Some(copy.op),
                live: false,
            })
        }
        StoreChange::Update { reference, patch, resources } => {
            let id = key_or_none(reference);
            let Some(stored) = by_key(current).get(&id) else { return Err(not_found(reference)) };
            let no_fields = patch.keys().all(|name| name == "subtype");
            if no_fields && resources.is_none() {
                return Ok(CheckedOp { id, op: None, live: false });
            }
            let dto = dto_of(stored).as_object().cloned().unwrap_or_default();
            resolve_annotation_patch(&dto, patch, None).map_err(core_error)?;
            let live = record_of(&id).is_some_and(|record| {
                drawn_after(&record, patch).is_ok_and(|d| d.source == Some(Source::Vector))
            });
            let mut op = json!({ "type": "annotations.update", "ref": reference, "patch": patch });
            if let Some(r) = resources {
                op["resources"] = r.clone();
            }
            Ok(CheckedOp { id, op: Some(op), live })
        }
    }
}

/// The key a create op's record takes.
pub fn created_key(op: &Value) -> Option<String> {
    let number =
        op.get("objectNumber").filter(|n| !epdf_kernel::is_undefined(n) && !n.is_null())?;
    key_of(
        &json!({ "kind": "objectNumber", "page": op.get("page").cloned().unwrap_or(Value::Null), "objectNumber": number }),
    )
}

#[cfg(test)]
mod tests {
    use super::model_over;
    use epdf_annotation_core::update::session::initial_model;
    use serde_json::json;

    /// A shown record `from_dto` cannot read is left out of `order` as well
    /// as `by_id`, so the two stay in step.
    #[test]
    fn a_record_model_over_cannot_read_leaves_order_too() {
        let page = json!({ "kind": "objectNumber", "objectNumber": 1 });
        let readable = json!({ "subtype": "square", "page": page,
            "ref": { "kind": "objectNumber", "page": page, "objectNumber": 1 } });
        let unreadable = json!({ "subtype": "square", "page": page, "ref": { "kind": "bogus" } });
        let shown = json!({
            "byKey": {
                "obj:1": { "dto": readable, "pictureVersion": 0, "$v": 1 },
                "obj:2": { "dto": unreadable, "pictureVersion": 0, "$v": 2 }
            },
            "order": ["obj:1", "obj:2"]
        });
        let empty = json!({ "byKey": {}, "order": [] });
        let model = model_over(&initial_model(), &empty, &shown);
        assert_eq!(*model.order, vec!["obj:1".to_string()]);
        assert_eq!(model.by_id.len(), 1);
    }
}
