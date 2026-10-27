//! `sync/tree.ts`: how the outline tree changes. [`fold_outline`] applies
//! one confirmed event's facts, the mirror's `fold`; [`predict_outline`]
//! applies one op of a pending change by the engine's rules (engine-core's
//! `outlineItemOfDraft`, `outlineItemAfter` and `outlineItemWithChildren`,
//! ported here), the mirror's `predict`; [`undo_of`] says what undoing ops
//! looks like. Pure: each returns its input `Rc` when nothing changes.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::Error;
use epdf_kernel::lanes::js_string;
use epdf_kernel::mirror::Fold;
use serde_json::{Map, Value, json};

use crate::model::*;

thread_local! {
    static TREE_COPIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times this thread deep-copied a whole outline tree (its items)
/// to fold or predict a change. The guard (`tree_copies_per_change` below)
/// holds it at one per change that changes the tree, none for one that
/// doesn't, and one for a run of predictions replayed over each other.
#[cfg(test)]
fn tree_copies() -> usize {
    TREE_COPIES.with(std::cell::Cell::get)
}

fn count_copy() {
    TREE_COPIES.with(|n| n.set(n.get() + 1));
}

// ── engine-core's outline rules (`outline/predict.ts`, `PdfLinkTarget.ts`) ───

/// `actionTreeOf(target)`: the `/A` a target is written as, as a read of it
/// returns it.
pub fn action_tree_of(target: &Value) -> Value {
    let root = match target.get("kind").and_then(Value::as_str) {
        Some("uri") => json!({
            "type": "uri", "subtype": "URI", "uri": member_or_undefined(target, "uri"),
            "isMap": false, "next": [],
        }),
        Some("goto") => json!({
            "type": "goto", "subtype": "GoTo",
            "destination": member_or_undefined(target, "destination"), "next": [],
        }),
        _ => json!({
            "type": "named", "subtype": "Named", "name": member_or_undefined(target, "name"),
            "next": [],
        }),
    };
    json!({ "root": root, "incomplete": false, "warningFlags": 0, "warnings": [] })
}

/// `targetFieldsOf(target)`: the target, and the `/A` it is written as.
fn target_fields_of(target: Option<&Value>) -> (Value, Value) {
    match target.filter(|t| truthy(Some(t))) {
        Some(target) => (target.clone(), action_tree_of(target)),
        None => (Value::Null, Value::Null),
    }
}

/// `colorAsRead(color)`: `#rrggbb` in lowercase digits, or `null`. A truthy
/// colour that isn't a string throws as `color.toLowerCase()` does.
fn color_as_read(color: Option<&Value>) -> Result<Value, Error> {
    match color {
        Some(Value::String(s)) if !s.is_empty() => Ok(Value::String(s.to_lowercase())),
        other if truthy(other) => {
            Err(Error::named("TypeError", "color.toLowerCase is not a function"))
        }
        _ => Ok(Value::Null),
    }
}

/// `outlineItemOfDraft(ref, parent, draft)`: the new item. It has no
/// children yet, so it isn't open.
pub fn outline_item_of_draft(
    item_ref: Value,
    parent: Value,
    draft: &Value,
) -> Result<Value, Error> {
    let (target, action) = target_fields_of(member(draft, "target"));
    let or_false = |name: &str| {
        member(draft, name).filter(|v| !v.is_null()).cloned().unwrap_or(Value::Bool(false))
    };
    Ok(json!({
        "ref": item_ref,
        "parent": parent,
        "title": member_or_undefined(draft, "title"),
        "target": target,
        "action": action,
        "open": false,
        "color": color_as_read(member(draft, "color"))?,
        "bold": or_false("bold"),
        "italic": or_false("italic"),
    }))
}

/// `outlineItemAfter(item, patch, hasChildren)`: the item with the fields
/// the patch names; `None` when the patch changes nothing (the TypeScript
/// returns `item` itself). `open` holds only on an item with children.
pub fn outline_item_after(
    item: &Value,
    patch: &Value,
    has_children: bool,
) -> Result<Option<Value>, Error> {
    let mut next = item.as_object().cloned().unwrap_or_default();
    if let Some(title) = member(patch, "title") {
        next.insert("title".into(), title.clone());
    }
    let target_given = member(patch, "target").is_some();
    if target_given {
        let (target, action) = target_fields_of(member(patch, "target"));
        next.insert("target".into(), target);
        next.insert("action".into(), action);
    }
    if let Some(color) = member(patch, "color") {
        next.insert("color".into(), color_as_read(Some(color))?);
    }
    if let Some(bold) = member(patch, "bold") {
        next.insert("bold".into(), bold.clone());
    }
    if let Some(italic) = member(patch, "italic") {
        next.insert("italic".into(), italic.clone());
    }
    if let Some(open) = member(patch, "open") {
        // `patch.open && hasChildren`
        let value = if truthy(Some(open)) { Value::Bool(has_children) } else { open.clone() };
        next.insert("open".into(), value);
    }
    // A member the patch left alone is the same value; one it set compares
    // with `===`.
    let same_member =
        |name: &str| member(patch, name).is_none() || strict_equals(next.get(name), item.get(name));
    let same =
        !target_given && ["title", "color", "bold", "italic", "open"].into_iter().all(same_member);
    Ok((!same).then_some(Value::Object(next)))
}

/// `outlineItemWithChildren(item, hasChildren)`: one left without children
/// is no longer open; `None` when that changes nothing.
fn outline_item_with_children(item: &Value, has_children: bool) -> Option<Value> {
    if truthy(item.get("open")) && !has_children {
        let mut next = item.clone();
        next["open"] = Value::Bool(false);
        Some(next)
    } else {
        None
    }
}

/// `positionIndex(order, position, keyOf)` over keys: where `position`
/// falls, `None` where the TypeScript throws (a neighbour that isn't one of
/// them, or a position that is neither a neighbour nor `start`/`end`).
fn index_at(keys: &[OutlineKey], position: &Value) -> Option<usize> {
    match position {
        Value::String(s) if s == "start" => Some(0),
        Value::String(s) if s == "end" => Some(keys.len()),
        Value::Object(o) if !epdf_kernel::is_undefined(position) => {
            let before = o.contains_key("before");
            let anchor = if before { o.get("before") } else { o.get("after") };
            let anchor = outline_key_of(anchor);
            let at = keys.iter().position(|key| *key == anchor)?;
            Some(if before { at } else { at + 1 })
        }
        _ => None,
    }
}

fn inserted(keys: &[OutlineKey], at: usize, added: &[OutlineKey]) -> Vec<OutlineKey> {
    let mut out = keys[..at].to_vec();
    out.extend_from_slice(added);
    out.extend_from_slice(&keys[at..]);
    out
}

// ── tree edits ────────────────────────────────────────────────────────────────
//
// The TypeScript's edits spread the tree (`{ ...tree, items: { ...tree.items,
// [key]: item } }`), sharing every item. A JSON tree can't share its items,
// so a change copies the tree once ([`owned`]) and the edits below change
// that copy in place. A change that changes nothing copies nothing.

/// The tree to change: the mirror's own when nothing else holds it (a run
/// of predictions replayed over each other), else one copy of it.
fn owned(tree: Rc<Value>) -> Value {
    Rc::try_unwrap(tree).unwrap_or_else(|shared| {
        count_copy();
        (*shared).clone()
    })
}

/// The tree's `name` member as an object, made empty where it is missing
/// (`{ ...tree, [name]: { ...tree[name] } }`).
fn member_mut<'a>(tree: &'a mut Value, name: &str) -> &'a mut Map<String, Value> {
    if !tree.is_object() {
        *tree = Value::Object(Map::new());
    }
    let object = tree.as_object_mut().expect("an object");
    let entry = object.entry(name).or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    entry.as_object_mut().expect("an object")
}

/// Put a member map back in JavaScript key order.
fn reorder(map: &mut Map<String, Value>) {
    *map = js_ordered(std::mem::take(map));
}

/// `withChildren(tree, parent, keys)`: `parent`'s children set, and its open
/// state kept true to them.
fn with_children(tree: &mut Value, parent: &str, keys: &[OutlineKey]) {
    let opened =
        item_at(tree, parent).and_then(|item| outline_item_with_children(item, !keys.is_empty()));
    let children = member_mut(tree, "children");
    if keys.is_empty() {
        js_delete(children, parent);
    } else {
        children.insert(parent.to_string(), keys_value(keys));
    }
    reorder(children);
    if let Some(opened) = opened {
        member_mut(tree, "items").insert(parent.to_string(), opened);
    }
}

/// `withItems(tree, items)`: these items put in, replacing ones with the
/// same key. Nothing for no items.
fn with_items(tree: &mut Value, items: Vec<Value>) {
    if items.is_empty() {
        return;
    }
    let map = member_mut(tree, "items");
    for item in items {
        map.insert(outline_key_of(item.get("ref")), item);
    }
    reorder(map);
}

/// `withoutSubtree(tree, key)`: `key` and the items under it gone; their
/// parent keeps its other children. False, and nothing changed, for a key
/// the tree lacks.
fn without_subtree(tree: &mut Value, key: &str) -> bool {
    let Some(item) = item_at(tree, key) else { return false };
    let parent = outline_key_of(item.get("parent"));
    let gone = subtree_of(tree, key);
    let siblings: Vec<OutlineKey> =
        children_of(tree, &parent).into_iter().filter(|child| child != key).collect();
    let gone_set: HashSet<&String> = gone.iter().collect();
    member_mut(tree, "items").retain(|k, _| !gone_set.contains(k));
    member_mut(tree, "children").retain(|k, _| !gone_set.contains(k));
    with_children(tree, &parent, &siblings);
    true
}

/// `withRestored(tree, items)`: each item as given, and the children of each
/// restored item rebuilt from the items' parents, in the given order. The
/// first item's place among its parent's children is set by the caller.
fn with_restored(tree: &mut Value, items: &[Value]) {
    with_items(tree, items.to_vec());
    // `new Set(keys)`: insertion order, each once.
    let mut keys: Vec<OutlineKey> = Vec::new();
    for item in items {
        let key = outline_key_of(item.get("ref"));
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    let mut rebuilt: Vec<(OutlineKey, Vec<OutlineKey>)> = Vec::new();
    for item in items.iter().skip(1) {
        let parent = outline_key_of(item.get("parent"));
        if keys.contains(&parent) {
            let key = outline_key_of(item.get("ref"));
            match rebuilt.iter_mut().find(|(p, _)| *p == parent) {
                Some((_, list)) => list.push(key),
                None => rebuilt.push((parent, vec![key])),
            }
        }
    }
    let children = member_mut(tree, "children");
    for key in &keys {
        match rebuilt.iter().find(|(p, _)| p == key) {
            Some((_, list)) => {
                children.insert(key.clone(), keys_value(list));
            }
            None => js_delete(children, key),
        }
    }
    reorder(children);
}

/// `reparented`: the moved items whose parent isn't `parent` any more, each
/// with its new parent.
fn reparented<'a>(
    tree: &Value,
    moved: impl Iterator<Item = &'a OutlineKey>,
    target: &str,
    parent: &Value,
) -> Vec<Value> {
    moved
        .filter_map(|key| {
            let item = item_at(tree, key)?;
            (outline_key_of(item.get("parent")) != target).then(|| {
                let mut item = item.clone();
                item["parent"] = parent.clone();
                item
            })
        })
        .collect()
}

fn keys_of(refs: Option<&Value>) -> Vec<OutlineKey> {
    refs.and_then(Value::as_array)
        .map(|refs| refs.iter().map(|r| outline_key_of(Some(r))).collect())
        .unwrap_or_default()
}

/// `TypeError: Cannot read properties of undefined (reading '<member>')`:
/// where the TypeScript reads a member of something an event lacks.
fn missing(member_name: &str) -> Error {
    Error::named(
        "TypeError",
        format!("Cannot read properties of undefined (reading '{member_name}')"),
    )
}

fn array_member<'a>(value: &'a Value, name: &str) -> Result<&'a Vec<Value>, Error> {
    member(value, name)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::named("TypeError", format!("{name} is not iterable")))
}

fn object_member<'a>(value: &'a Value, name: &str) -> Result<&'a Value, Error> {
    member(value, name).filter(|v| !v.is_null()).ok_or_else(|| missing(name))
}

// ── confirmed events ──────────────────────────────────────────────────────────

/// `foldOutline(tree, event)`: one confirmed event, applied with the facts it
/// carries, the same for every origin. Before the first load there is
/// nothing to apply it to. A page delete or restore changes where items go,
/// so the outline is read again.
pub fn fold_outline(tree: Rc<Value>, event: &Value) -> Result<Fold, Error> {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or_default();
    if matches!(kind, "pages.deleted" | "pages.restored") {
        return Ok(if tree.is_null() { Fold::Value(tree) } else { Fold::Reload });
    }
    if tree.is_null() {
        return Ok(Fold::Value(tree));
    }
    let next = match kind {
        "outline.created" => {
            let item = object_member(event, "item")?.clone();
            let order = object_member(event, "order")?;
            let parent = outline_key_of(order.get("parent"));
            let keys: Vec<OutlineKey> =
                array_member(order, "children")?.iter().map(|r| outline_key_of(Some(r))).collect();
            let mut next = owned(tree);
            with_items(&mut next, vec![item]);
            with_children(&mut next, &parent, &keys);
            next
        }
        "outline.updated" => {
            let item = object_member(event, "item")?.clone();
            let mut next = owned(tree);
            with_items(&mut next, vec![item]);
            next
        }
        "outline.moved" => {
            let orders = array_member(event, "orders")?;
            for order in orders {
                array_member(order, "children")?;
            }
            let mut next = owned(tree);
            for order in orders {
                let parent = outline_key_of(order.get("parent"));
                let keys: Vec<OutlineKey> = array_member(order, "children")?
                    .iter()
                    .map(|r| outline_key_of(Some(r)))
                    .collect();
                with_children(&mut next, &parent, &keys);
                let parent_ref = order.get("parent").cloned().unwrap_or(Value::Null);
                let moved = reparented(&next, keys.iter(), &parent, &parent_ref);
                with_items(&mut next, moved);
            }
            next
        }
        "outline.deleted" => {
            let deleted = array_member(event, "deleted")?;
            let Some(first) = deleted.first() else { return Ok(Fold::Value(tree)) };
            let key = outline_key_of(Some(first));
            if item_at(&tree, &key).is_none() {
                return Ok(Fold::Value(tree));
            }
            let mut next = owned(tree);
            without_subtree(&mut next, &key);
            next
        }
        "outline.restored" => {
            let items = array_member(event, "items")?;
            let order = object_member(event, "order")?;
            let keys: Vec<OutlineKey> =
                array_member(order, "children")?.iter().map(|r| outline_key_of(Some(r))).collect();
            let mut next = owned(tree);
            with_restored(&mut next, items);
            with_children(&mut next, &outline_key_of(order.get("parent")), &keys);
            next
        }
        _ => return Ok(Fold::Value(tree)),
    };
    Ok(Fold::Value(Rc::new(next)))
}

// ── pending changes ───────────────────────────────────────────────────────────

/// `predictOutline(tree, op)`: one op of a change this session staged, shown
/// before the engine answers, by the engine's rules. An op the tree already
/// holds changes nothing; an op the engine would refuse leaves the tree as
/// it is. A tree nothing else holds is changed in place.
pub fn predict_outline(tree: Rc<Value>, op: &Value) -> Result<Rc<Value>, Error> {
    if tree.is_null() {
        return Ok(tree);
    }
    let next = match op.get("type").and_then(Value::as_str) {
        Some("outline.create") => {
            let Some(number) = member(op, "objectNumber") else { return Ok(tree) };
            let item_ref = to_outline_item_ref(number.clone());
            let key = outline_key_of(Some(&item_ref));
            let parent = outline_key_of(op.get("parent"));
            if item_at(&tree, &key).is_some()
                || (parent != TOP && item_at(&tree, &parent).is_none())
            {
                return Ok(tree);
            }
            let siblings = children_of(&tree, &parent);
            let position = member(op, "position").cloned().unwrap_or_else(|| json!("end"));
            let Some(at) = index_at(&siblings, &position) else { return Ok(tree) };
            let parent_ref = op.get("parent").cloned().unwrap_or_else(epdf_kernel::undefined);
            let item =
                outline_item_of_draft(item_ref, parent_ref, &member_or_undefined(op, "data"))?;
            let mut next = owned(tree);
            with_items(&mut next, vec![item]);
            with_children(&mut next, &parent, &inserted(&siblings, at, &[key]));
            next
        }
        Some("outline.update") => {
            let key = outline_key_of(op.get("ref"));
            let Some(item) = item_at(&tree, &key) else { return Ok(tree) };
            let has_children = !children_of(&tree, &key).is_empty();
            match outline_item_after(item, &member_or_undefined(op, "patch"), has_children)? {
                Some(item) => {
                    let mut next = owned(tree);
                    with_items(&mut next, vec![item]);
                    next
                }
                None => return Ok(tree),
            }
        }
        Some("outline.move") => {
            let moved = keys_of(op.get("refs"));
            let parent = op.get("parent").cloned().unwrap_or_else(epdf_kernel::undefined);
            let position = member_or_undefined(op, "position");
            let Some(plan) = move_plan(&tree, &moved, &parent, &position) else {
                return Ok(tree);
            };
            let mut next = owned(tree);
            moved_tree(&mut next, &moved, &parent, plan);
            next
        }
        Some("outline.delete") => {
            let key = outline_key_of(op.get("ref"));
            if item_at(&tree, &key).is_none() {
                return Ok(tree);
            }
            let mut next = owned(tree);
            without_subtree(&mut next, &key);
            next
        }
        Some("outline.restore") => {
            let items = op.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
            let Some(first) = items.first() else { return Ok(tree) };
            let parent = outline_key_of(first.get("parent"));
            if parent != TOP && item_at(&tree, &parent).is_none() {
                return Ok(tree);
            }
            let mut next = owned(tree);
            restored_tree(&mut next, &items, op.get("index"));
            next
        }
        _ => return Ok(tree),
    };
    Ok(Rc::new(next))
}

/// What a move does: the target's key, its new children, and the parents
/// the moved items leave, in the order `new Set(moved.map(parent key))`
/// gives them.
struct MovePlan {
    target: OutlineKey,
    order: Vec<OutlineKey>,
    left: Vec<OutlineKey>,
}

/// `movedTree`'s checks: `None` where the TypeScript returns `tree`.
fn move_plan(
    tree: &Value,
    moved: &[OutlineKey],
    parent: &Value,
    position: &Value,
) -> Option<MovePlan> {
    let target = outline_key_of(Some(parent));
    if target != TOP && item_at(tree, &target).is_none() {
        return None;
    }
    let moved_set: HashSet<&OutlineKey> = moved.iter().collect();
    if moved_set.len() != moved.len() || moved.iter().any(|key| item_at(tree, key).is_none()) {
        return None;
    }
    // No item under itself, and no item together with one it holds.
    for key in moved {
        if *key == target || ancestors_of(tree, key).iter().any(|a| moved_set.contains(a)) {
            return None;
        }
    }
    if ancestors_of(tree, &target).iter().any(|a| moved_set.contains(a)) {
        return None;
    }
    let current = children_of(tree, &target);
    let staying: Vec<OutlineKey> =
        current.iter().filter(|k| !moved_set.contains(k)).cloned().collect();
    let at = index_at(&staying, position)?;
    let order = inserted(&staying, at, moved);
    if order == current {
        return None;
    }
    let mut left: Vec<OutlineKey> = Vec::new();
    for key in moved {
        let from = outline_key_of(item_at(tree, key).and_then(|item| item.get("parent")));
        if from != target && !left.contains(&from) {
            left.push(from);
        }
    }
    Some(MovePlan { target, order, left })
}

/// `movedTree`: the items, with the items under them, at the planned place
/// among `parent`'s children.
fn moved_tree(tree: &mut Value, moved: &[OutlineKey], parent: &Value, plan: MovePlan) {
    let moved_set: HashSet<&OutlineKey> = moved.iter().collect();
    for old in plan.left {
        let kept: Vec<OutlineKey> =
            children_of(tree, &old).into_iter().filter(|k| !moved_set.contains(k)).collect();
        with_children(tree, &old, &kept);
    }
    with_children(tree, &plan.target, &plan.order);
    let reparented = reparented(tree, moved.iter(), &plan.target, parent);
    with_items(tree, reparented);
}

/// `Math.max(0, Math.min(index, length))`, NaN (and so a missing index)
/// reading as 0 the way `slice` reads it.
fn clamp_index(index: Option<&Value>, length: usize) -> usize {
    let index = index.and_then(Value::as_f64).unwrap_or(f64::NAN);
    if index.is_nan() {
        return 0;
    }
    let clamped = index.min(length as f64).max(0.0);
    // `slice` truncates a fraction toward zero.
    clamped.trunc() as usize
}

/// `restoredTree(tree, items, index)`: the items as they were, the first at
/// `index` among its parent's children, wherever they are now and whether
/// or not they are here. The caller has checked the first item's parent.
fn restored_tree(tree: &mut Value, items: &[Value], index: Option<&Value>) {
    let Some(first) = items.first() else { return };
    let parent = outline_key_of(first.get("parent"));
    let key = outline_key_of(first.get("ref"));
    // Take out what is here now, so nothing is listed twice.
    for item in items {
        let item_key = outline_key_of(item.get("ref"));
        let Some(current) = item_at(tree, &item_key) else { continue };
        let from = outline_key_of(current.get("parent"));
        let kept: Vec<OutlineKey> =
            children_of(tree, &from).into_iter().filter(|child| *child != item_key).collect();
        with_children(tree, &from, &kept);
    }
    let siblings = children_of(tree, &parent);
    let at = clamp_index(index, siblings.len());
    with_restored(tree, items);
    with_children(tree, &parent, &inserted(&siblings, at, &[key]));
}

/// `restoreOf(tree, key)`: the item and the items under it, as the tree holds
/// them: what a restore brings back.
fn restore_of(tree: &Value, key: &str) -> Option<Value> {
    let item = item_at(tree, key)?;
    let items: Vec<Value> =
        subtree_of(tree, key).iter().filter_map(|child| item_at(tree, child).cloned()).collect();
    let siblings = children_of(tree, &outline_key_of(item.get("parent")));
    // `indexOf`: -1 when the parent doesn't list it.
    let index = siblings.iter().position(|k| k == key).map_or(-1, |i| i as i64);
    Some(json!({ "type": "outline.restore", "items": items, "index": index }))
}

/// `undoOfOp(tree, op)`: what undoing one op looks like, read off the tree it
/// applies to.
fn undo_of_op(tree: &Value, op: &Value) -> Vec<Value> {
    match op.get("type").and_then(Value::as_str) {
        Some("outline.create") => match member(op, "objectNumber") {
            None => Vec::new(),
            Some(number) => vec![
                json!({ "type": "outline.delete", "ref": to_outline_item_ref(number.clone()) }),
            ],
        },
        Some("outline.update" | "outline.delete") => {
            restore_of(tree, &outline_key_of(op.get("ref"))).into_iter().collect()
        }
        Some("outline.move") => {
            // Each item back at its place, the first places first, so each
            // index holds.
            let mut restores: Vec<Value> = op
                .get("refs")
                .and_then(Value::as_array)
                .map(|refs| {
                    refs.iter().filter_map(|r| restore_of(tree, &outline_key_of(Some(r)))).collect()
                })
                .unwrap_or_default();
            let index = |v: &Value| v.get("index").and_then(Value::as_f64).unwrap_or(f64::NAN);
            restores.sort_by(|a, b| {
                (index(a) - index(b)).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
            });
            restores
        }
        _ => Vec::new(),
    }
}

/// `undoOf(tree, ops)`: what undoing `ops` looks like, each op read off the
/// tree the ops before it leave; the last op is undone first. An op the
/// prediction throws on (a colour that isn't a string) throws here, as
/// `predictOutline` does inside the TypeScript's `undoOf`.
pub fn undo_of(tree: &Rc<Value>, ops: &[Value]) -> Result<Vec<Value>, Error> {
    if tree.is_null() {
        return Ok(Vec::new());
    }
    let mut undo: Vec<Vec<Value>> = Vec::new();
    let mut current = tree.clone();
    for op in ops {
        undo.push(undo_of_op(&current, op));
        // `predictOutline(current, op) ?? current`: a throw leaves `undoOf`.
        current = predict_outline(current, op)?;
    }
    Ok(undo.into_iter().rev().flatten().collect())
}

/// `js_string` of a key, re-exported for the controller's messages.
pub(crate) fn number_text(value: Option<&Value>) -> String {
    js_string(&value.cloned().unwrap_or_else(epdf_kernel::undefined))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: u32, parent: Option<u32>) -> Value {
        json!({
            "ref": to_outline_item_ref(json!(n)),
            "parent": parent.map(|p| to_outline_item_ref(json!(p))),
            "title": format!("t{n}"), "target": null, "action": null, "open": false,
            "color": null, "bold": false, "italic": false,
        })
    }

    fn tree() -> Rc<Value> {
        let mut open = item(1, None);
        open["open"] = json!(true);
        Rc::new(tree_of(&json!({
            "items": [open, item(2, Some(1)), item(3, Some(1)), item(4, None)],
            "incomplete": false,
        })))
    }

    fn r(n: u32) -> Value {
        to_outline_item_ref(json!(n))
    }

    #[test]
    fn create_goes_where_its_position_says_and_refuses_an_unknown_neighbour() {
        let t = tree();
        let create = |position: Value| {
            json!({"type": "outline.create", "parent": r(1), "position": position,
                   "data": {"title": "N", "color": "#ABCDEF"}, "objectNumber": 9})
        };
        let next = predict_outline(t.clone(), &create(json!({"after": r(2)}))).unwrap();
        assert_eq!(children_of(&next, "1"), ["2", "9", "3"]);
        assert_eq!(item_at(&next, "9").unwrap()["color"], "#abcdef");
        let next = predict_outline(t.clone(), &create(json!("start"))).unwrap();
        assert_eq!(children_of(&next, "1"), ["9", "2", "3"]);
        let same = predict_outline(t.clone(), &create(json!({"before": r(4)}))).unwrap();
        assert!(Rc::ptr_eq(&same, &t));
    }

    #[test]
    fn a_move_under_itself_changes_nothing_and_a_real_move_closes_the_old_parent() {
        let t = tree();
        let under_itself =
            json!({"type": "outline.move", "refs": [r(1)], "parent": r(2), "position": "end"});
        assert!(Rc::ptr_eq(&predict_outline(t.clone(), &under_itself).unwrap(), &t));
        let out = json!({"type": "outline.move", "refs": [r(2), r(3)], "parent": null, "position": "start"});
        let next = predict_outline(t.clone(), &out).unwrap();
        assert_eq!(children_of(&next, TOP), ["2", "3", "1", "4"]);
        assert_eq!(item_at(&next, "1").unwrap()["open"], false);
        assert_eq!(item_at(&next, "2").unwrap()["parent"], Value::Null);
    }

    #[test]
    fn undoing_a_delete_restores_the_subtree_in_place() {
        let t = tree();
        let ops = [json!({"type": "outline.delete", "ref": r(1)})];
        let undo = undo_of(&t, &ops).unwrap();
        assert_eq!(undo.len(), 1);
        assert_eq!(undo[0]["index"], 0);
        let gone = predict_outline(t.clone(), &ops[0]).unwrap();
        assert_eq!(children_of(&gone, TOP), ["4"]);
        let back = predict_outline(gone, &undo[0]).unwrap();
        assert_eq!(children_of(&back, TOP), ["1", "4"]);
        assert_eq!(children_of(&back, "1"), ["2", "3"]);
    }

    #[test]
    fn a_colour_that_is_not_a_string_throws_as_to_lower_case_does() {
        let t = tree();
        let op = json!({"type": "outline.update", "ref": r(2), "patch": {"color": 5}});
        let error = predict_outline(t.clone(), &op).unwrap_err();
        assert_eq!(
            (error.name.as_str(), error.message.as_str()),
            ("TypeError", "color.toLowerCase is not a function")
        );
        assert!(undo_of(&t, &[op]).is_err());
        let cleared = json!({"type": "outline.update", "ref": r(2), "patch": {"color": null}});
        assert!(predict_outline(t, &cleared).is_ok());
    }

    #[test]
    fn an_update_that_changes_nothing_keeps_the_tree() {
        let t = tree();
        let op =
            json!({"type": "outline.update", "ref": r(2), "patch": {"title": "t2", "open": true}});
        assert!(Rc::ptr_eq(&predict_outline(t.clone(), &op).unwrap(), &t));
        let op = json!({"type": "outline.update", "ref": r(1), "patch": {"open": false}});
        assert_eq!(item_at(&predict_outline(t, &op).unwrap(), "1").unwrap()["open"], false);
    }

    /// M39: a fold or a prediction copies the tree once when it changes
    /// it and not at all when it doesn't, and a run of predictions over
    /// each other (the view's replay, `undoOf`) copies it once. The
    /// TypeScript spreads `items` and `children` per change (O(n) pointer
    /// copies); this port copies the items once (O(n) deep).
    #[test]
    fn tree_copies_per_change() {
        let mut listed = Vec::new();
        for n in 1..=1000u32 {
            listed.push(item(n, (n % 10 != 1).then(|| n - (n - 1) % 10)));
        }
        let t = Rc::new(tree_of(&json!({ "items": listed, "incomplete": false })));
        let ops = [
            json!({"type": "outline.create", "parent": r(1), "position": "end", "data": {"title": "N"}, "objectNumber": 5001}),
            json!({"type": "outline.update", "ref": r(2), "patch": {"title": "renamed"}}),
            json!({"type": "outline.move", "refs": [r(3)], "parent": r(11), "position": "start"}),
            json!({"type": "outline.delete", "ref": r(21)}),
        ];
        let before = tree_copies();
        for op in &ops {
            let next = predict_outline(t.clone(), op).unwrap();
            assert!(!Rc::ptr_eq(&next, &t));
        }
        assert_eq!(tree_copies(), before + ops.len(), "one copy per prediction");
        let unchanged = json!({"type": "outline.update", "ref": r(2), "patch": {"title": "t2"}});
        assert!(Rc::ptr_eq(&predict_outline(t.clone(), &unchanged).unwrap(), &t));
        assert_eq!(tree_copies(), before + ops.len(), "none for a prediction that changes nothing");
        let before = tree_copies();
        let mut replayed = t.clone();
        for op in &ops {
            replayed = predict_outline(replayed, op).unwrap();
        }
        assert_eq!(tree_copies(), before + 1, "a replay copies the tree once");
        let undo = undo_of(&t, &ops).unwrap();
        assert_eq!(undo.len(), 4);
        assert_eq!(tree_copies(), before + 2, "undoOf copies it once");
        let before = tree_copies();
        let mut folded = t.clone();
        for n in 1..=10u32 {
            let event = json!({"type": "outline.updated", "item": item(n * 7, None)});
            let Fold::Value(next) = fold_outline(folded.clone(), &event).unwrap() else { panic!() };
            folded = next;
        }
        assert_eq!(tree_copies(), before + 10, "one copy per fold");
    }
}
