//! Reading engine answers, which this port keeps as JSON, with
//! JavaScript's rules, and the promise shapes the TypeScript chains.

use epdf_kernel::Error;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{is_undefined, undefined};
use serde_json::{Map, Value};

use crate::services::asset_engine::read_off;

/// `null` or `undefined`.
pub fn nullish(value: &Value) -> bool {
    value.is_null() || is_undefined(value)
}

/// `value.member`: `undefined` for a missing member, a `TypeError` reading
/// it off `null` or `undefined`.
pub fn member(value: &Value, key: &str) -> Result<Value, Error> {
    if nullish(value) {
        return Err(read_off(value, key));
    }
    Ok(value.get(key).cloned().unwrap_or_else(undefined))
}

/// `value?.member ?? fallback` for an object member.
pub fn optional_object(value: &Value, key: &str) -> Map<String, Value> {
    if nullish(value) {
        return Map::new();
    }
    value.get(key).and_then(Value::as_object).cloned().unwrap_or_default()
}

/// `ref.objectNumber`, as `doc.page(ref)` reads it: a missing ref is the
/// TypeScript's `TypeError`. A ref the kernel's `PageRef` can't hold (no
/// whole object number) is refused with the port's own `TypeError`, where
/// the TypeScript would call the engine with it.
pub fn page_ref(value: &Value) -> Result<epdf_kernel::geometry::PageRef, Error> {
    if nullish(value) {
        return Err(read_off(value, "objectNumber"));
    }
    epdf_kernel::geometry::PageRef::from_json(value)
        .ok_or_else(|| Error::named("TypeError", format!("[stamp] not a page ref: {value}")))
}

/// `expression.method(...)` on what should be an array: reading the method
/// off `null` or `undefined`, or calling a member that is not a function,
/// is the TypeScript's `TypeError`.
pub fn array<'a>(
    value: &'a Value,
    expression: &str,
    method: &str,
) -> Result<&'a Vec<Value>, Error> {
    if nullish(value) {
        return Err(read_off(value, method));
    }
    value.as_array().ok_or_else(|| {
        Error::named("TypeError", format!("{expression}.{method} is not a function"))
    })
}

/// `return promise` from an `async` function: the resolve-thenable job,
/// then the `then` reaction that settles the function's own promise.
pub async fn adopt<T: Clone + 'static>(promise: Promise<T>) -> Result<T, Error> {
    rt::tick().await;
    promise.reaction().await
}

// ── PieceInfo entries (`write/libraries.ts`) ──────────────────────────────────

fn entry<'a>(entries: &'a Map<String, Value>, key: &str, kind: &str) -> Option<&'a Value> {
    let entry = entries.get(key)?;
    (entry.get("type").and_then(Value::as_str) == Some(kind)).then(|| entry.get("value")).flatten()
}

/// `entryString(entries, key)`: a non-empty string entry.
pub fn entry_string(entries: &Map<String, Value>, key: &str) -> Option<String> {
    entry(entries, key, "string")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `entryName(entries, key)`: a non-empty name entry.
pub fn entry_name(entries: &Map<String, Value>, key: &str) -> Option<String> {
    entry(entries, key, "name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `entryStringArray(entries, key)`: a copy of a string-array entry.
pub fn entry_string_array(entries: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    entry(entries, key, "string-array").and_then(Value::as_array).map(|values| {
        values.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string)).collect()
    })
}
