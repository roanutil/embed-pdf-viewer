//! A JavaScript value, for data that comes from people: the layout
//! (`layout.ts`) is plain data, read the way the TypeScript reads it, so a
//! member of the wrong type is skipped and named instead of refused.
//!
//! Objects and arrays sit behind an [`Rc`], so an edit that copies an object
//! with a spread keeps every member it didn't touch as the same allocation,
//! the way the TypeScript keeps the same reference. [`JsValue::same`] is
//! `===`: it compares objects and arrays by allocation.
//!
//! Members are own members only. `bars['constructor']` reads nothing here,
//! where the TypeScript finds `Object.prototype`'s function (README, "Not
//! ported").

use std::fmt;
use std::rc::Rc;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::js::Record;

#[derive(Clone, Debug, PartialEq)]
pub enum JsValue {
    Null,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    Array(Rc<Vec<JsValue>>),
    Object(Rc<Record<JsValue>>),
}

impl JsValue {
    pub fn object(record: Record<JsValue>) -> JsValue {
        JsValue::Object(Rc::new(record))
    }

    pub fn array(items: Vec<JsValue>) -> JsValue {
        JsValue::Array(Rc::new(items))
    }

    /// `{}`.
    pub fn empty_object() -> JsValue {
        JsValue::object(Record::new())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsValue]> {
        match self {
            JsValue::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Record<JsValue>> {
        match self {
            JsValue::Object(record) => Some(record),
            _ => None,
        }
    }

    /// `typeof value === 'object' && value !== null`: an object or an array.
    pub fn is_object(&self) -> bool {
        matches!(self, JsValue::Object(_) | JsValue::Array(_))
    }

    /// `typeof value`.
    pub fn type_of(&self) -> &'static str {
        match self {
            JsValue::Null | JsValue::Array(_) | JsValue::Object(_) => "object",
            JsValue::Bool(_) => "boolean",
            JsValue::Number(_) => "number",
            JsValue::String(_) => "string",
        }
    }

    /// `Boolean(value)`.
    pub fn truthy(&self) -> bool {
        match self {
            JsValue::Null => false,
            JsValue::Bool(b) => *b,
            JsValue::Number(n) => *n != 0.0 && !n.is_nan(),
            JsValue::String(s) => !s.is_empty(),
            JsValue::Array(_) | JsValue::Object(_) => true,
        }
    }

    /// `value[key]`, an own member: `None` is `undefined`. An array answers
    /// its indices and `length`, a string its UTF-16 units and `length`.
    pub fn get(&self, key: &str) -> Option<JsValue> {
        match self {
            JsValue::Object(record) => record.get(key).cloned(),
            JsValue::Array(items) if key == "length" => Some(JsValue::Number(items.len() as f64)),
            JsValue::Array(items) => {
                crate::js::array_index(key).and_then(|i| items.get(i as usize)).cloned()
            }
            JsValue::String(s) if key == "length" => {
                Some(JsValue::Number(crate::js::utf16_len(s) as f64))
            }
            JsValue::String(s) => crate::js::array_index(key)
                .and_then(|i| utf16_units(s).into_iter().nth(i as usize))
                .map(JsValue::from),
            _ => None,
        }
    }

    /// `key in value`, for an object or an array.
    pub fn has(&self, key: &str) -> bool {
        match self {
            JsValue::Object(record) => record.contains_key(key),
            JsValue::Array(items) => {
                key == "length"
                    || crate::js::array_index(key).is_some_and(|i| (i as usize) < items.len())
            }
            _ => false,
        }
    }

    /// `Object.entries(value)`: an object's members in key order, an array's
    /// or a string's indices. Anything else has none.
    pub fn entries(&self) -> Vec<(String, JsValue)> {
        match self {
            JsValue::Object(record) => {
                record.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
            }
            JsValue::Array(items) => {
                items.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect()
            }
            JsValue::String(s) => utf16_units(s)
                .into_iter()
                .enumerate()
                .map(|(i, unit)| (i.to_string(), JsValue::from(unit)))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// `Object.keys(value)`.
    pub fn keys(&self) -> Vec<String> {
        self.entries().into_iter().map(|(k, _)| k).collect()
    }

    /// `left === right`: objects and arrays by allocation.
    pub fn same(left: &JsValue, right: &JsValue) -> bool {
        match (left, right) {
            (JsValue::Null, JsValue::Null) => true,
            (JsValue::Bool(a), JsValue::Bool(b)) => a == b,
            (JsValue::Number(a), JsValue::Number(b)) => a == b,
            (JsValue::String(a), JsValue::String(b)) => a == b,
            (JsValue::Array(a), JsValue::Array(b)) => Rc::ptr_eq(a, b),
            (JsValue::Object(a), JsValue::Object(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// SameValueZero, what `includes` and `Set` use: `===`, except that
    /// `NaN` is itself.
    pub fn same_value_zero(left: &JsValue, right: &JsValue) -> bool {
        match (left, right) {
            (JsValue::Number(a), JsValue::Number(b)) if a.is_nan() && b.is_nan() => true,
            _ => JsValue::same(left, right),
        }
    }

    /// Canonical JSON (`canonical.ts`) to a value: `{"$number": ..}` is a
    /// number, and a member that is `{"$undefined": true}` is left out, as
    /// JSON leaves it out. A top-level `undefined` reads as `null`.
    pub fn from_canonical(value: &Value) -> JsValue {
        match value {
            Value::Null => JsValue::Null,
            Value::Bool(b) => JsValue::Bool(*b),
            Value::Number(n) => JsValue::Number(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => JsValue::from(s.as_str()),
            Value::Array(items) => {
                JsValue::array(items.iter().map(JsValue::from_canonical).collect())
            }
            Value::Object(map) => {
                if let Some(n) = crate::js::number_from_value(value).filter(|_| map.len() == 1) {
                    return JsValue::Number(n);
                }
                if epdf_kernel::is_undefined(value) {
                    return JsValue::Null;
                }
                JsValue::object(
                    map.iter()
                        .filter(|(_, v)| !epdf_kernel::is_undefined(v))
                        .map(|(k, v)| (k.clone(), JsValue::from_canonical(v)))
                        .collect(),
                )
            }
        }
    }

    /// The value as canonical JSON: a non-finite number is `{"$number": ..}`.
    pub fn to_canonical(&self) -> Value {
        match self {
            JsValue::Null => Value::Null,
            JsValue::Bool(b) => Value::Bool(*b),
            JsValue::Number(n) => crate::js::number_to_value(*n),
            JsValue::String(s) => Value::String(s.to_string()),
            JsValue::Array(items) => {
                Value::Array(items.iter().map(JsValue::to_canonical).collect())
            }
            JsValue::Object(record) => Value::Object(
                record.iter().map(|(k, v)| (k.to_string(), v.to_canonical())).collect(),
            ),
        }
    }
}

/// A string's UTF-16 units, each as a string. Half of a surrogate pair can't
/// be a Rust `String`, so each half reads as U+FFFD.
fn utf16_units(s: &str) -> Vec<String> {
    let mut units = Vec::new();
    for c in s.chars() {
        if c.len_utf16() == 1 {
            units.push(c.to_string());
        } else {
            units.push('\u{FFFD}'.to_string());
            units.push('\u{FFFD}'.to_string());
        }
    }
    units
}

/// `{ ...value }`: an object's own members, an array's or a string's
/// indices; nothing for `undefined`, `null`, a boolean or a number.
pub fn spread(value: Option<&JsValue>) -> Record<JsValue> {
    match value {
        Some(JsValue::Object(record)) => (**record).clone(),
        Some(other) => other.entries().into_iter().collect(),
        None => Record::new(),
    }
}

/// `String(value)`, as a template literal prints it.
pub fn js_string(value: &JsValue) -> String {
    match value {
        JsValue::Null => "null".into(),
        JsValue::Bool(b) => b.to_string(),
        JsValue::Number(n) => epdf_kernel::js::number_to_string(*n),
        JsValue::String(s) => s.to_string(),
        // `Array.prototype.join` prints `null` as nothing.
        JsValue::Array(items) => items
            .iter()
            .map(|v| if matches!(v, JsValue::Null) { String::new() } else { js_string(v) })
            .collect::<Vec<_>>()
            .join(","),
        JsValue::Object(_) => "[object Object]".into(),
    }
}

/// `JSON.stringify(value)`: compact, members in key order, a non-finite
/// number as `null`.
pub fn json_stringify(value: &JsValue) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &JsValue, out: &mut String) {
    match value {
        JsValue::Null => out.push_str("null"),
        JsValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsValue::Number(n) if !n.is_finite() => out.push_str("null"),
        JsValue::Number(n) => out.push_str(&epdf_kernel::js::number_to_string(*n)),
        JsValue::String(s) => out.push_str(&quote(s)),
        JsValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        JsValue::Object(record) => {
            out.push('{');
            for (i, (k, v)) in record.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&quote(k));
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

/// A JSON string literal: `serde_json` escapes the same characters
/// `JSON.stringify` does (`"`, `\` and the C0 controls, the short forms
/// where there is one).
fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("a string serializes")
}

impl From<&str> for JsValue {
    fn from(s: &str) -> Self {
        JsValue::String(Rc::from(s))
    }
}

impl From<String> for JsValue {
    fn from(s: String) -> Self {
        JsValue::String(Rc::from(s))
    }
}

impl From<f64> for JsValue {
    fn from(n: f64) -> Self {
        JsValue::Number(n)
    }
}

impl From<bool> for JsValue {
    fn from(b: bool) -> Self {
        JsValue::Bool(b)
    }
}

impl From<Vec<JsValue>> for JsValue {
    fn from(items: Vec<JsValue>) -> Self {
        JsValue::array(items)
    }
}

impl From<Record<JsValue>> for JsValue {
    fn from(record: Record<JsValue>) -> Self {
        JsValue::object(record)
    }
}

/// Plain JSON, read as canonical JSON ([`JsValue::from_canonical`]).
impl From<Value> for JsValue {
    fn from(value: Value) -> Self {
        JsValue::from_canonical(&value)
    }
}

impl From<&Value> for JsValue {
    fn from(value: &Value) -> Self {
        JsValue::from_canonical(value)
    }
}

impl Serialize for JsValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            JsValue::Null => serializer.serialize_unit(),
            JsValue::Bool(b) => serializer.serialize_bool(*b),
            // An integral number prints as an integer, as `JSON.stringify`
            // prints it; `-0` prints as `0`.
            JsValue::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
                serializer.serialize_i64(*n as i64)
            }
            JsValue::Number(n) => serializer.serialize_f64(*n),
            JsValue::String(s) => serializer.serialize_str(s),
            JsValue::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items.iter() {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            JsValue::Object(record) => {
                let mut map = serializer.serialize_map(Some(record.len()))?;
                for (k, v) in record.iter() {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for JsValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsVisitor;
        impl<'de> Visitor<'de> for JsVisitor {
            type Value = JsValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON value")
            }
            fn visit_unit<E>(self) -> Result<JsValue, E> {
                Ok(JsValue::Null)
            }
            fn visit_none<E>(self) -> Result<JsValue, E> {
                Ok(JsValue::Null)
            }
            fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<JsValue, D::Error> {
                JsValue::deserialize(d)
            }
            fn visit_bool<E>(self, b: bool) -> Result<JsValue, E> {
                Ok(JsValue::Bool(b))
            }
            fn visit_i64<E>(self, n: i64) -> Result<JsValue, E> {
                Ok(JsValue::Number(n as f64))
            }
            fn visit_u64<E>(self, n: u64) -> Result<JsValue, E> {
                Ok(JsValue::Number(n as f64))
            }
            fn visit_f64<E>(self, n: f64) -> Result<JsValue, E> {
                Ok(JsValue::Number(n))
            }
            fn visit_str<E>(self, s: &str) -> Result<JsValue, E> {
                Ok(JsValue::from(s))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<JsValue, A::Error> {
                let mut items = Vec::new();
                while let Some(item) = access.next_element()? {
                    items.push(item);
                }
                Ok(JsValue::array(items))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<JsValue, A::Error> {
                let mut record = Record::new();
                while let Some((k, v)) = access.next_entry::<String, JsValue>()? {
                    record.insert(k, v);
                }
                Ok(JsValue::object(record))
            }
        }
        deserializer.deserialize_any(JsVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_and_prints_canonical_json() {
        let v = JsValue::from(json!({"b": 1, "2": {"$number": "NaN"}, "u": {"$undefined": true}}));
        assert_eq!(v.keys(), ["2", "b"]);
        assert!(v.get("2").and_then(|n| n.as_f64()).is_some_and(f64::is_nan));
        assert_eq!(v.to_canonical(), json!({"2": {"$number": "NaN"}, "b": 1.0}));
        assert_eq!(
            serde_json::to_string(&JsValue::from(json!([1.5, -0.0, 3]))).unwrap(),
            "[1.5,0,3]"
        );
        let back: JsValue = serde_json::from_str(r#"{"x":[true,null,"s"]}"#).unwrap();
        assert_eq!(back, JsValue::from(json!({"x": [true, null, "s"]})));
    }

    #[test]
    fn stringifies_as_javascript_does() {
        let v =
            JsValue::from(json!({"nope": true, "n": 1e21, "f": 0.5, "s": "a\"\n", "a": [1, null]}));
        assert_eq!(
            json_stringify(&v),
            r#"{"nope":true,"n":1e+21,"f":0.5,"s":"a\"\n","a":[1,null]}"#
        );
        assert_eq!(json_stringify(&JsValue::Number(f64::NAN)), "null");
        assert_eq!(js_string(&JsValue::from(json!([1, null, "x"]))), "1,,x");
        assert_eq!(js_string(&JsValue::from(json!({}))), "[object Object]");
        assert_eq!(js_string(&JsValue::Number(-1.0)), "-1");
    }

    #[test]
    fn spreads_and_reads_members_as_javascript_does() {
        assert_eq!(spread(Some(&JsValue::from("ab"))).keys().collect::<Vec<_>>(), ["0", "1"]);
        assert_eq!(spread(Some(&JsValue::from(json!(["x"])))).get("0"), Some(&JsValue::from("x")));
        assert!(spread(Some(&JsValue::Number(5.0))).is_empty());
        // Half a surrogate pair reads as U+FFFD (README, "Not ported").
        let halves = spread(Some(&JsValue::from("😀")));
        assert_eq!(
            halves.values().collect::<Vec<_>>(),
            [&JsValue::from("\u{FFFD}"), &JsValue::from("\u{FFFD}")]
        );
        assert!(spread(None).is_empty());
        let arr = JsValue::from(json!(["a"]));
        assert_eq!(arr.get("length"), Some(JsValue::Number(1.0)));
        assert!(arr.has("0") && !arr.has("1"));
        assert_eq!(JsValue::from("😀a").get("length"), Some(JsValue::Number(3.0)));
        assert_eq!(JsValue::from("😀a").get("2"), Some(JsValue::from("a")));
        // Own members only: no `Object.prototype`.
        assert_eq!(JsValue::empty_object().get("constructor"), None);
    }

    #[test]
    fn compares_as_javascript_does() {
        let a = JsValue::from(json!({"x": 1}));
        assert!(JsValue::same(&a, &a.clone()));
        assert!(!JsValue::same(&a, &JsValue::from(json!({"x": 1}))));
        assert!(!JsValue::same(&JsValue::Number(f64::NAN), &JsValue::Number(f64::NAN)));
        assert!(JsValue::same_value_zero(&JsValue::Number(f64::NAN), &JsValue::Number(f64::NAN)));
        assert!(JsValue::same(&JsValue::from("s"), &JsValue::from("s")));
        assert!(!JsValue::Number(0.0).truthy() && JsValue::from(json!([])).truthy());
        assert_eq!(JsValue::Null.type_of(), "object");
    }
}
