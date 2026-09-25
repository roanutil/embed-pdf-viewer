//! The JavaScript semantics the TypeScript leans on without saying so: a
//! plain object used as a record (own keys in property order, and the
//! `Object.prototype` members every lookup can also find), and `String(n)`
//! for a number.

use std::fmt;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, Serializer};

/// The members every plain object inherits from `Object.prototype`. A
/// record lookup (`locales[code]`) finds these when the record has no own
/// key of that name, and every one of them is truthy.
pub const OBJECT_PROTOTYPE_MEMBERS: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// What `record[key]` finds on `Object.prototype` when the record has no
/// own `key`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inherited {
    /// A method; `name` is its `Function.prototype.name`.
    Function { name: &'static str },
    /// `__proto__`: `Object.prototype` itself.
    ObjectPrototype,
}

impl Inherited {
    pub fn of(key: &str) -> Option<Inherited> {
        let member = OBJECT_PROTOTYPE_MEMBERS.iter().find(|m| **m == key)?;
        Some(match *member {
            "__proto__" => Inherited::ObjectPrototype,
            "constructor" => Inherited::Function { name: "Object" },
            name => Inherited::Function { name },
        })
    }

    /// `String(value)`.
    pub fn to_js_string(self) -> String {
        match self {
            Inherited::Function { name } => format!("function {name}() {{ [native code] }}"),
            Inherited::ObjectPrototype => "[object Object]".to_string(),
        }
    }
}

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

/// A plain object used as a string-keyed record, as the TypeScript's
/// `Record<string, T>` is at runtime: own keys enumerate integer-like keys
/// first (ascending), then the rest in insertion order, and assigning an
/// existing key keeps its position.
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

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
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

    /// `Object.keys(record)`.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.iter().map(|(k, _)| k)
    }

    /// `Object.values(record)`.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, v)| v)
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

/// `String(n)`: ECMAScript `Number::toString(10)`, shortest round-trip
/// digits, with the exponent thresholds of 21 and -7.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let (digits, exponent) = shortest_digits(n.abs());
    // n = 0.d1d2...dk × 10^point, as ECMAScript spells it: k digits, point = e + 1.
    let k = digits.len() as i32;
    let point = exponent + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let e = point - 1;
        let sign_e = if e < 0 { '-' } else { '+' };
        let mantissa =
            if k == 1 { digits.clone() } else { format!("{}.{}", &digits[..1], &digits[1..]) };
        format!("{mantissa}e{sign_e}{}", e.abs())
    };
    format!("{sign}{body}")
}

/// The shortest decimal digits that round-trip `x` (positive, finite), and
/// the exponent of the first: `x = d.ddd × 10^exponent`.
pub fn shortest_digits(x: f64) -> (String, i32) {
    // Rust's `{:e}` is the shortest round-trip representation.
    let text = format!("{x:e}");
    let (mantissa, exponent) = text.split_once('e').expect("{:e} has an exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exponent.parse().expect("{:e} exponent is an integer"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_as_javascript_does() {
        let cases = [
            (0.0, "0"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (1e21, "1e+21"),
            (1e-7, "1e-7"),
            (123456789012345680000.0, "123456789012345680000"),
            (0.000001, "0.000001"),
            (-2.5e-8, "-2.5e-8"),
            (0.1 + 0.2, "0.30000000000000004"),
            (9007199254740992.0, "9007199254740992"),
            (f64::MAX, "1.7976931348623157e+308"),
            (100.0, "100"),
            (5e-324, "5e-324"),
        ];
        for (n, want) in cases {
            assert_eq!(number_to_string(n), want, "{n}");
        }
    }

    #[test]
    fn records_enumerate_integer_keys_first() {
        let record: JsRecord<u8> =
            [("b", 1), ("10", 2), ("2", 3), ("a", 4), ("01", 5), ("4294967295", 6)]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect();
        assert_eq!(record.keys().collect::<Vec<_>>(), ["2", "10", "b", "a", "01", "4294967295"]);
    }
}
