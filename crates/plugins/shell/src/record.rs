//! A plain object used as a record, which `ShellState.surfaces` is at
//! runtime: `openSurfaces()` lists its own keys in property order. The same
//! type as `epdf-plugin-i18n`'s `JsRecord`; the two plugins do not depend on
//! each other, so each carries it.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// A canonical array index (`"0"`, `"17"`, below 2^32 - 1): the keys a
/// JavaScript object enumerates first, in ascending numeric order.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    if !key.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|n| *n != u32::MAX)
}

/// A string-keyed record as a JavaScript object keeps one: own keys
/// enumerate integer-like keys first (ascending), then the rest in
/// insertion order, and assigning an existing key keeps its position.
#[derive(Clone, Debug, PartialEq)]
pub struct JsRecord<V> {
    entries: Vec<(String, V)>,
}

impl<V> Default for JsRecord<V> {
    fn default() -> Self {
        JsRecord { entries: Vec::new() }
    }
}

impl<V> JsRecord<V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The own member `key`.
    pub fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `record[key] = value`.
    pub fn insert(&mut self, key: impl Into<String>, value: V) {
        let key = key.into();
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key, value)),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `Object.entries(record)`, in property order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        let mut indexed: Vec<(u32, usize)> = Vec::new();
        let mut named: Vec<usize> = Vec::new();
        for (position, (key, _)) in self.entries.iter().enumerate() {
            match array_index(key) {
                Some(index) => indexed.push((index, position)),
                None => named.push(position),
            }
        }
        indexed.sort_unstable();
        indexed.into_iter().map(|(_, position)| position).chain(named).map(move |position| {
            let (key, value) = &self.entries[position];
            (key.as_str(), value)
        })
    }
}

impl<V> FromIterator<(String, V)> for JsRecord<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(iter: I) -> Self {
        let mut record = JsRecord::new();
        for (key, value) in iter {
            record.insert(key, value);
        }
        record
    }
}

impl<V: Serialize> Serialize for JsRecord<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (key, value) in self.iter() {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<'de, V: Deserialize<'de>> Deserialize<'de> for JsRecord<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RecordVisitor<V>(std::marker::PhantomData<V>);
        impl<'de, V: Deserialize<'de>> Visitor<'de> for RecordVisitor<V> {
            type Value = JsRecord<V>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut access: M) -> Result<JsRecord<V>, M::Error> {
                let mut record = JsRecord::new();
                while let Some((key, value)) = access.next_entry::<String, V>()? {
                    record.insert(key, value);
                }
                Ok(record)
            }
        }
        deserializer.deserialize_map(RecordVisitor(std::marker::PhantomData))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_keys_enumerate_first() {
        let record: JsRecord<u8> =
            [("b", 1), ("10", 2), ("2", 3), ("a", 4), ("01", 5), ("4294967295", 6)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        let keys: Vec<&str> = record.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["2", "10", "b", "a", "01", "4294967295"]);
    }
}
