//! `model.ts`: the outline as the plugin holds it, and the pure functions
//! that read it.
//!
//! The tree keeps every item by key and each parent's children in order, the
//! top level under [`TOP`]; `sync` changes it, by confirmed events and by
//! pending changes. Here: the rows a tree view shows, the item for the
//! reading position, and the session state, which items are expanded and
//! which are selected.
//!
//! The tree is the mirror's value, so it is JSON: `{items, children,
//! incomplete}`, with `items` and `children` keyed by object number as
//! JavaScript keys them (array-index keys first, ascending, then the rest in
//! insertion order; see [`js_ordered`]), so a store dump prints the members
//! in the order the TypeScript's does. Every function that changes nothing
//! returns its input `Rc`, as the TypeScript returns `tree`.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::lanes::js_string;
use epdf_kernel::{is_undefined, undefined};
use serde::Serialize;
use serde_json::{Map, Value, json};

/// An item's key: its object number, as a string.
pub type OutlineKey = String;

/// The key the top level's items are listed under.
pub const TOP: &str = "top";

// ── JavaScript values ─────────────────────────────────────────────────────────

/// JavaScript truthiness of a member (`None` is `undefined`).
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

/// `value === undefined`: missing, or the `$undefined` marker.
pub fn absent(value: Option<&Value>) -> bool {
    value.is_none_or(is_undefined)
}

/// `value.<name>`, `None` when it is missing or `undefined`.
pub fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value.get(name).filter(|v| !is_undefined(v))
}

/// `value.<name>` as a value: `undefined` when it is missing.
pub fn member_or_undefined(value: &Value, name: &str) -> Value {
    member(value, name).cloned().unwrap_or_else(undefined)
}

/// JavaScript `===` between two members, objects and arrays never equal
/// (two values the TypeScript would hold as distinct objects).
pub fn strict_equals(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a.filter(|v| !is_undefined(v)), b.filter(|v| !is_undefined(v))) {
        (None, None) => true,
        (Some(Value::Object(_) | Value::Array(_)), _)
        | (_, Some(Value::Object(_) | Value::Array(_))) => false,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `outlineKeyOf(ref)`: `ref ? String(ref.objectNumber) : TOP`.
pub fn outline_key_of(item_ref: Option<&Value>) -> OutlineKey {
    match item_ref {
        Some(r) if truthy(Some(r)) => js_string(&member_or_undefined(r, "objectNumber")),
        _ => TOP.to_string(),
    }
}

/// `toOutlineItemRef(objectNumber)`.
pub fn to_outline_item_ref(object_number: Value) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": object_number })
}

/// The key as an array index, which JavaScript lists before every other
/// key: `"0"`, or digits with no leading zero, below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    if key == "0" {
        return Some(0);
    }
    if key.is_empty() || key.starts_with('0') || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u64>().ok().filter(|&n| n < u64::from(u32::MAX)).map(|n| n as u32)
}

/// The object with its keys in the order JavaScript enumerates them:
/// array-index keys ascending, then the others in insertion order. The map
/// keeps insertion order, and a replaced key keeps its place, as in
/// JavaScript, so only the index keys need moving.
pub fn js_ordered(map: Map<String, Value>) -> Map<String, Value> {
    let rank = |key: &str| array_index(key).map_or((1, 0), |index| (0, index));
    let sorted = map.keys().zip(map.keys().skip(1)).all(|(a, b)| rank(a) <= rank(b));
    if sorted {
        return map;
    }
    let mut entries: Vec<(String, Value)> = map.into_iter().collect();
    entries.sort_by_key(|(key, _)| rank(key));
    entries.into_iter().collect()
}

/// `delete object[key]`, keeping the order of the rest.
pub fn js_delete(map: &mut Map<String, Value>, key: &str) {
    if map.contains_key(key) {
        map.retain(|k, _| k != key);
    }
}

// ── the tree ──────────────────────────────────────────────────────────────────

/// The tree's `items`.
pub fn items_map(tree: &Value) -> Option<&Map<String, Value>> {
    tree.get("items").and_then(Value::as_object)
}

/// The tree's `children`.
pub fn children_map(tree: &Value) -> Option<&Map<String, Value>> {
    tree.get("children").and_then(Value::as_object)
}

/// `tree.items[key]`.
pub fn item_at<'a>(tree: &'a Value, key: &str) -> Option<&'a Value> {
    items_map(tree).and_then(|items| items.get(key))
}

/// `childrenOf(tree, parent)`: a parent's children in order; a leaf has none.
pub fn children_of(tree: &Value, parent: &str) -> Vec<OutlineKey> {
    children_map(tree)
        .and_then(|children| children.get(parent))
        .and_then(Value::as_array)
        .map(|keys| keys.iter().map(js_string).collect())
        .unwrap_or_default()
}

/// The key list as the tree stores it.
pub fn keys_value(keys: &[OutlineKey]) -> Value {
    Value::Array(keys.iter().map(|k| Value::String(k.clone())).collect())
}

/// `treeOf(outline)`: every item under its parent, in the read's order.
pub fn tree_of(outline: &Value) -> Value {
    let mut items = Map::new();
    let mut children: Map<String, Value> = Map::new();
    let listed = outline.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
    for item in listed {
        let key = outline_key_of(item.get("ref"));
        let parent = outline_key_of(item.get("parent"));
        items.insert(key.clone(), item);
        match children.get_mut(&parent) {
            Some(Value::Array(keys)) => keys.push(Value::String(key)),
            _ => {
                children.insert(parent, json!([key]));
            }
        }
    }
    let mut tree = Map::new();
    tree.insert("items".into(), Value::Object(js_ordered(items)));
    tree.insert("children".into(), Value::Object(js_ordered(children)));
    tree.insert("incomplete".into(), member_or_undefined(outline, "incomplete"));
    Value::Object(tree)
}

/// Depth first from the top level, a parent before its children, siblings
/// in their order: `visit(key, item, depth)` answers whether to go into the
/// item's children. A key listed under two parents is visited under each,
/// as the TypeScript's recursion does; a key that is its own ancestor (a
/// loop, which the TypeScript recurses into until the stack overflows) is
/// not entered again.
fn walk(tree: &Value, mut visit: impl FnMut(&str, &Value, u32, bool) -> bool) {
    let mut frames: Vec<(Vec<OutlineKey>, usize)> = vec![(children_of(tree, TOP), 0)];
    let mut path: Vec<OutlineKey> = Vec::new();
    while let Some((children, next)) = frames.last_mut() {
        let Some(key) = children.get(*next).cloned() else {
            frames.pop();
            path.pop();
            continue;
        };
        *next += 1;
        let Some(item) = item_at(tree, &key) else { continue };
        if path.contains(&key) {
            continue;
        }
        let grandchildren = children_of(tree, &key);
        if visit(&key, item, path.len() as u32, !grandchildren.is_empty()) {
            frames.push((grandchildren, 0));
            path.push(key);
        }
    }
}

/// `itemsOf(tree)`: every item, depth first: a parent before its children,
/// siblings in their order.
pub fn items_of(tree: &Value) -> Vec<Value> {
    let mut list = Vec::new();
    walk(tree, |_, item, _, _| {
        list.push(item.clone());
        true
    });
    list
}

/// `subtreeOf(tree, key)`: `key` and every item under it, breadth first,
/// as the TypeScript lists them. A child that is already one of its own
/// ancestors in this walk (a loop, where the TypeScript never ends) is left
/// out.
pub fn subtree_of(tree: &Value, key: &str) -> Vec<OutlineKey> {
    let mut keys = vec![key.to_string()];
    // The index of the entry each entry was listed under.
    let mut from: Vec<Option<usize>> = vec![None];
    let mut i = 0;
    while i < keys.len() {
        for child in children_of(tree, &keys[i]) {
            let mut at = Some(i);
            let mut looped = false;
            while let Some(j) = at {
                if keys[j] == child {
                    looped = true;
                    break;
                }
                at = from[j];
            }
            if !looped {
                keys.push(child);
                from.push(Some(i));
            }
        }
        i += 1;
    }
    keys
}

/// `ancestorsOf(tree, key)`: the items `key` sits under, nearest first;
/// empty at the top level.
pub fn ancestors_of(tree: &Value, key: &str) -> Vec<OutlineKey> {
    let mut ancestors = Vec::new();
    let mut seen: HashSet<OutlineKey> = HashSet::from([key.to_string()]);
    let mut parent = item_at(tree, key).and_then(|item| item.get("parent")).cloned();
    while let Some(p) = parent.filter(|p| truthy(Some(p))) {
        let parent_key = outline_key_of(Some(&p));
        if !seen.insert(parent_key.clone()) {
            break;
        }
        ancestors.push(parent_key.clone());
        parent = item_at(tree, &parent_key).and_then(|item| item.get("parent")).cloned();
    }
    ancestors
}

/// `topmostOf(tree, refs)`: the refs a delete names, each once, without the
/// ones under another named ref: they go with it.
pub fn topmost_of(tree: &Value, refs: &[Value]) -> Vec<Value> {
    let named: HashSet<OutlineKey> = refs.iter().map(|r| outline_key_of(Some(r))).collect();
    let mut seen = HashSet::new();
    refs.iter()
        .filter(|r| {
            let key = outline_key_of(Some(r));
            if !seen.insert(key.clone()) {
                return false;
            }
            !ancestors_of(tree, &key).iter().any(|ancestor| named.contains(ancestor))
        })
        .cloned()
        .collect()
}

// ── rows ──────────────────────────────────────────────────────────────────────

/// `OutlineRow`: one row of the outline as a tree view draws it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineRow {
    pub item: Value,
    /// How deep it sits: 0 at the top level.
    pub depth: u32,
    pub has_children: bool,
    /// Whether its children show.
    pub expanded: bool,
}

/// `rowsOf(tree, expanded)`: every item whose parents are all expanded,
/// depth first.
pub fn rows_of(tree: &Value, expanded: &Map<String, Value>) -> Vec<OutlineRow> {
    let mut rows = Vec::new();
    walk(tree, |key, item, depth, has_children| {
        let open = has_children && expanded.get(key) == Some(&Value::Bool(true));
        rows.push(OutlineRow { item: item.clone(), depth, has_children, expanded: open });
        open
    });
    rows
}

// ── the reading position ──────────────────────────────────────────────────────

/// `OutlineSpot`: where an item goes, as a place in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineSpot {
    pub key: OutlineKey,
    pub page_index: f64,
    pub y: f64,
}

/// A number member as JavaScript's arithmetic reads it: `null` is 0 and
/// anything that isn't a number is NaN.
fn js_number(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    }
}

/// `left - right` as a sort comparator reads it: NaN compares equal.
fn order_of(difference: f64) -> std::cmp::Ordering {
    difference.partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
}

/// `spotsOf(items, pageIndexOf)`: every item that goes to a page of this
/// document, in reading order: by page, then down the page, then in outline
/// order. An item with no point on its page goes to the page's top.
pub fn spots_of(
    items: &[Value],
    page_index_of: impl Fn(&Value) -> Option<f64>,
) -> Vec<OutlineSpot> {
    let mut spots: Vec<(OutlineSpot, usize)> = Vec::new();
    for (order, item) in items.iter().enumerate() {
        let Some(target) = member(item, "target").filter(|t| !t.is_null()) else { continue };
        if target.get("kind").and_then(Value::as_str) != Some("goto") {
            continue;
        }
        let destination = member_or_undefined(target, "destination");
        let Some(page_index) = page_index_of(&member_or_undefined(&destination, "page")) else {
            continue;
        };
        // `'y' in destination ? (destination.y ?? 0) : 0`
        let y = match destination.get("y") {
            Some(y) if !is_undefined(y) => js_number(Some(y)),
            _ => 0.0,
        };
        let key = outline_key_of(item.get("ref"));
        spots.push((OutlineSpot { key, page_index, y }, order));
    }
    // `Array.prototype.sort` is stable, as `sort_by` is.
    spots.sort_by(|(left, lo), (right, ro)| {
        let by_page = left.page_index - right.page_index;
        let by_y = left.y - right.y;
        let by_order = *lo as f64 - *ro as f64;
        let difference = if by_page != 0.0 && !by_page.is_nan() {
            by_page
        } else if by_y != 0.0 && !by_y.is_nan() {
            by_y
        } else {
            by_order
        };
        order_of(difference)
    });
    spots.into_iter().map(|(spot, _)| spot).collect()
}

/// How far below the top of the view an item's spot may be and still count
/// as reached, in points.
const REACHED_TOLERANCE: f64 = 1.0;

/// `spotAt(spots, pageIndex, y)`: the last spot at or above the reading
/// position, or `None` before the first.
pub fn spot_at(spots: &[OutlineSpot], page_index: f64, y: f64) -> Option<&OutlineSpot> {
    let (mut low, mut high) = (0, spots.len());
    while low < high {
        let middle = (low + high) >> 1;
        let spot = &spots[middle];
        let reached = spot.page_index < page_index
            || (spot.page_index == page_index && spot.y <= y + REACHED_TOLERANCE);
        if reached {
            low = middle + 1;
        } else {
            high = middle;
        }
    }
    (low > 0).then(|| &spots[low - 1])
}

// ── the session ───────────────────────────────────────────────────────────────

/// `OutlineState`: the plugin's slice.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OutlineState {
    /// The items whose children show (`{key: true}`). Starts from the items
    /// the file opens.
    pub expanded: Rc<Map<String, Value>>,
    /// Whether `expanded` started from the file's open items: on the first
    /// load.
    pub seeded: bool,
    pub selected: Rc<Vec<Value>>,
}

/// `initialOutlineState()`.
pub fn initial_outline_state() -> OutlineState {
    OutlineState { expanded: Rc::new(Map::new()), seeded: false, selected: Rc::new(Vec::new()) }
}

/// `seeded(state, tree)`: the first load opens the items the file opens.
/// Later loads keep what the user chose.
pub fn seeded(state: Rc<OutlineState>, tree: &Value) -> Rc<OutlineState> {
    if state.seeded {
        return state;
    }
    let mut expanded = (*state.expanded).clone();
    if let Some(items) = items_map(tree) {
        for (key, item) in items {
            if truthy(item.get("open")) {
                expanded.insert(key.clone(), Value::Bool(true));
            }
        }
    }
    Rc::new(OutlineState {
        expanded: Rc::new(js_ordered(expanded)),
        seeded: true,
        ..(*state).clone()
    })
}

/// `expanded(state, keys)`.
pub fn expanded(state: Rc<OutlineState>, keys: &[OutlineKey]) -> Rc<OutlineState> {
    let added: Vec<&OutlineKey> =
        keys.iter().filter(|key| state.expanded.get(*key) != Some(&Value::Bool(true))).collect();
    if added.is_empty() {
        return state;
    }
    let mut next = (*state.expanded).clone();
    for key in added {
        next.insert(key.clone(), Value::Bool(true));
    }
    Rc::new(OutlineState { expanded: Rc::new(js_ordered(next)), ..(*state).clone() })
}

/// `collapsed(state, keys)`.
pub fn collapsed(state: Rc<OutlineState>, keys: &[OutlineKey]) -> Rc<OutlineState> {
    let removed: Vec<&OutlineKey> =
        keys.iter().filter(|key| state.expanded.get(*key) == Some(&Value::Bool(true))).collect();
    if removed.is_empty() {
        return state;
    }
    let mut next = (*state.expanded).clone();
    for key in removed {
        js_delete(&mut next, key);
    }
    Rc::new(OutlineState { expanded: Rc::new(next), ..(*state).clone() })
}

/// `selected(state, refs)`: the refs, each once, in the order given; the
/// same state when that is the selection already.
pub fn selected(state: Rc<OutlineState>, refs: &[Value]) -> Rc<OutlineState> {
    let keys: Vec<OutlineKey> = refs.iter().map(|r| outline_key_of(Some(r))).collect();
    let unique: Vec<Value> = refs
        .iter()
        .enumerate()
        .filter(|(i, _)| keys.iter().position(|k| *k == keys[*i]) == Some(*i))
        .map(|(_, r)| r.clone())
        .collect();
    let same = unique.len() == state.selected.len()
        && unique
            .iter()
            .zip(state.selected.iter())
            .all(|(a, b)| strict_equals(a.get("objectNumber"), b.get("objectNumber")));
    if same {
        return state;
    }
    Rc::new(OutlineState { selected: Rc::new(unique), ..(*state).clone() })
}

/// `pruned(state, tree)`: forget what names items the tree no longer has.
pub fn pruned(state: Rc<OutlineState>, tree: &Value) -> Rc<OutlineState> {
    let expanded_keys: Vec<OutlineKey> =
        state.expanded.keys().filter(|key| item_at(tree, key).is_none()).cloned().collect();
    let selection: Vec<Value> = state
        .selected
        .iter()
        .filter(|r| item_at(tree, &outline_key_of(Some(r))).is_some())
        .cloned()
        .collect();
    if expanded_keys.is_empty() && selection.len() == state.selected.len() {
        return state;
    }
    let next = collapsed(state.clone(), &expanded_keys);
    if selection.len() == state.selected.len() {
        next
    } else {
        Rc::new(OutlineState { selected: Rc::new(selection), ..(*next).clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: u32, parent: Option<u32>, open: bool) -> Value {
        json!({
            "ref": to_outline_item_ref(json!(n)),
            "parent": parent.map(|p| to_outline_item_ref(json!(p))),
            "title": format!("t{n}"),
            "target": null, "action": null, "open": open,
            "color": null, "bold": false, "italic": false,
        })
    }

    fn tree() -> Value {
        tree_of(&json!({
            "items": [item(20, None, true), item(3, Some(20), false), item(100, Some(3), false), item(7, None, false)],
            "incomplete": false,
        }))
    }

    #[test]
    fn keys_come_in_javascript_order() {
        let t = tree();
        let keys: Vec<&String> = items_map(&t).unwrap().keys().collect();
        assert_eq!(keys, ["3", "7", "20", "100"]);
        let keys: Vec<&String> = children_map(&t).unwrap().keys().collect();
        assert_eq!(keys, ["3", "20", "top"]);
    }

    #[test]
    fn depth_first_and_subtrees() {
        let t = tree();
        let titles: Vec<String> =
            items_of(&t).iter().map(|i| i["title"].as_str().unwrap().to_string()).collect();
        assert_eq!(titles, ["t20", "t3", "t100", "t7"]);
        assert_eq!(subtree_of(&t, "20"), ["20", "3", "100"]);
        assert_eq!(ancestors_of(&t, "100"), ["3", "20"]);
        let refs = [
            to_outline_item_ref(json!(100)),
            to_outline_item_ref(json!(20)),
            to_outline_item_ref(json!(20)),
        ];
        assert_eq!(topmost_of(&t, &refs), [to_outline_item_ref(json!(20))]);
    }

    #[test]
    fn a_key_under_two_parents_is_listed_under_each_and_a_loop_ends() {
        // The TypeScript recurses into every listing; Rust does too, and
        // stops where an item would be its own ancestor.
        let twice = json!({
            "items": { "1": item(1, None, false), "2": item(2, Some(1), false) },
            "children": { "1": ["2"], "top": ["1", "2"] },
            "incomplete": false,
        });
        let titles: Vec<Value> = items_of(&twice).iter().map(|i| i["title"].clone()).collect();
        assert_eq!(titles, [json!("t1"), json!("t2"), json!("t2")]);
        let looped = json!({
            "items": { "1": item(1, Some(2), false), "2": item(2, Some(1), false) },
            "children": { "1": ["2"], "2": ["1"], "top": ["1"] },
            "incomplete": false,
        });
        assert_eq!(items_of(&looped).len(), 2);
        assert_eq!(subtree_of(&looped, "1"), ["1", "2"]);
        let open = Map::from_iter([("1".to_string(), json!(true)), ("2".to_string(), json!(true))]);
        let depths: Vec<u32> = rows_of(&looped, &open).iter().map(|row| row.depth).collect();
        assert_eq!(depths, [0, 1]);
    }

    #[test]
    fn spots_sort_by_page_then_y_then_order() {
        let goto = |n: u32, page: u32, y: Value| {
            let mut i = item(n, None, false);
            i["target"] = json!({"kind":"goto","destination":{"kind":"xyz","page":{"kind":"objectNumber","objectNumber":page},"x":0,"y":y,"zoom":null}});
            i
        };
        let items = [
            goto(1, 2, json!(10)),
            goto(2, 1, json!(500)),
            goto(3, 1, json!(null)),
            goto(4, 9, json!(0)),
        ];
        let spots = spots_of(&items, |page| match page["objectNumber"].as_u64() {
            Some(1) => Some(0.0),
            Some(2) => Some(1.0),
            _ => None,
        });
        let keys: Vec<&str> = spots.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["3", "2", "1"]);
        assert_eq!(spot_at(&spots, 0.0, 0.0).unwrap().key, "3");
        assert_eq!(spot_at(&spots, 0.0, 499.0).unwrap().key, "2");
        assert!(spot_at(&spots, -1.0, 0.0).is_none());
    }

    #[test]
    fn session_transitions_keep_identity_when_nothing_changes() {
        let state = Rc::new(initial_outline_state());
        let t = tree();
        let seeded_once = seeded(state, &t);
        assert_eq!(*seeded_once.expanded, *json!({"20": true}).as_object().unwrap());
        assert!(Rc::ptr_eq(&seeded_once, &seeded(seeded_once.clone(), &t)));
        assert!(Rc::ptr_eq(&seeded_once, &expanded(seeded_once.clone(), &["20".into()])));
        let refs = [to_outline_item_ref(json!(3)), to_outline_item_ref(json!(3))];
        let one = selected(seeded_once, &refs);
        assert_eq!(one.selected.len(), 1);
        assert!(Rc::ptr_eq(&one, &selected(one.clone(), &refs[..1])));
        let gone = pruned(one, &tree_of(&json!({"items": [], "incomplete": false})));
        assert!(gone.expanded.is_empty() && gone.selected.is_empty());
    }
}
