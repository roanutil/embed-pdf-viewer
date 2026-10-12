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

// ── Array.prototype.sort ────────────────────────────────────────────────────

/// `array.sort(compare)` as V8 runs it: the TimSort in V8's
/// `third_party/v8/builtins/array-sort.tq`, step for step, so even a
/// comparator that is not a total order (one that answers `NaN`, which the
/// spec reads as `0`) leaves the elements where V8 leaves them, and never
/// panics. A consistent comparator gets the one stable order any stable sort
/// gives.
///
/// `compare` answers like a JavaScript comparator: negative, zero, positive,
/// or `NaN` for "equal".
pub fn sort_by<T>(items: &mut Vec<T>, mut compare: impl FnMut(&T, &T) -> f64) {
    if items.len() < 2 {
        return;
    }
    let order = {
        let items = &*items;
        let mut sort = TimSort {
            work: (0..items.len()).collect(),
            temp: Vec::new(),
            runs: Vec::new(),
            min_gallop: MIN_GALLOP,
            compare: |a: usize, b: usize| {
                let v = compare(&items[a], &items[b]);
                if v.is_nan() { 0.0 } else { v }
            },
        };
        sort.run();
        sort.work
    };
    let mut slots: Vec<Option<T>> = std::mem::take(items).into_iter().map(Some).collect();
    items.extend(order.into_iter().map(|i| slots[i].take().expect("a permutation")));
}

/// [`sort_by`] with a comparator that answers an [`Ordering`](std::cmp::Ordering).
pub fn sort_by_ordering<T>(
    items: &mut Vec<T>,
    mut compare: impl FnMut(&T, &T) -> std::cmp::Ordering,
) {
    sort_by(items, |a, b| compare(a, b) as i8 as f64);
}

const MIN_GALLOP: usize = 7;

/// One sort's state, over indices into the caller's elements.
struct TimSort<F: FnMut(usize, usize) -> f64> {
    work: Vec<usize>,
    temp: Vec<usize>,
    /// Pending runs, `(base, length)`.
    runs: Vec<(usize, usize)>,
    min_gallop: usize,
    compare: F,
}

impl<F: FnMut(usize, usize) -> f64> TimSort<F> {
    fn cmp(&mut self, a: usize, b: usize) -> f64 {
        (self.compare)(a, b)
    }

    /// `ArrayTimSortImpl`.
    fn run(&mut self) {
        let length = self.work.len();
        let mut remaining = length;
        let mut low = 0;
        let min_run = min_run_length(remaining);
        while remaining != 0 {
            let mut current = self.count_and_make_run(low, low + remaining);
            if current < min_run {
                let forced = min_run.min(remaining);
                self.binary_insertion_sort(low, low + current, low + forced);
                current = forced;
            }
            self.runs.push((low, current));
            self.merge_collapse();
            low += current;
            remaining -= current;
        }
        self.merge_force_collapse();
    }

    /// `BinaryInsertionSort(low, start, high)`: `[low, start)` is sorted.
    fn binary_insertion_sort(&mut self, low: usize, start: usize, high: usize) {
        let mut start = if low == start { start + 1 } else { start };
        while start < high {
            let mut left = low;
            let mut right = start;
            let pivot = self.work[start];
            while left < right {
                let mid = left + ((right - left) >> 1);
                let order = self.cmp(pivot, self.work[mid]);
                if order < 0.0 {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.work.copy_within(left..start, left + 1);
            self.work[left] = pivot;
            start += 1;
        }
    }

    /// `CountAndMakeRun(low, high)`: the run starting at `low`, reversed in
    /// place when strictly descending.
    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run_length = 2;
        let element_low = self.work[low];
        let element_low_pre = self.work[low - 1];
        let order = self.cmp(element_low, element_low_pre);
        let descending = order < 0.0;
        let mut previous = element_low;
        for idx in low + 1..high {
            let current = self.work[idx];
            let order = self.cmp(current, previous);
            if descending {
                if order >= 0.0 {
                    break;
                }
            } else if order < 0.0 {
                break;
            }
            previous = current;
            run_length += 1;
        }
        if descending {
            self.work[low_arg..low_arg + run_length].reverse();
        }
        run_length
    }

    fn run_invariant_established(&self, n: usize) -> bool {
        if n < 2 {
            return true;
        }
        self.runs[n - 2].1 > self.runs[n - 1].1 + self.runs[n].1
    }

    /// `MergeCollapse`.
    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if !self.run_invariant_established(n + 1) || !self.run_invariant_established(n) {
                if self.runs[n - 1].1 < self.runs[n + 1].1 {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.runs[n].1 <= self.runs[n + 1].1 {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    /// `MergeForceCollapse`.
    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.runs[n - 1].1 < self.runs[n + 1].1 {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    /// `MergeAt(i)`: merge runs `i` and `i + 1`.
    fn merge_at(&mut self, i: usize) {
        let (mut base_a, mut length_a) = self.runs[i];
        let (base_b, mut length_b) = self.runs[i + 1];
        self.runs[i].1 = length_a + length_b;
        self.runs.remove(i + 1);

        let key_right = self.work[base_b];
        let k = self.gallop_right(false, key_right, base_a, length_a, 0);
        base_a += k;
        length_a -= k;
        if length_a == 0 {
            return;
        }
        let key_left = self.work[base_a + length_a - 1];
        length_b = self.gallop_left(false, key_left, base_b, length_b, length_b - 1);
        if length_b == 0 {
            return;
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    fn at(&self, temp: bool, i: usize) -> usize {
        if temp { self.temp[i] } else { self.work[i] }
    }

    /// `GallopLeft(array, key, base, length, hint)`: where `key` goes in the
    /// sorted `array[base..base + length]`, left of any equal elements.
    fn gallop_left(
        &mut self,
        temp: bool,
        key: usize,
        base: usize,
        length: usize,
        hint: usize,
    ) -> usize {
        let mut last_ofs: isize = 0;
        let mut offset: isize = 1;
        let (length, hint) = (length as isize, hint as isize);
        let base_hint = self.at(temp, base + hint as usize);
        let order = self.cmp(base_hint, key);
        if order < 0.0 {
            let max_ofs = length - hint;
            while offset < max_ofs {
                let element = self.at(temp, (base as isize + hint + offset) as usize);
                if self.cmp(element, key) >= 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            last_ofs += hint;
            offset += hint;
        } else {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                let element = self.at(temp, (base as isize + hint - offset) as usize);
                if self.cmp(element, key) < 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            let tmp = last_ofs;
            last_ofs = hint - offset;
            offset = hint - tmp;
        }
        last_ofs += 1;
        while last_ofs < offset {
            let m = last_ofs + ((offset - last_ofs) >> 1);
            let element = self.at(temp, (base as isize + m) as usize);
            if self.cmp(element, key) < 0.0 {
                last_ofs = m + 1;
            } else {
                offset = m;
            }
        }
        offset as usize
    }

    /// `GallopRight(array, key, base, length, hint)`: as
    /// [`gallop_left`](Self::gallop_left), right of any equal elements.
    fn gallop_right(
        &mut self,
        temp: bool,
        key: usize,
        base: usize,
        length: usize,
        hint: usize,
    ) -> usize {
        let mut last_ofs: isize = 0;
        let mut offset: isize = 1;
        let (length, hint) = (length as isize, hint as isize);
        let base_hint = self.at(temp, base + hint as usize);
        let order = self.cmp(key, base_hint);
        if order < 0.0 {
            let max_ofs = hint + 1;
            while offset < max_ofs {
                let element = self.at(temp, (base as isize + hint - offset) as usize);
                if self.cmp(key, element) >= 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            let tmp = last_ofs;
            last_ofs = hint - offset;
            offset = hint - tmp;
        } else {
            let max_ofs = length - hint;
            while offset < max_ofs {
                let element = self.at(temp, (base as isize + hint + offset) as usize);
                if self.cmp(key, element) < 0.0 {
                    break;
                }
                last_ofs = offset;
                offset = (offset << 1) + 1;
            }
            offset = offset.min(max_ofs);
            last_ofs += hint;
            offset += hint;
        }
        last_ofs += 1;
        while last_ofs < offset {
            let m = last_ofs + ((offset - last_ofs) >> 1);
            let element = self.at(temp, (base as isize + m) as usize);
            if self.cmp(key, element) < 0.0 {
                offset = m;
            } else {
                last_ofs = m + 1;
            }
        }
        offset as usize
    }

    /// `MergeLow`: run A (the shorter) goes to `temp`, the merge runs
    /// forward.
    fn merge_low(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a, length_b);
        self.temp.clear();
        self.temp.extend_from_slice(&self.work[base_a..base_a + length_a]);
        let mut dest = base_a;
        let mut cursor_temp = 0;
        let mut cursor_b = base_b;
        self.work[dest] = self.work[cursor_b];
        dest += 1;
        cursor_b += 1;

        enum Exit {
            Succeed,
            CopyB,
        }
        let exit = 'merge: {
            length_b -= 1;
            if length_b == 0 {
                break 'merge Exit::Succeed;
            }
            if length_a == 1 {
                break 'merge Exit::CopyB;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a = 0;
                let mut wins_b = 0;
                loop {
                    let order = self.cmp(self.work[cursor_b], self.temp[cursor_temp]);
                    if order < 0.0 {
                        self.work[dest] = self.work[cursor_b];
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 0 {
                            break 'merge Exit::Succeed;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest] = self.temp[cursor_temp];
                        dest += 1;
                        cursor_temp += 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 1 {
                            break 'merge Exit::CopyB;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.work[cursor_b];
                    wins_a = self.gallop_right(true, key, cursor_temp, length_a, 0);
                    if wins_a > 0 {
                        self.work[dest..dest + wins_a]
                            .copy_from_slice(&self.temp[cursor_temp..cursor_temp + wins_a]);
                        dest += wins_a;
                        cursor_temp += wins_a;
                        length_a -= wins_a;
                        if length_a == 1 {
                            break 'merge Exit::CopyB;
                        }
                        // Impossible for a consistent comparator; not assumed.
                        if length_a == 0 {
                            break 'merge Exit::Succeed;
                        }
                    }
                    self.work[dest] = self.work[cursor_b];
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        break 'merge Exit::Succeed;
                    }

                    let key = self.temp[cursor_temp];
                    wins_b = self.gallop_left(false, key, cursor_b, length_b, 0);
                    if wins_b > 0 {
                        self.work.copy_within(cursor_b..cursor_b + wins_b, dest);
                        dest += wins_b;
                        cursor_b += wins_b;
                        length_b -= wins_b;
                        if length_b == 0 {
                            break 'merge Exit::Succeed;
                        }
                    }
                    self.work[dest] = self.temp[cursor_temp];
                    dest += 1;
                    cursor_temp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        break 'merge Exit::CopyB;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match exit {
            Exit::Succeed => {
                if length_a > 0 {
                    self.work[dest..dest + length_a]
                        .copy_from_slice(&self.temp[cursor_temp..cursor_temp + length_a]);
                }
            }
            Exit::CopyB => {
                // The last element of run A belongs at the end of the merge.
                self.work.copy_within(cursor_b..cursor_b + length_b, dest);
                self.work[dest + length_b] = self.temp[cursor_temp];
            }
        }
    }

    /// `MergeHigh`: run B (the shorter) goes to `temp`, the merge runs
    /// backward.
    fn merge_high(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a as isize, length_b as isize);
        self.temp.clear();
        self.temp.extend_from_slice(&self.work[base_b..base_b + length_b as usize]);
        let base_a = base_a as isize;
        let mut dest = base_b as isize + length_b - 1;
        let mut cursor_temp = length_b - 1;
        let mut cursor_a = base_a + length_a - 1;
        let w = |i: isize| i as usize;
        self.work[w(dest)] = self.work[w(cursor_a)];
        dest -= 1;
        cursor_a -= 1;

        enum Exit {
            Succeed,
            CopyA,
        }
        let exit = 'merge: {
            length_a -= 1;
            if length_a == 0 {
                break 'merge Exit::Succeed;
            }
            if length_b == 1 {
                break 'merge Exit::CopyA;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let mut wins_a: isize = 0;
                let mut wins_b: isize = 0;
                loop {
                    let order = self.cmp(self.temp[w(cursor_temp)], self.work[w(cursor_a)]);
                    if order < 0.0 {
                        self.work[w(dest)] = self.work[w(cursor_a)];
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 0 {
                            break 'merge Exit::Succeed;
                        }
                        if wins_a >= min_gallop as isize {
                            break;
                        }
                    } else {
                        self.work[w(dest)] = self.temp[w(cursor_temp)];
                        dest -= 1;
                        cursor_temp -= 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 1 {
                            break 'merge Exit::CopyA;
                        }
                        if wins_b >= min_gallop as isize {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP as isize || wins_b >= MIN_GALLOP as isize || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.temp[w(cursor_temp)];
                    let k = self.gallop_right(false, key, w(base_a), w(length_a), w(length_a - 1))
                        as isize;
                    wins_a = length_a - k;
                    if wins_a > 0 {
                        dest -= wins_a;
                        cursor_a -= wins_a;
                        self.work
                            .copy_within(w(cursor_a + 1)..w(cursor_a + 1 + wins_a), w(dest + 1));
                        length_a -= wins_a;
                        if length_a == 0 {
                            break 'merge Exit::Succeed;
                        }
                    }
                    self.work[w(dest)] = self.temp[w(cursor_temp)];
                    dest -= 1;
                    cursor_temp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        break 'merge Exit::CopyA;
                    }

                    let key = self.work[w(cursor_a)];
                    let k = self.gallop_left(true, key, 0, w(length_b), w(length_b - 1)) as isize;
                    wins_b = length_b - k;
                    if wins_b > 0 {
                        dest -= wins_b;
                        cursor_temp -= wins_b;
                        let from = w(cursor_temp + 1);
                        let to = w(dest + 1);
                        let n = w(wins_b);
                        self.work[to..to + n].copy_from_slice(&self.temp[from..from + n]);
                        length_b -= wins_b;
                        if length_b == 1 {
                            break 'merge Exit::CopyA;
                        }
                        // Impossible for a consistent comparator; not assumed.
                        if length_b == 0 {
                            break 'merge Exit::Succeed;
                        }
                    }
                    self.work[w(dest)] = self.work[w(cursor_a)];
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        break 'merge Exit::Succeed;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        };
        match exit {
            Exit::Succeed => {
                if length_b > 0 {
                    let to = w(dest - (length_b - 1));
                    let n = w(length_b);
                    self.work[to..to + n].copy_from_slice(&self.temp[..n]);
                }
            }
            Exit::CopyA => {
                // The first element of run B belongs at the front of the merge.
                dest -= length_a;
                cursor_a -= length_a;
                self.work.copy_within(w(cursor_a + 1)..w(cursor_a + 1 + length_a), w(dest + 1));
                self.work[w(dest)] = self.temp[w(cursor_temp)];
            }
        }
    }
}

/// `ComputeMinRunLength(n)`.
fn min_run_length(mut n: usize) -> usize {
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
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
    fn id_set_reads_a_set_or_an_array() {
        let set: IdSet = serde_json::from_value(serde_json::json!({"$set": ["a", "b"]})).unwrap();
        let array: IdSet = serde_json::from_value(serde_json::json!(["a", "b", "a"])).unwrap();
        assert_eq!(set, array);
        assert!(array.has("b") && !array.has("c"));
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
