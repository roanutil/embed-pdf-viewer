//! Small helpers for the engine's JSON: refs and their keys, pages, and the
//! JavaScript numerics the plugin's reads print.

use serde_json::{Map, Value};

pub use epdf_annotation_core::json::{Object, num, number};

/// `annotationKey(ref)`: `obj:<n>` or `base:<page>:<i>`. A ref of another
/// kind has no key (review finding N6, a kept Rust-only refusal): the
/// TypeScript keys it `"undefined"`, which would collapse every such record
/// into one.
pub fn key_of(reference: &Value) -> Option<String> {
    epdf_annotation_core::record::annotation_key(reference)
}

/// A ref's key, or one no record has (so a lookup misses), for a ref of a
/// kind no key is defined for.
pub fn key_or_none(reference: &Value) -> String {
    key_of(reference).unwrap_or_else(|| "\u{0}no-key".to_string())
}

/// `ref.page.objectNumber` / `page.objectNumber`: a page ref's number, or a
/// ref's page's.
pub fn page_number(value: &Value) -> Option<f64> {
    let direct = value.get("objectNumber").and_then(number);
    match value.get("kind").and_then(Value::as_str) {
        Some("objectNumber") if value.get("page").is_none() => direct,
        _ => value.get("page").and_then(|p| p.get("objectNumber")).and_then(number).or(direct),
    }
}

/// A page ref's object number (`page.objectNumber`).
pub fn page_object_number(page: &Value) -> f64 {
    page.get("objectNumber").and_then(number).unwrap_or(f64::NAN)
}

/// `annotation.page.objectNumber`.
pub fn annotation_page(annotation: &Object) -> f64 {
    annotation.get("page").map(page_object_number).unwrap_or(f64::NAN)
}

/// `Object.keys(object)` of a JSON value; none for anything else.
pub fn keys(value: &Value) -> Vec<String> {
    value.as_object().map(|o| o.keys().cloned().collect()).unwrap_or_default()
}

/// A member that is present and not `undefined`.
pub fn defined<'a>(object: &'a Object, key: &str) -> Option<&'a Value> {
    object.get(key).filter(|v| !epdf_kernel::is_undefined(v))
}

/// `{ ...base, ...over }`.
pub fn spread(base: &Object, over: &Object) -> Object {
    let mut out = base.clone();
    for (k, v) in over {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// An object from members, in order.
pub fn object<const N: usize>(members: [(&str, Value); N]) -> Object {
    let mut out = Map::new();
    for (k, v) in members {
        out.insert(k.to_string(), v);
    }
    out
}

/// `JSON.stringify(value)` as the comparisons in the TypeScript use it.
pub fn stringify(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(v) if epdf_kernel::is_undefined(v) => "undefined".into(),
        Some(v) => epdf_annotation_core::json::stringify(v),
    }
}

/// `typeof value === 'number'` and finite: a JSON number.
pub fn finite(value: Option<&Value>) -> Option<f64> {
    value.and_then(number).filter(|x| x.is_finite())
}

/// A JSON number from an `f64`, `null` for none.
pub fn opt_num(value: Option<f64>) -> Value {
    value.map_or(Value::Null, num)
}
