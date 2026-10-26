//! `model.ts`: `changedKeys`, the one pure function.

use serde_json::Value;

/// `changedKeys(previous, next)`: the keys whose values differ between two
/// snapshots, removed keys included. With no previous snapshot, every key
/// of `next`. Otherwise the previous keys then the new ones, each once, in
/// that order, kept when `previous[key] !== next[key]`. Key order alone is
/// no change.
///
/// Both snapshots hold strings, `null` and enums, so `!==` is exact; an
/// object or array member, which the engine never sends, compares unequal
/// as two distinct JavaScript objects do. A `next` that is not an object
/// has no keys here.
pub fn changed_keys(previous: Option<&Value>, next: &Value) -> Vec<String> {
    let Some(next) = next.as_object() else { return Vec::new() };
    let Some(previous) = previous.and_then(Value::as_object) else {
        return next.keys().cloned().collect();
    };
    let mut all: Vec<&String> = previous.keys().collect();
    all.extend(next.keys().filter(|key| !previous.contains_key(key.as_str())));
    all.into_iter()
        .filter(|key| !strict_equals(previous.get(key.as_str()), next.get(key.as_str())))
        .cloned()
        .collect()
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
    use serde_json::json;

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
    fn members_compare_as_triple_equals() {
        let previous = json!({ "n": 1, "o": {}, "t": "unknown", "z": null });
        let next = json!({ "n": 1.0, "o": {}, "t": "unknown", "z": null });
        assert_eq!(changed_keys(Some(&previous), &next), ["o"]);
        assert_eq!(changed_keys(Some(&json!({ "n": 1 })), &json!({ "n": 2 })), ["n"]);
    }
}
