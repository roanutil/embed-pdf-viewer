//! `sync/records.ts`: the records. Every annotation the engine confirmed,
//! held in a mirror: loaded whole once, kept current by folding confirmed
//! events from every origin, and a page re-read only when an event
//! describes it too coarsely. Form fields' widgets aren't records here.
//!
//! Two pure functions change the records, side by side here:
//! [`fold_records`] (what the engine confirmed) and [`predict_records`]
//! (what this session asked for and the engine hasn't answered yet, which
//! `records.view()` shows on top).
//!
//! The kernel's mirror holds JSON, so the records are one JSON value:
//! `{ "byKey": { key: { "dto", "pictureVersion", "$v" } }, "order": [key] }`.
//! `$v` stands for the record object's identity, which the view's caches
//! and `isPending` compare: every record put gets a fresh one, as every
//! TypeScript `put` makes a new object.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use epdf_annotation_core::engine::resolve::annotation_patch_between;
use epdf_annotation_core::record::{annotation_after, annotation_of_new};
use epdf_kernel::Error;
use epdf_kernel::mirror::Fold;
use serde_json::{Map, Value, json};

use crate::engine::{deleted_with, reorder_part, reordered_list};
use crate::js::{Object, key_of, number, page_number};

thread_local! {
    static NEXT_VERSION: Cell<u64> = const { Cell::new(1) };
}

/// A fresh record identity.
fn fresh() -> u64 {
    NEXT_VERSION.with(|n| {
        let v = n.get();
        n.set(v + 1);
        v
    })
}

/// `NO_RECORDS`.
pub fn no_records() -> Value {
    json!({ "byKey": {}, "order": [] })
}

/// The records' `byKey`.
pub fn by_key(records: &Value) -> &Map<String, Value> {
    static EMPTY: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    records.get("byKey").and_then(Value::as_object).unwrap_or_else(|| EMPTY.get_or_init(Map::new))
}

/// The records' `order`.
pub fn order(records: &Value) -> Vec<String> {
    records
        .get("order")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// One record: its annotation and appearance version.
pub fn record<'a>(records: &'a Value, key: &str) -> Option<&'a Value> {
    by_key(records).get(key)
}

/// A record's annotation.
pub fn dto_of(stored: &Value) -> &Value {
    &stored["dto"]
}

/// A record's appearance version.
pub fn picture_version(stored: &Value) -> f64 {
    stored.get("pictureVersion").and_then(number).unwrap_or(0.0)
}

/// A record's identity.
pub fn identity(stored: &Value) -> u64 {
    stored.get("$v").and_then(Value::as_u64).unwrap_or(0)
}

/// Whether two stored records are the same object.
pub fn same_record(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => identity(a) == identity(b),
        (None, None) => true,
        _ => false,
    }
}

/// The records as a store dump shows them: `$v` is this port's stand-in
/// for a record object's identity, which the TypeScript's records carry
/// as the object itself, so the dump leaves it out.
pub fn records_json(records: &Value) -> Value {
    let mut out = records.clone();
    if let Some(by_key) = out.get_mut("byKey").and_then(Value::as_object_mut) {
        for stored in by_key.values_mut() {
            if let Some(stored) = stored.as_object_mut() {
                stored.remove("$v");
            }
        }
    }
    out
}

fn stored(dto: Value, picture_version: f64) -> Value {
    json!({ "dto": dto, "pictureVersion": crate::js::num(picture_version), "$v": fresh() })
}

fn type_error(of: &str, reading: &str) -> Error {
    Error::named("TypeError", format!("Cannot read properties of {of} (reading '{reading}')"))
}

/// `annotationKey(dto.ref)`, as reading a record's ref throws: a missing ref
/// is the TypeScript's `TypeError`; a ref of a kind no key is defined for is
/// refused (review finding N6), where the TypeScript keys it `"undefined"`.
pub fn dto_key(dto: &Value) -> Result<String, Error> {
    match dto.get("ref") {
        None => Err(type_error("undefined", "kind")),
        Some(v) if epdf_kernel::is_undefined(v) => Err(type_error("undefined", "kind")),
        Some(Value::Null) => Err(type_error("null", "kind")),
        Some(reference) => key_of(reference).ok_or_else(|| {
            Error::new(format!(
                "[annotation] a ref of kind {} has no annotation key",
                crate::js::stringify(reference.get("kind"))
            ))
        }),
    }
}

fn page_of_dto(dto: &Value) -> f64 {
    dto.get("page").and_then(|p| p.get("objectNumber")).and_then(number).unwrap_or(f64::NAN)
}

fn with_parts(byk: Map<String, Value>, order: Vec<String>) -> Rc<Value> {
    Rc::new(json!({ "byKey": byk, "order": order }))
}

/// `put(records, dtos, bump)`: add or replace records; a replaced one keeps
/// its place.
fn put(records: &Rc<Value>, dtos: &[Value], bump: bool) -> Result<Rc<Value>, Error> {
    if dtos.is_empty() {
        return Ok(records.clone());
    }
    let mut byk = by_key(records).clone();
    let mut order = order(records);
    for dto in dtos {
        let key = dto_key(dto)?;
        let previous = byk.get(&key).map(picture_version);
        if previous.is_none() {
            order.push(key.clone());
        }
        let version = previous.unwrap_or(0.0) + if bump { 1.0 } else { 0.0 };
        byk.insert(key, stored(dto.clone(), version));
    }
    Ok(with_parts(byk, order))
}

fn drop_keys(records: &Rc<Value>, keys: &[String]) -> Rc<Value> {
    let byk = by_key(records);
    let present: HashSet<&String> = keys.iter().filter(|k| byk.contains_key(*k)).collect();
    if present.is_empty() {
        return records.clone();
    }
    let mut next = byk.clone();
    for key in &present {
        next.shift_remove(*key);
    }
    let order: Vec<String> = order(records).into_iter().filter(|k| !present.contains(k)).collect();
    with_parts(next, order)
}

fn keys_on_pages(records: &Value, pages: &[Value]) -> Vec<String> {
    let wanted: Vec<f64> = pages.iter().filter_map(page_number).collect();
    let byk = by_key(records);
    order(records)
        .into_iter()
        .filter(|k| byk.get(k).is_some_and(|s| wanted.contains(&page_of_dto(dto_of(s)))))
        .collect()
}

/// `goesTo(value, pages)`: a target or action tree names one of `pages`, or
/// holds what couldn't be read.
fn goes_to(value: &Value, pages: &[f64]) -> bool {
    match value {
        Value::Array(items) => items.iter().any(|v| goes_to(v, pages)),
        Value::Object(node) => {
            if node.get("kind").and_then(Value::as_str) == Some("unsupported") {
                return true;
            }
            if let Some(page) =
                node.get("page").and_then(|p| p.get("objectNumber")).and_then(number)
                && pages.contains(&page)
            {
                return true;
            }
            node.values().any(|v| goes_to(v, pages))
        }
        _ => false,
    }
}

/// `pagesGoingTo(records, pages)`: the other pages whose annotations go to `pages`.
fn pages_going_to(records: &Value, pages: &[Value]) -> Vec<Value> {
    let named: Vec<f64> = pages.iter().filter_map(page_number).collect();
    let mut found: Vec<(f64, Value)> = Vec::new();
    let byk = by_key(records);
    for key in order(records) {
        let Some(s) = byk.get(&key) else { continue };
        let dto = dto_of(s);
        let page = page_of_dto(dto);
        if named.contains(&page) || found.iter().any(|(p, _)| *p == page) {
            continue;
        }
        let target = dto.get("target").unwrap_or(&Value::Null);
        let actions = dto.get("actions").unwrap_or(&Value::Null);
        if goes_to(target, &named) || goes_to(actions, &named) {
            found.push((page, dto.get("page").cloned().unwrap_or(Value::Null)));
        }
    }
    found.into_iter().map(|(_, p)| p).collect()
}

/// `fromSnapshot(current, dtos)`: a fresh read of the whole document. Every
/// record's appearance version moves past the one it had.
pub fn from_snapshot(current: &Value, dtos: &[Value]) -> Result<Value, Error> {
    let mut byk = Map::new();
    let mut order: Vec<String> = Vec::new();
    let previous = by_key(current);
    for dto in dtos {
        let key = dto_key(dto)?;
        let version = previous.get(&key).map_or(0.0, |p| picture_version(p) + 1.0);
        if !byk.contains_key(&key) {
            order.push(key.clone());
        }
        byk.insert(key, stored(dto.clone(), version));
    }
    Ok(json!({ "byKey": byk, "order": order }))
}

/// `withPages(records, pages, dtos)`: some pages read again.
pub fn with_pages(
    records: &Rc<Value>,
    pages: &[Value],
    dtos: &[Value],
) -> Result<Rc<Value>, Error> {
    let mut read = HashSet::new();
    for dto in dtos {
        read.insert(dto_key(dto)?);
    }
    let stale: Vec<String> =
        keys_on_pages(records, pages).into_iter().filter(|k| !read.contains(k)).collect();
    put(&drop_keys(records, &stale), dtos, true)
}

fn event_pages(event: &Value, key: &str) -> Vec<Value> {
    event.get(key).and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `foldRecords(records, event)`: one confirmed event, the same for every origin.
pub fn fold_records(records: Rc<Value>, event: &Value) -> Result<Fold, Error> {
    let ty = event.get("type").and_then(Value::as_str).unwrap_or("");
    let annotation = || event.get("annotation").cloned().unwrap_or(Value::Null);
    Ok(match ty {
        "annotations.created" => Fold::Value(put(&records, &[annotation()], true)?),
        "annotations.updated" => {
            let changed =
                event.pointer("/appearance/changed").and_then(Value::as_bool) == Some(true);
            Fold::Value(put(&records, &[annotation()], changed)?)
        }
        "annotations.reordered" => {
            let page = event.get("page").and_then(page_number).unwrap_or(f64::NAN);
            let byk = by_key(&records);
            let in_part = |key: &str| byk.get(key).is_some_and(|s| page_of_dto(dto_of(s)) == page);
            let mut keys = Vec::new();
            for r in event.get("order").and_then(Value::as_array).into_iter().flatten() {
                keys.push(key_of(r).ok_or_else(|| {
                    Error::new("[annotation] a reordered ref has no annotation key")
                })?);
            }
            let next = reorder_part(&order(&records), &in_part, &keys);
            let byk = byk.clone();
            Fold::Value(with_parts(byk, next))
        }
        "annotations.deleted" => {
            let mut keys = Vec::new();
            for r in event.get("deleted").and_then(Value::as_array).into_iter().flatten() {
                keys.push(key_of(r).ok_or_else(|| {
                    Error::new("[annotation] a deleted ref has no annotation key")
                })?);
            }
            Fold::Value(drop_keys(&records, &keys))
        }
        "annotations.restored" => {
            Fold::Pages(vec![event.get("page").cloned().unwrap_or(Value::Null)])
        }
        "pages.deleted" => {
            let pages = event_pages(event, "pages");
            let going = pages_going_to(&records, &pages);
            if going.is_empty() {
                Fold::Value(drop_keys(&records, &keys_on_pages(&records, &pages)))
            } else {
                Fold::Pages(pages.into_iter().chain(going).collect())
            }
        }
        "pages.inserted" => {
            let inserted = event_pages(event, "insertedPages");
            if inserted.is_empty() {
                Fold::Value(records)
            } else {
                let going = pages_going_to(&records, &inserted);
                Fold::Pages(inserted.into_iter().chain(going).collect())
            }
        }
        "pages.restored" => {
            let pages = event_pages(event, "pages");
            let going = pages_going_to(&records, &pages);
            Fold::Pages(pages.into_iter().chain(going).collect())
        }
        "redaction.applied" | "pages.flattened" => {
            let pages: Vec<Value> = event
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|r| r.get("status").and_then(Value::as_str) == Some("applied"))
                .map(|r| r.get("page").cloned().unwrap_or(Value::Null))
                .collect();
            if pages.is_empty() { Fold::Value(records) } else { Fold::Pages(pages) }
        }
        "annotations.flattened" => {
            let any = event
                .get("results")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|r| r.get("status").and_then(Value::as_str) == Some("applied"));
            if any {
                Fold::Pages(vec![event.get("page").cloned().unwrap_or(Value::Null)])
            } else {
                Fold::Value(records)
            }
        }
        _ => Fold::Value(records),
    })
}

// ── what this session asked for ─────────────────────────────────────────────

/// `annotationOfCreate(draft, ref)`: the annotation a create writes, as the
/// engine reads it back, with the annotations it links to stated.
pub fn annotation_of_create(draft: &Object, reference: &Value) -> Result<Object, Error> {
    let mut fields = draft.clone();
    let reply = fields.shift_remove("reply");
    let parent = fields.shift_remove("parent");
    let mut annotation = annotation_of_new(&fields, reference).map_err(core_error)?;
    if let Some(reply) = reply.filter(|r| epdf_annotation_core::json::truthy(Some(r))) {
        let ty =
            reply.get("type").filter(|t| !t.is_null() && !epdf_kernel::is_undefined(t)).cloned();
        annotation.insert(
            "reply".into(),
            json!({ "to": reply.get("to").cloned().unwrap_or(Value::Null), "type": ty.unwrap_or(json!("reply")) }),
        );
    }
    if let Some(parent) = parent.filter(|p| epdf_annotation_core::json::truthy(Some(p))) {
        annotation.insert("parent".into(), parent);
    }
    Ok(annotation)
}

/// A core error as the plugin reports it.
pub fn core_error(error: epdf_annotation_core::CoreError) -> Error {
    let mut out = Error::named(error.name, error.message.clone());
    if let Some(code) = &error.code {
        out = out.with("code", *code);
    }
    if let Some(details) = error.details {
        out = out.with("details", *details);
    }
    out
}

/// The page's annotations, in drawing order.
fn annotations_on(records: &Value, page: f64) -> Vec<&Value> {
    let byk = by_key(records);
    order(records)
        .iter()
        .filter_map(|k| byk.get(k))
        .map(dto_of)
        .filter(|dto| page_of_dto(dto) == page)
        .collect()
}

/// `reorderedKeys(records, page, keys, position)`: unchanged when the engine would refuse it.
fn reordered_keys(
    records: &Value,
    page: f64,
    keys: &[String],
    position: &Value,
) -> Option<Vec<String>> {
    let byk = by_key(records);
    let on_page = |key: &str| byk.get(key).is_some_and(|s| page_of_dto(dto_of(s)) == page);
    let all = order(records);
    let page_order: Vec<String> = all.iter().filter(|k| on_page(k)).cloned().collect();
    let next = reordered_list(&page_order, keys, position).ok()?;
    Some(reorder_part(&all, &on_page, &next))
}

/// A position whose neighbour is a ref, with the neighbour as its key.
fn keyed_position(position: &Value) -> Value {
    match position {
        Value::Object(o) => {
            if let Some(before) = o.get("before") {
                json!({ "before": crate::js::key_or_none(before) })
            } else {
                json!({ "after": crate::js::key_or_none(o.get("after").unwrap_or(&Value::Null)) })
            }
        }
        other => other.clone(),
    }
}

/// `predictRecords(records, op)`: one op of this session's pending changes,
/// applied as the engine will apply it. An op whose answer the records hold
/// already changes nothing, and so does one the engine would refuse.
pub fn predict_records(records: Rc<Value>, op: &Value) -> Result<Rc<Value>, Error> {
    let ty = op.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "annotations.create" => {
            let Some(object_number) =
                op.get("objectNumber").filter(|n| !epdf_kernel::is_undefined(n))
            else {
                return Ok(records);
            };
            let reference = json!({
                "kind": "objectNumber",
                "page": op.get("page").cloned().unwrap_or(Value::Null),
                "objectNumber": object_number,
            });
            let Some(key) = key_of(&reference) else { return Ok(records) };
            if by_key(&records).contains_key(&key) {
                return Ok(records);
            }
            let draft = op.get("data").and_then(Value::as_object).cloned().unwrap_or_default();
            match annotation_of_create(&draft, &reference) {
                Ok(annotation) => {
                    put(&records, &[Value::Object(annotation)], false).or(Ok(records))
                }
                Err(_) => Ok(records),
            }
        }
        "annotations.update" => {
            let Some(key) = op.get("ref").and_then(key_of) else { return Ok(records) };
            let Some(stored) = record(&records, &key) else { return Ok(records) };
            let current = dto_of(stored).as_object().cloned().unwrap_or_default();
            let patch = op.get("patch").and_then(Value::as_object).cloned().unwrap_or_default();
            match annotation_after(&current, &patch) {
                Ok(after) => put(&records, &[Value::Object(after)], false).or(Ok(records)),
                Err(_) => Ok(records),
            }
        }
        "annotations.delete" => {
            let reference = op.get("ref").cloned().unwrap_or(Value::Null);
            let page = page_number(&reference).unwrap_or(f64::NAN);
            let on_page = annotations_on(&records, page);
            let gone: Vec<String> = deleted_with(&on_page, &reference)
                .into_iter()
                .filter_map(|i| on_page[i].get("ref").and_then(key_of))
                .collect();
            Ok(drop_keys(&records, &gone))
        }
        "annotations.reorder" => {
            let page = op.get("page").and_then(page_number).unwrap_or(f64::NAN);
            let keys: Vec<String> = op
                .get("refs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(crate::js::key_or_none)
                .collect();
            let position = keyed_position(op.get("position").unwrap_or(&Value::Null));
            match reordered_keys(&records, page, &keys, &position) {
                Some(next) if next != order(&records) => {
                    Ok(with_parts(by_key(&records).clone(), next))
                }
                _ => Ok(records),
            }
        }
        "annotations.restore" => {
            let annotation = op.get("annotation").cloned().unwrap_or(Value::Null);
            let key = dto_key(&annotation)?;
            if by_key(&records).contains_key(&key) {
                return Ok(records);
            }
            let page = page_of_dto(&annotation);
            let byk = by_key(&records);
            let all = order(&records);
            let on_page: Vec<&String> = all
                .iter()
                .filter(|k| byk.get(*k).is_some_and(|s| page_of_dto(dto_of(s)) == page))
                .collect();
            let index = op.get("index").and_then(number).unwrap_or(f64::NAN);
            let before = if index >= 0.0 && index.fract() == 0.0 {
                on_page.get(index as usize)
            } else {
                None
            };
            let at = match before {
                Some(b) => all.iter().position(|k| k == *b).unwrap_or(all.len()),
                None => all.len(),
            };
            let mut next_byk = byk.clone();
            next_byk.insert(key.clone(), stored(annotation, 0.0));
            let mut next_order = all[..at].to_vec();
            next_order.push(key);
            next_order.extend_from_slice(&all[at..]);
            Ok(with_parts(next_byk, next_order))
        }
        _ => Ok(records),
    }
}

/// `undoOfOp(records, op)`.
fn undo_of_op(records: &Rc<Value>, op: &Value) -> Vec<Value> {
    match op.get("type").and_then(Value::as_str).unwrap_or("") {
        "annotations.create" => {
            match op.get("objectNumber").filter(|n| !epdf_kernel::is_undefined(n)) {
                None => Vec::new(),
                Some(n) => vec![json!({
                    "type": "annotations.delete",
                    "ref": { "kind": "objectNumber", "page": op.get("page").cloned().unwrap_or(Value::Null), "objectNumber": n },
                })],
            }
        }
        "annotations.update" => {
            let Some(key) = op.get("ref").and_then(key_of) else { return Vec::new() };
            let Some(stored) = record(records, &key) else { return Vec::new() };
            let current = dto_of(stored).as_object().cloned().unwrap_or_default();
            let patch = op.get("patch").and_then(Value::as_object).cloned().unwrap_or_default();
            let Ok(after) = annotation_after(&current, &patch) else { return Vec::new() };
            let mut back = annotation_patch_between(&after, &current);
            back.insert("subtype".into(), current.get("subtype").cloned().unwrap_or(Value::Null));
            vec![json!({ "type": "annotations.update", "ref": op["ref"], "patch": back })]
        }
        "annotations.delete" => {
            let reference = op.get("ref").cloned().unwrap_or(Value::Null);
            let page = page_number(&reference).unwrap_or(f64::NAN);
            let on_page = annotations_on(records, page);
            let gone: HashSet<usize> = deleted_with(&on_page, &reference).into_iter().collect();
            on_page
                .iter()
                .enumerate()
                .filter(|(i, _)| gone.contains(i))
                .map(|(i, a)| json!({ "type": "annotations.restore", "annotation": a, "index": i }))
                .collect()
        }
        "annotations.reorder" => {
            let page = op.get("page").and_then(page_number).unwrap_or(f64::NAN);
            let refs: Vec<Value> = annotations_on(records, page)
                .iter()
                .map(|a| a.get("ref").cloned().unwrap_or(Value::Null))
                .collect();
            vec![json!({
                "type": "annotations.reorder",
                "page": op.get("page").cloned().unwrap_or(Value::Null),
                "refs": refs,
                "position": "start",
            })]
        }
        _ => Vec::new(),
    }
}

/// `undoOf(records, ops)`: the ops that bring the records back, the last op's first.
pub fn undo_of(records: &Rc<Value>, ops: &[Value]) -> Vec<Value> {
    let mut undo: Vec<Vec<Value>> = Vec::new();
    let mut current = records.clone();
    for op in ops {
        undo.push(undo_of_op(&current, op));
        // The TypeScript throws here; the change is checked before it is
        // staged, so a prediction fails only on an op it cannot read, and
        // the records the earlier ops left stand.
        if let Ok(next) = predict_records(current.clone(), op) {
            current = next;
        }
    }
    undo.into_iter().rev().flatten().collect()
}

// ── the mirror ──────────────────────────────────────────────────────────────

/// `createRecordsMirror(ctx, events)`: every change it applies is published
/// on the internal `records_changed` event (`sync/confirmed.rs` reacts).
pub fn create_records_mirror(
    cx: &epdf_kernel::Controller<crate::model::AnnotationState, Value>,
    events: &crate::services::events::Events,
) -> Result<epdf_kernel::mirror::Mirror, Error> {
    use epdf_kernel::mirror::{Loaded, MirrorSpec};
    use epdf_kernel::rt::Promise;
    use std::cell::RefCell;

    // What `records.get()` reads inside a load: the value every applied
    // change left (the mirror announces each one), so the closures need no
    // handle on the mirror itself.
    let latest: Rc<RefCell<Rc<Value>>> = Rc::new(RefCell::new(Rc::new(no_records())));
    let current = {
        let latest = latest.clone();
        move || latest.borrow().clone()
    };
    let load: epdf_kernel::mirror::Load = Rc::new(move |doc, _signal| {
        let Some(doc) = doc else {
            return Promise::rejected(Error::new("[annotation] no document to read"));
        };
        let call = doc.call("annotations.list", None, vec![]);
        let current = current.clone();
        Promise::spawn(async move {
            let list = call.await?;
            let annotations =
                list.get("annotations").and_then(Value::as_array).cloned().unwrap_or_default();
            let value = from_snapshot(&current(), &annotations)?;
            // `cursor: list.auditHead`, as the engine sent it.
            let cursor = list.get("auditHead").cloned();
            Ok(Loaded { value, cursor })
        })
    });
    let fold: Rc<dyn Fn(Rc<Value>, &Value) -> Result<Fold, Error>> = Rc::new(fold_records);
    let mut spec = MirrorSpec::new("records", no_records(), load, fold);
    let readable_cx = cx.cx.clone();
    spec.readable = Some(Rc::new(move || match readable_cx.doc() {
        Ok(doc) => doc.security().allows("doc.annotate.read"),
        Err(_) => true,
    }));
    spec.predict = Some(Rc::new(predict_records));
    spec.to_json = Some(Rc::new(records_json));
    let pages_cx = cx.cx.clone();
    spec.load_pages = Some(Rc::new(move |doc, pages, _signal| {
        let present: Vec<f64> = pages_cx
            .document()
            .and_then(|meta| {
                meta.pages.as_array().map(|list| {
                    list.iter().filter_map(|p| p.get("ref").and_then(page_number)).collect()
                })
            })
            .unwrap_or_default();
        let reading: Vec<Value> = pages
            .iter()
            .filter(|p| page_number(p).is_some_and(|n| present.contains(&n)))
            .cloned()
            .collect();
        let call = match (&doc, reading.is_empty()) {
            (Some(doc), false) => {
                Some(doc.call("annotations.list", None, vec![json!({ "pages": reading })]))
            }
            _ => None,
        };
        Promise::spawn(async move {
            let annotations = match call {
                Some(call) => call
                    .await?
                    .get("annotations")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
                None => Vec::new(),
            };
            let merge: epdf_kernel::mirror::Merge = Rc::new(move |current: Rc<Value>| {
                with_pages(&current, &pages, &annotations).unwrap_or(current)
            });
            Ok(merge)
        })
    }));
    let changed = events.records_changed.clone();
    spec.changed = Some(Rc::new(move |change| {
        *latest.borrow_mut() = change.next.clone();
        changed.emit(&change);
        Ok(())
    }));
    let mirror = cx.mirror(spec)?;
    Ok(mirror)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(n: u32, page: u32) -> Value {
        json!({
            "subtype": "square",
            "ref": { "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": page }, "objectNumber": n },
            "page": { "kind": "objectNumber", "objectNumber": page },
        })
    }

    #[test]
    fn a_dump_shows_the_records_without_their_identity() {
        let records = from_snapshot(&no_records(), &[square(1, 1)]).unwrap();
        assert!(identity(record(&records, "obj:1").unwrap()) > 0);
        let shown = records_json(&records);
        let keys: Vec<&str> =
            shown["byKey"]["obj:1"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["dto", "pictureVersion"]);
        assert_eq!(shown["byKey"]["obj:1"]["dto"], records["byKey"]["obj:1"]["dto"]);
        assert_eq!(shown["order"], json!(["obj:1"]));
        // The value the mirror keeps is untouched.
        assert!(record(&records, "obj:1").unwrap().get("$v").is_some());
    }

    #[test]
    fn a_put_keeps_its_place_and_takes_a_new_identity() {
        let records = Rc::new(from_snapshot(&no_records(), &[square(1, 1), square(2, 1)]).unwrap());
        let before = identity(record(&records, "obj:1").unwrap());
        let next = put(&records, &[square(1, 1)], false).unwrap();
        assert_eq!(order(&next), vec!["obj:1", "obj:2"]);
        assert_ne!(identity(record(&next, "obj:1").unwrap()), before);
    }

    #[test]
    fn a_record_without_a_ref_is_the_typescript_type_error() {
        let error = dto_key(&json!({ "subtype": "square" })).unwrap_err();
        assert_eq!(error.name, "TypeError");
        assert_eq!(error.message, "Cannot read properties of undefined (reading 'kind')");
    }

    #[test]
    fn a_ref_of_unknown_kind_is_refused() {
        let error = dto_key(&json!({ "ref": { "kind": "nm", "nm": "x" } })).unwrap_err();
        assert!(error.message.contains("no annotation key"));
    }

    /// `undoOf` steps each op over the records the earlier ones left. An op
    /// whose prediction fails (a restore with no ref) leaves them as they
    /// were, not as they started: the delete after it still finds the
    /// record the create before it made, and its undo restores it.
    #[test]
    fn a_failed_prediction_keeps_what_the_earlier_ops_predicted() {
        let records = Rc::new(no_records());
        let page = json!({ "kind": "objectNumber", "objectNumber": 1 });
        let ops = vec![
            json!({ "type": "annotations.create", "page": page, "objectNumber": 20,
                    "data": { "subtype": "square", "box": { "x": 0, "y": 0, "width": 10, "height": 10 } } }),
            json!({ "type": "annotations.restore", "annotation": { "subtype": "square" }, "index": 0 }),
            json!({ "type": "annotations.delete",
                    "ref": { "kind": "objectNumber", "page": page, "objectNumber": 20 } }),
        ];
        let undo = undo_of(&records, &ops);
        let kinds: Vec<&str> = undo.iter().filter_map(|op| op["type"].as_str()).collect();
        assert_eq!(kinds, vec!["annotations.restore", "annotations.delete"], "{undo:?}");
    }
}
