//! The JavaScript semantics the TypeScript leans on without saying so: object
//! key order, `Map` insertion order, numbers that may be `NaN` or infinite,
//! `String.prototype.trim`, UTF-16 lengths, and `Array.prototype.splice`'s
//! start index.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// A string-keyed plain object (`Record<string, V>`), iterated as
/// `Object.entries` iterates one: integer-like keys first in ascending
/// numeric order, then the rest in insertion order. Assigning an existing key
/// keeps its position.
#[derive(Clone, Debug, PartialEq)]
pub struct Record<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for Record<V> {
    fn default() -> Self {
        Record { entries: Vec::new() }
    }
}

/// The key as an array index (a canonical numeric string below 2^32 - 1),
/// which JavaScript orders before every other key.
pub fn array_index(key: &str) -> Option<u32> {
    if key == "0" {
        return Some(0);
    }
    if key.is_empty() || key.starts_with('0') || !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u64>().ok().filter(|&n| n < u64::from(u32::MAX)).map(|n| n as u32)
}

impl<V> Record<V> {
    pub fn new() -> Self {
        Record::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `object[key] = value`.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
            return;
        }
        match array_index(&key) {
            Some(index) => {
                let at = self
                    .entries
                    .iter()
                    .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                    .unwrap_or(self.entries.len());
                self.entries.insert(at, (key, value));
            }
            None => self.entries.push((key, value)),
        }
    }

    /// `Object.entries(object)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }

    /// `Object.fromEntries(Object.entries(object).map(([k, v]) => [k, f(v)]))`.
    pub fn map_values<W>(&self, mut f: impl FnMut(&V) -> W) -> Record<W> {
        Record { entries: self.entries.iter().map(|(k, v)| (k.clone(), f(v))).collect() }
    }
}

/// `Object.fromEntries`: later duplicates overwrite in place.
impl<K: Into<String>, V> FromIterator<(K, V)> for Record<V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut record = Record::new();
        for (k, v) in iter {
            record.insert(k, v);
        }
        record
    }
}

impl<V: Serialize> Serialize for Record<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (k, v) in &self.entries {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for Record<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor<V>(PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for RecordVisitor<V> {
            type Value = Record<V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Record<V>, A::Error> {
                let mut record = Record::new();
                while let Some((k, v)) = access.next_entry::<String, V>()? {
                    record.insert(k, v);
                }
                Ok(record)
            }
        }
        deserializer.deserialize_map(RecordVisitor(PhantomData))
    }
}

/// A string-keyed `Map`: insertion order, and `set` on an existing key
/// replaces the value where it stands. Serialized in the canonical form,
/// `{"$map": [[key, value], ...]}`.
#[derive(Clone, Debug, PartialEq)]
pub struct JsMap<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for JsMap<V> {
    fn default() -> Self {
        JsMap { entries: Vec::new() }
    }
}

impl<V> JsMap<V> {
    pub fn new() -> Self {
        JsMap::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl<V: Serialize> Serialize for JsMap<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Entries<'a, V>(&'a [(String, V)]);
        impl<V: Serialize> Serialize for Entries<'_, V> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
                for entry in self.0 {
                    seq.serialize_element(&(&entry.0, &entry.1))?;
                }
                seq.end()
            }
        }
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$map", &Entries(&self.entries))?;
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for JsMap<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire<V> {
            #[serde(rename = "$map")]
            map: Vec<(String, V)>,
        }
        let wire = Wire::<V>::deserialize(deserializer)?;
        let mut out = JsMap::new();
        for (k, v) in wire.map {
            out.set(k, v);
        }
        Ok(out)
    }
}

/// A number in canonical JSON: a plain number, or `{"$number": "NaN" |
/// "Infinity" | "-Infinity"}`.
pub fn number_from_value(value: &Value) -> Option<f64> {
    if let Some(n) = value.as_f64() {
        return Some(n);
    }
    match value.get("$number")?.as_str()? {
        "NaN" => Some(f64::NAN),
        "Infinity" => Some(f64::INFINITY),
        "-Infinity" => Some(f64::NEG_INFINITY),
        _ => None,
    }
}

/// A number in canonical JSON, the inverse of [`number_from_value`].
pub fn number_to_value(n: f64) -> Value {
    if n.is_nan() {
        serde_json::json!({ "$number": "NaN" })
    } else if n == f64::INFINITY {
        serde_json::json!({ "$number": "Infinity" })
    } else if n == f64::NEG_INFINITY {
        serde_json::json!({ "$number": "-Infinity" })
    } else {
        serde_json::json!(if n == 0.0 { 0.0 } else { n })
    }
}

/// `#[serde(with = "js::number")]` for an `f64` that may be `NaN` or infinite.
pub mod number {
    use super::*;

    pub fn serialize<S: Serializer>(n: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        number_to_value(*n).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let value = Value::deserialize(deserializer)?;
        number_from_value(&value).ok_or_else(|| serde::de::Error::custom("expected a number"))
    }
}

/// `#[serde(with = "js::opt_number")]` for an optional member.
pub mod opt_number {
    use super::*;

    pub fn serialize<S: Serializer>(n: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error> {
        match n {
            Some(n) => number_to_value(*n).serialize(serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<f64>, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.is_null() || epdf_kernel::is_undefined(&value) {
            return Ok(None);
        }
        number_from_value(&value)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("expected a number"))
    }
}

/// ECMAScript WhiteSpace and LineTerminator: what `trim` removes. Unlike
/// `char::is_whitespace` it includes U+FEFF and excludes U+0085.
pub fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// `String.prototype.trim`.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// `String.prototype.length`: UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `s.charAt(0).toUpperCase() + s.slice(1)`. `charAt(0)` is one UTF-16 code
/// unit, so a first character outside the BMP is half a surrogate pair,
/// which `toUpperCase` leaves alone: the string comes back unchanged.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) if first.len_utf16() == 1 => {
            let mut out: String = first.to_uppercase().collect();
            out.push_str(chars.as_str());
            out
        }
        _ => s.to_string(),
    }
}

/// `Array.prototype.splice`'s start: ToIntegerOrInfinity, then relative to
/// the end when negative, clamped to `0..=len`.
pub fn splice_start(at: f64, len: usize) -> usize {
    let relative = if at.is_nan() { 0.0 } else { at.trunc() };
    let len_f = len as f64;
    if relative < 0.0 { (len_f + relative).max(0.0) as usize } else { relative.min(len_f) as usize }
}
