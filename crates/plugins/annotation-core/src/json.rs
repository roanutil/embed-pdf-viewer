//! The engine's annotation, draft and patch objects as JSON values.
//!
//! The TypeScript keeps an annotation as a plain object and edits it with
//! spreads, so its members keep the order its builder wrote them in and a
//! new member lands at the end. A `serde_json::Map` (with `preserve_order`)
//! does the same: `insert` on an existing key keeps its place. That is why
//! the engine's records are maps here and not structs.
//!
//! A JSON number cannot be `NaN` or infinite. Canonical JSON spells those
//! `{"$number": "NaN" | "Infinity" | "-Infinity"}` (`canonical.ts`), and so
//! does every value this crate builds: [`num`] writes that spelling and
//! [`number`] reads it, so a NaN that reaches a record survives a round
//! trip, as it does in the TypeScript.

use epdf_geometry::{Point, Quad, Rect};
use serde_json::{Map, Value};

/// A JSON object, members in insertion order.
pub type Object = Map<String, Value>;

/// A number as a value: finite numbers as JSON numbers, the rest in their
/// canonical spelling.
pub fn num(x: f64) -> Value {
    if x.is_nan() {
        special("NaN")
    } else if x == f64::INFINITY {
        special("Infinity")
    } else if x == f64::NEG_INFINITY {
        special("-Infinity")
    } else {
        serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}

fn special(name: &str) -> Value {
    let mut map = Map::new();
    map.insert("$number".into(), Value::String(name.into()));
    Value::Object(map)
}

/// A value read as a number (`typeof value === 'number'`), the canonical
/// non-finite spellings included.
pub fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::Object(map) if map.len() == 1 => match map.get("$number").and_then(Value::as_str) {
            Some("NaN") => Some(f64::NAN),
            Some("Infinity") => Some(f64::INFINITY),
            Some("-Infinity") => Some(f64::NEG_INFINITY),
            _ => None,
        },
        _ => None,
    }
}

/// `value == null`: absent or `null`. The canonical `undefined` reads as absent.
pub fn nullish(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Object(map)) => {
            map.len() == 1 && map.get("$undefined") == Some(&Value::Bool(true))
        }
        _ => false,
    }
}

/// `value === undefined`: absent, or the canonical `undefined`.
pub fn undefined(value: Option<&Value>) -> bool {
    match value {
        None => true,
        Some(Value::Object(map)) => {
            map.len() == 1 && map.get("$undefined") == Some(&Value::Bool(true))
        }
        _ => false,
    }
}

/// A present, non-null member of `object`.
pub fn get<'a>(object: &'a Object, name: &str) -> Option<&'a Value> {
    object.get(name).filter(|v| !nullish(Some(v)))
}

pub fn get_str<'a>(object: &'a Object, name: &str) -> Option<&'a str> {
    object.get(name).and_then(Value::as_str)
}

pub fn get_num(object: &Object, name: &str) -> Option<f64> {
    object.get(name).and_then(number)
}

pub fn get_bool(object: &Object, name: &str) -> Option<bool> {
    object.get(name).and_then(Value::as_bool)
}

/// JavaScript truthiness of a member: `!!object[name]`.
pub fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(v @ Value::Number(_)) => number(v).is_some_and(|x| x != 0.0 && !x.is_nan()),
        Some(v @ Value::Object(_)) => match number(v) {
            Some(x) => !x.is_nan(),
            None => !undefined(Some(v)),
        },
        Some(Value::Array(_)) => true,
    }
}

pub fn point(value: &Value) -> Option<Point> {
    let object = value.as_object()?;
    Some(Point { x: get_num(object, "x")?, y: get_num(object, "y")? })
}

/// A point member read loosely: a missing coordinate reads as NaN, as
/// `undefined` does in arithmetic.
pub fn point_loose(value: &Value) -> Point {
    let object = value.as_object();
    let at = |name| object.and_then(|o| get_num(o, name)).unwrap_or(f64::NAN);
    Point { x: at("x"), y: at("y") }
}

pub fn point_value(p: Point) -> Value {
    let mut map = Map::new();
    map.insert("x".into(), num(p.x));
    map.insert("y".into(), num(p.y));
    Value::Object(map)
}

pub fn points(value: &Value) -> Option<Vec<Point>> {
    value.as_array()?.iter().map(point).collect()
}

pub fn points_value(points: &[Point]) -> Value {
    Value::Array(points.iter().map(|p| point_value(*p)).collect())
}

pub fn rect(value: &Value) -> Option<Rect> {
    let object = value.as_object()?;
    Some(Rect {
        x: get_num(object, "x")?,
        y: get_num(object, "y")?,
        width: get_num(object, "width")?,
        height: get_num(object, "height")?,
    })
}

/// A box member read loosely: a missing side reads as NaN.
pub fn rect_loose(value: &Value) -> Rect {
    let object = value.as_object();
    let at = |name| object.and_then(|o| get_num(o, name)).unwrap_or(f64::NAN);
    Rect { x: at("x"), y: at("y"), width: at("width"), height: at("height") }
}

pub fn rect_value(r: Rect) -> Value {
    let mut map = Map::new();
    map.insert("x".into(), num(r.x));
    map.insert("y".into(), num(r.y));
    map.insert("width".into(), num(r.width));
    map.insert("height".into(), num(r.height));
    Value::Object(map)
}

pub fn quad(value: &Value) -> Option<Quad> {
    let object = value.as_object()?;
    Some(Quad {
        upper_left: point(object.get("upperLeft")?)?,
        upper_right: point(object.get("upperRight")?)?,
        lower_left: point(object.get("lowerLeft")?)?,
        lower_right: point(object.get("lowerRight")?)?,
    })
}

pub fn quad_value(q: Quad) -> Value {
    let mut map = Map::new();
    map.insert("upperLeft".into(), point_value(q.upper_left));
    map.insert("upperRight".into(), point_value(q.upper_right));
    map.insert("lowerLeft".into(), point_value(q.lower_left));
    map.insert("lowerRight".into(), point_value(q.lower_right));
    Value::Object(map)
}

pub fn quads(value: &Value) -> Option<Vec<Quad>> {
    value.as_array()?.iter().map(quad).collect()
}

/// Build an object from `(name, value)` pairs, in order.
pub fn object<const N: usize>(members: [(&str, Value); N]) -> Object {
    let mut map = Map::new();
    for (name, value) in members {
        map.insert(name.into(), value);
    }
    map
}

/// `{ ...base, ...over }`: members of `over` replace those of `base` in
/// place, and new ones go at the end.
pub fn spread(base: &Object, over: &Object) -> Object {
    let mut out = base.clone();
    for (k, v) in over {
        out.insert(k.clone(), v.clone());
    }
    out
}

/// `{ ...object }` without the member `name` (a rest destructure).
pub fn without(object: &Object, name: &str) -> Object {
    let mut out = object.clone();
    out.shift_remove(name);
    out
}

/// `Object.keys(value).length`, ignoring canonical `undefined` members as
/// JSON would.
pub fn defined_len(object: &Object) -> usize {
    object.values().filter(|v| !undefined(Some(v))).count()
}

/// A JavaScript `typeof value === 'object' && value !== null` test on a
/// value that is not a canonical number or `undefined`.
pub fn is_plain_object(value: &Value) -> bool {
    matches!(value, Value::Object(_)) && number(value).is_none() && !undefined(Some(value))
}

/// Every number in `value`, nested, NaN or not: `hasNaN` walks this.
pub fn has_nan(value: &Value) -> bool {
    if let Some(x) = number(value) {
        return x.is_nan();
    }
    match value {
        Value::Array(items) => items.iter().any(has_nan),
        Value::Object(map) => map.values().any(has_nan),
        _ => false,
    }
}

/// `Object.is(a, b)` for JSON leaves, and exact deep equality for arrays and
/// objects with member order ignored (`patchBetween.ts`'s `sameValue`):
/// `undefined` members are skipped, numbers compare with `Object.is` (`NaN`
/// is itself, `0` is not `-0`; JSON keeps no `-0`, so that half is moot).
pub fn same_value(left: Option<&Value>, right: Option<&Value>) -> bool {
    let (l, r) = match (left, right) {
        (None, None) => return true,
        (Some(l), Some(r)) => (l, r),
        (Some(v), None) | (None, Some(v)) => return undefined(Some(v)),
    };
    if undefined(Some(l)) || undefined(Some(r)) {
        return undefined(Some(l)) && undefined(Some(r));
    }
    if let (Some(a), Some(b)) = (number(l), number(r)) {
        return a == b || (a.is_nan() && b.is_nan());
    }
    if number(l).is_some() || number(r).is_some() {
        return false;
    }
    match (l, r) {
        (Value::Array(a), Value::Array(b)) => {
            let a: Vec<&Value> = a.iter().collect();
            let b: Vec<&Value> = b.iter().collect();
            // Arrays compare by their index keys, as objects do.
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| same_value(Some(x), Some(y)))
        }
        (Value::Object(a), Value::Object(b)) => {
            let names: Vec<&String> =
                a.iter().filter(|(_, v)| !undefined(Some(v))).map(|(k, _)| k).collect();
            let others = b.values().filter(|v| !undefined(Some(v))).count();
            names.len() == others && names.iter().all(|k| same_value(a.get(*k), b.get(*k)))
        }
        (Value::Array(_), Value::Object(_)) | (Value::Object(_), Value::Array(_)) => false,
        _ => l == r,
    }
}

/// `JSON.stringify(value)`: members in order, non-finite numbers as `null`.
pub fn stringify(value: &Value) -> String {
    fn plain(value: &Value) -> Value {
        if number(value).is_some_and(|x| !x.is_finite()) {
            return Value::Null;
        }
        match value {
            Value::Array(items) => Value::Array(items.iter().map(plain).collect()),
            Value::Object(map) => Value::Object(
                map.iter()
                    .filter(|(_, v)| !undefined(Some(v)))
                    .map(|(k, v)| (k.clone(), plain(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    serde_json::to_string(&plain(value)).unwrap_or_default()
}
