//! The JavaScript semantics this port reproduces beyond `epdf_geometry::js`:
//! `Number.prototype.toFixed`, `Number(string)`, the three states of an
//! optional nullable member (absent, `null`, a value), `Object.entries`
//! order for a numeric-keyed record, a `Set` of ids, and the names every
//! plain object inherits from `Object.prototype`.

use std::fmt;
use std::marker::PhantomData;

use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use epdf_geometry::js::{hypot, max, max_of, min, min_of, number_to_string, round, sign};

// ── numbers ─────────────────────────────────────────────────────────────────

/// `x` as `m * 2^e` with an integer mantissa, for a positive finite `x`.
fn decompose(x: f64) -> (u64, i32) {
    let bits = x.to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if exponent == 0 { (fraction, -1074) } else { (fraction | (1u64 << 52), exponent - 1075) }
}

/// Whether `x * 10^digits` lies exactly halfway between two integers, for a
/// positive finite `x`: `2 * x * 10^digits` is then an odd integer.
fn exact_tie(x: f64, digits: u32) -> bool {
    let (m, e) = decompose(x);
    let t = u128::from(m) * 5u128.pow(digits);
    // 2 * x * 10^digits = t * 2^(e + 1 + digits)
    let shift = e + 1 + digits as i32;
    match shift {
        s if s > 0 => false,
        0 => t % 2 == 1,
        s => t != 0 && t.trailing_zeros() == (-s) as u32,
    }
}

/// `Number.prototype.toFixed(digits)`. The decimal is the exact value of the
/// double rounded half up (away from zero, on the magnitude); Rust's `{:.N}`
/// breaks an exact tie to even, so a tie is written out one digit longer
/// (exactly, ending in 5) and rounded up by hand. Magnitudes of `1e21` and more print as `Number#toString` does.
pub fn to_fixed(x: f64, digits: u32) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.abs() >= 1e21 {
        return number_to_string(x);
    }
    let negative = x < 0.0;
    let y = x.abs();
    let precision = digits as usize;
    let body = if y != 0.0 && exact_tie(y, digits) {
        round_up_tie(&format!("{:.*}", precision + 1, y))
    } else {
        format!("{:.*}", precision, y)
    };
    if negative { format!("-{body}") } else { body }
}

/// Drop the trailing `5` of an exact decimal and add one in the last place
/// kept: `0.03125` to `0.0313`, `2.5` to `3`, `9.95` to `10.0`.
fn round_up_tie(exact: &str) -> String {
    let mut digits: Vec<u8> = exact[..exact.len() - 1].bytes().collect();
    if digits.last() == Some(&b'.') {
        digits.pop();
    }
    let mut i = digits.len();
    loop {
        if i == 0 {
            digits.insert(0, b'1');
            break;
        }
        i -= 1;
        match digits[i] {
            b'.' => continue,
            b'9' => digits[i] = b'0',
            d => {
                digits[i] = d + 1;
                break;
            }
        }
    }
    String::from_utf8(digits).unwrap_or_default()
}

/// `Number(x.toFixed(digits))`: `x` rounded to `digits` decimals the way the
/// TypeScript's `Number(n.toFixed(d))` idiom does.
pub fn fixed(x: f64, digits: u32) -> f64 {
    string_to_number(&to_fixed(x, digits))
}

/// ECMAScript `WhiteSpace` and `LineTerminator`, which `Number(string)` trims.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

fn radix_digits(s: &str, radix: u32) -> f64 {
    if s.is_empty() || !s.chars().all(|c| c.is_digit(radix)) {
        return f64::NAN;
    }
    s.chars().fold(0.0, |acc, c| acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)))
}

/// Whether `s` is a `StrUnsignedDecimalLiteral` without `Infinity`:
/// `digits [. digits?] | . digits`, then an optional exponent.
fn is_decimal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let int = b.iter().take_while(|c| c.is_ascii_digit()).count();
    i += int;
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        i += frac;
    }
    if int == 0 && frac == 0 {
        return false;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if exp == 0 {
            return false;
        }
        i += exp;
    }
    i == b.len()
}

/// `Number(string)` (ECMAScript `StringToNumber`): whitespace-trimmed, empty
/// is `0`, `0x`/`0o`/`0b` integers, a signed decimal or `Infinity`, and NaN
/// for anything else. Rust's `parse::<f64>` also takes `inf`, `nan` and
/// `infinity`, which JavaScript does not.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_js_space);
    if t.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return radix_digits(rest, radix);
        }
    }
    let (negative, unsigned) = match t.as_bytes()[0] {
        b'+' => (false, &t[1..]),
        b'-' => (true, &t[1..]),
        _ => (false, t),
    };
    let magnitude = if unsigned == "Infinity" {
        f64::INFINITY
    } else if is_decimal(unsigned) {
        unsigned.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        return f64::NAN;
    };
    if negative { -magnitude } else { magnitude }
}

/// `Number(s.slice(1))`: the number after the first UTF-16 code unit. A
/// first character outside the BMP leaves half a surrogate pair behind,
/// which no number spelling contains.
pub fn number_after_first_unit(s: &str) -> f64 {
    let mut chars = s.chars();
    match chars.next() {
        None => string_to_number(""),
        Some(c) if c.len_utf16() == 2 => f64::NAN,
        Some(_) => string_to_number(chars.as_str()),
    }
}

/// `String.prototype.length`: UTF-16 code units.
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// The names every plain object inherits from `Object.prototype`, so a
/// lookup `table[key]` finds a function (or the prototype) instead of
/// `undefined`.
pub fn is_prototype_key(key: &str) -> bool {
    matches!(
        key,
        "constructor"
            | "__defineGetter__"
            | "__defineSetter__"
            | "hasOwnProperty"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toString"
            | "valueOf"
            | "__proto__"
            | "toLocaleString"
    )
}

// ── three-state members ─────────────────────────────────────────────────────

/// An optional member whose value may also be `null`: `interiorColor` in a
/// patch, `link`. Absent (`undefined`) leaves a value alone, `null` clears
/// it, a value sets it. Use with `#[serde(default, skip_serializing_if =
/// "Nullable::is_undefined")]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Nullable<T> {
    #[default]
    Undefined,
    Null,
    Value(T),
}

impl<T> Nullable<T> {
    pub fn is_undefined(&self) -> bool {
        matches!(self, Nullable::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Nullable::Null)
    }

    /// `value ?? null`: undefined and null both read as `None`.
    pub fn as_option(&self) -> Option<&T> {
        match self {
            Nullable::Value(v) => Some(v),
            _ => None,
        }
    }

    pub fn from_option(value: Option<T>) -> Self {
        value.map_or(Nullable::Null, Nullable::Value)
    }
}

impl<T: Serialize> Serialize for Nullable<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Nullable::Value(v) => serializer.serialize_some(v),
            // A member that is present and undefined is skipped by the
            // container; elsewhere it prints as null, the closest JSON has.
            _ => serializer.serialize_none(),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nullable<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(deserializer)?.map_or(Nullable::Null, Nullable::Value))
    }
}

// ── records and sets ────────────────────────────────────────────────────────

/// A string-keyed plain object iterated as `for...in` / `Object.keys` does:
/// integer-like keys first in ascending numeric order, then the rest in
/// insertion order. Assigning an existing key keeps its position.
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

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

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

/// A `Set` of ids (`ReadonlySet<Id>`), in insertion order. Canonical JSON
/// spells it `{"$set": [...]}`; a plain array reads too.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdSet(pub Vec<String>);

impl IdSet {
    pub fn has(&self, id: &str) -> bool {
        self.0.iter().any(|x| x == id)
    }
}

impl FromIterator<String> for IdSet {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        let mut out = Vec::new();
        for id in iter {
            if !out.contains(&id) {
                out.push(id);
            }
        }
        IdSet(out)
    }
}

impl Serialize for IdSet {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Items<'a>(&'a [String]);
        impl Serialize for Items<'_> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
                for id in self.0 {
                    seq.serialize_element(id)?;
                }
                seq.end()
            }
        }
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$set", &Items(&self.0))?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for IdSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SetVisitor;
        impl<'de> Visitor<'de> for SetVisitor {
            type Value = IdSet;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a set of ids")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<IdSet, A::Error> {
                let mut ids = Vec::new();
                while let Some(id) = seq.next_element::<String>()? {
                    ids.push(id);
                }
                Ok(ids.into_iter().collect())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<IdSet, A::Error> {
                let mut ids = IdSet::default();
                while let Some(key) = map.next_key::<String>()? {
                    let items: Vec<String> = map.next_value()?;
                    if key == "$set" {
                        ids = items.into_iter().collect();
                    }
                }
                Ok(ids)
            }
        }
        deserializer.deserialize_any(SetVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_fixed_matches_javascript() {
        let cases: &[(f64, u32, &str)] = &[
            (0.03125, 4, "0.0313"),
            (-0.03125, 4, "-0.0313"),
            (1.005, 2, "1.00"),
            (1.5, 0, "2"),
            (2.5, 0, "3"),
            (-2.5, 0, "-3"),
            (0.5, 0, "1"),
            (-0.0, 3, "0.000"),
            (-0.00001, 3, "-0.000"),
            (123.456, 1, "123.5"),
            (1e21, 2, "1e+21"),
            (f64::INFINITY, 2, "Infinity"),
            (f64::NEG_INFINITY, 2, "-Infinity"),
            (f64::NAN, 2, "NaN"),
            (0.1, 20, "0.10000000000000000555"),
            (5e-324, 4, "0.0000"),
            (0.125, 2, "0.13"),
            (4503599627370494.5, 0, "4503599627370495"),
            (2251799813685247.0 + 0.25, 1, "2251799813685247.3"),
            (1125899906842623.0 + 0.125, 2, "1125899906842623.13"),
            (9.5, 0, "10"),
            (99.95, 1, "100.0"),
            (0.5, 0, "1"),
        ];
        for (x, d, want) in cases {
            assert_eq!(to_fixed(*x, *d), *want, "{x}.toFixed({d})");
        }
    }

    #[test]
    fn record_iterates_as_object_keys() {
        let keys = ["b", "10", "01", "1", "-1", "0", "4294967295", "4294967294"];
        let mut r: Record<usize> = keys.iter().enumerate().map(|(i, k)| (*k, i)).collect();
        r.insert("b", 99);
        let order: Vec<(&str, usize)> = r.iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(
            order,
            [
                ("0", 5),
                ("1", 3),
                ("10", 1),
                ("4294967294", 7),
                ("b", 99),
                ("01", 2),
                ("-1", 4),
                ("4294967295", 6)
            ]
        );
        assert_eq!((r.len(), r.is_empty(), r.get("01"), r.get("zz")), (8, false, Some(&2), None));
        assert!(Record::<u8>::new().is_empty());
        assert_eq!(array_index(""), None);
        assert_eq!(array_index("1a"), None);
    }

    #[test]
    fn string_to_number_matches_javascript() {
        let cases: &[(&str, f64)] = &[
            ("", 0.0),
            ("  ", 0.0),
            ("1", 1.0),
            (" 2\n", 2.0),
            ("0x1f", 31.0),
            ("0b101", 5.0),
            ("0o17", 15.0),
            ("1e3", 1000.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("+3", 3.0),
            ("-Infinity", f64::NEG_INFINITY),
        ];
        for (s, want) in cases {
            assert_eq!(string_to_number(s), *want, "{s:?}");
        }
        for s in ["inf", "nan", "infinity", "1e", "0x", "-0x1", "1_0", ".", "e5", "1.2.3"] {
            assert!(string_to_number(s).is_nan(), "{s:?}");
        }
        assert!(number_after_first_unit("\u{1F600}1").is_nan());
        assert_eq!(number_after_first_unit("v12"), 12.0);
        assert_eq!(number_after_first_unit(""), 0.0);
    }
}
