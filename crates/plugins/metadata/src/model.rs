//! `model.ts`: `changedKeys`, the one pure function.

use serde_json::{Map, Value};

/// `changedKeys(previous, next)`: the keys whose values differ between two
/// snapshots, removed keys included. With no previous snapshot, every key
/// of `next`. Otherwise the previous keys then the new ones, each once, in
/// that order, kept when `previous[key] !== next[key]`. Key order alone is
/// no change.
///
/// Both snapshots hold strings, `null` and enums, so `!==` is exact; an
/// object or array member, which the engine never sends, compares unequal
/// as two distinct JavaScript objects do. Each snapshot's keys come in
/// `Object.keys` order ([`object_keys`]), whatever order the engine's JSON
/// had. A `previous` or `next` that is not an object has no keys here.
pub fn changed_keys(previous: Option<&Value>, next: &Value) -> Vec<String> {
    let Some(next) = next.as_object() else { return Vec::new() };
    let Some(previous) = previous.and_then(Value::as_object) else {
        return object_keys(next).into_iter().cloned().collect();
    };
    let mut all = object_keys(previous);
    all.extend(object_keys(next).into_iter().filter(|key| !previous.contains_key(key.as_str())));
    all.into_iter()
        .filter(|key| !strict_equals(previous.get(key.as_str()), next.get(key.as_str())))
        .cloned()
        .collect()
}

/// `Object.keys(object)`: array-index keys (canonical numeric strings below
/// 2^32 - 1) first, in ascending numeric order, then the rest in insertion
/// order.
fn object_keys(object: &Map<String, Value>) -> Vec<&String> {
    let mut keys: Vec<&String> = object.keys().collect();
    // Stable: the non-index keys keep their insertion order.
    keys.sort_by_key(|key| array_index(key).map_or((1, 0), |index| (0, index)));
    keys
}

/// The key as an array index, which JavaScript orders before every other
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

/// JavaScript `===` over two members, `None` being `undefined`.
fn strict_equals(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Object(_) | Value::Array(_)), _)
        | (_, Some(Value::Object(_) | Value::Array(_))) => false,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::changed_keys;
    use serde_json::{Map, Value, json};

    #[test]
    fn with_no_previous_every_key_of_next_changed() {
        assert_eq!(changed_keys(None, &json!({ "B": "1", "A": "2" })), ["B", "A"]);
        assert_eq!(changed_keys(Some(&json!(null)), &json!({ "A": "1" })), ["A"]);
    }

    #[test]
    fn key_order_alone_is_no_change() {
        let previous = json!({ "A": "1", "B": "2" });
        assert!(changed_keys(Some(&previous), &json!({ "B": "2", "A": "1" })).is_empty());
    }

    #[test]
    fn removed_added_and_emptied_keys_are_changes_in_previous_then_new_order() {
        let previous = json!({ "B": "2", "A": "1", "D": "4" });
        let next = json!({ "B": "3", "C": "", "D": "4" });
        assert_eq!(changed_keys(Some(&previous), &next), ["B", "A", "C"]);
        // `''` is a value, not an absence.
        assert_eq!(changed_keys(Some(&json!({ "A": "x" })), &json!({ "A": "" })), ["A"]);
    }

    #[test]
    fn keys_come_in_object_keys_order() {
        // Measured on the TypeScript: `changedKeys({Dept:'Ops','2024':'a'},
        // {Dept:'Eng','7':'b'})` is ['2024', 'Dept', '7'].
        let mut previous = Map::new();
        previous.insert("Dept".into(), json!("Ops"));
        previous.insert("2024".into(), json!("a"));
        let mut next = Map::new();
        next.insert("Dept".into(), json!("Eng"));
        next.insert("7".into(), json!("b"));
        let (previous, next) = (Value::Object(previous), Value::Object(next));
        assert_eq!(changed_keys(Some(&previous), &next), ["2024", "Dept", "7"]);
        let mut keys = Map::new();
        for key in ["b", "10", "01", "2", "4294967295", "4294967294", "a", "0"] {
            keys.insert(key.into(), json!(""));
        }
        assert_eq!(
            changed_keys(None, &Value::Object(keys)),
            ["0", "2", "10", "4294967294", "b", "01", "4294967295", "a"]
        );
    }

    #[test]
    fn a_previous_that_is_not_an_object_has_no_keys_here() {
        // Rust-only (README): the TypeScript lists a string's indices too,
        // `changedKeys('ab', {A: '1'})` is ['0', '1', 'A']. No engine sends one.
        assert_eq!(changed_keys(Some(&json!("ab")), &json!({ "A": "1" })), ["A"]);
    }

    #[test]
    fn members_compare_as_triple_equals() {
        let previous = json!({ "n": 1, "o": {}, "t": "unknown", "z": null });
        let next = json!({ "n": 1.0, "o": {}, "t": "unknown", "z": null });
        assert_eq!(changed_keys(Some(&previous), &next), ["o"]);
        assert_eq!(changed_keys(Some(&json!({ "n": 1 })), &json!({ "n": 2 })), ["n"]);
    }
}
