//! `packages/engine/core/src/signature/analysis/pdf-value.ts`: the fork's
//! shallow serialisation of a PDF object, parsed, and compared.
//!
//! Text is latin-1 (one char per byte), as the TypeScript reads it with
//! `String.fromCharCode` per byte. A dictionary keeps JavaScript's object
//! key order (integer-like keys first, ascending, then insertion order),
//! because `changedKeys` lists keys in that order and its lists reach the
//! findings' text.

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A dictionary, in JavaScript object order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entries(Vec<(String, PdfValue)>);

/// A canonical array index, as JavaScript orders object keys by.
fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    key.parse::<u32>().ok().filter(|n| *n < u32::MAX)
}

impl Entries {
    pub fn get(&self, key: &str) -> Option<&PdfValue> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// `entries[key] = value`: replaces in place, or appends.
    pub fn insert(&mut self, key: String, value: PdfValue) {
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.0.push((key, value)),
        }
    }

    /// `Object.keys(entries)`.
    pub fn keys(&self) -> Vec<&str> {
        let mut indexed: Vec<(u32, &str)> =
            self.0.iter().filter_map(|(k, _)| array_index(k).map(|n| (n, k.as_str()))).collect();
        indexed.sort_by_key(|(n, _)| *n);
        let mut out: Vec<&str> = indexed.into_iter().map(|(_, k)| k).collect();
        out.extend(
            self.0.iter().filter(|(k, _)| array_index(k).is_none()).map(|(k, _)| k.as_str()),
        );
        out
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for Entries {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let keys = self.keys();
        let mut map = serializer.serialize_map(Some(keys.len()))?;
        for key in keys {
            map.serialize_entry(key, self.get(key).expect("a listed key"))?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map = serde_json::Map::<String, serde_json::Value>::deserialize(deserializer)?;
        let mut out = Entries::default();
        for (k, v) in map {
            out.insert(k, serde_json::from_value(v).map_err(D::Error::custom)?);
        }
        Ok(out)
    }
}

/// `PdfValue`.
#[derive(Debug, Clone, PartialEq)]
pub enum PdfValue {
    Dict(Entries),
    Array(Vec<PdfValue>),
    Ref(u32),
    Name(String),
    String(String),
    Number(f64),
    Bool(bool),
    Null,
    Stream { length: u64, sha256: String, dict: Entries },
}

/// A JavaScript number as JSON: whole numbers as integers.
fn js_number(v: f64) -> serde_json::Value {
    if v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 {
        serde_json::json!(v as i64)
    } else {
        serde_json::json!(v)
    }
}

impl Serialize for PdfValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self {
            PdfValue::Dict(entries) => {
                map.serialize_entry("t", "dict")?;
                map.serialize_entry("entries", entries)?;
            }
            PdfValue::Array(items) => {
                map.serialize_entry("t", "array")?;
                map.serialize_entry("items", items)?;
            }
            PdfValue::Ref(num) => {
                map.serialize_entry("t", "ref")?;
                map.serialize_entry("num", num)?;
            }
            PdfValue::Name(v) => {
                map.serialize_entry("t", "name")?;
                map.serialize_entry("v", v)?;
            }
            PdfValue::String(v) => {
                map.serialize_entry("t", "string")?;
                map.serialize_entry("v", v)?;
            }
            PdfValue::Number(v) => {
                map.serialize_entry("t", "number")?;
                map.serialize_entry("v", &js_number(*v))?;
            }
            PdfValue::Bool(v) => {
                map.serialize_entry("t", "bool")?;
                map.serialize_entry("v", v)?;
            }
            PdfValue::Null => map.serialize_entry("t", "null")?,
            PdfValue::Stream { length, sha256, dict } => {
                map.serialize_entry("t", "stream")?;
                map.serialize_entry("length", length)?;
                map.serialize_entry("sha256", sha256)?;
                map.serialize_entry("dict", dict)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for PdfValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        let field = |name: &str| value.get(name).cloned().unwrap_or(serde_json::Value::Null);
        fn parse<T: serde::de::DeserializeOwned, E: serde::de::Error>(
            v: serde_json::Value,
        ) -> Result<T, E> {
            serde_json::from_value(v).map_err(E::custom)
        }
        Ok(match value.get("t").and_then(serde_json::Value::as_str) {
            Some("dict") => PdfValue::Dict(parse::<_, D::Error>(field("entries"))?),
            Some("array") => PdfValue::Array(parse::<_, D::Error>(field("items"))?),
            Some("ref") => PdfValue::Ref(parse::<_, D::Error>(field("num"))?),
            Some("name") => PdfValue::Name(parse::<_, D::Error>(field("v"))?),
            Some("string") => PdfValue::String(parse::<_, D::Error>(field("v"))?),
            Some("number") => PdfValue::Number(parse::<_, D::Error>(field("v"))?),
            Some("bool") => PdfValue::Bool(parse::<_, D::Error>(field("v"))?),
            Some("null") => PdfValue::Null,
            Some("stream") => PdfValue::Stream {
                length: parse::<_, D::Error>(field("length"))?,
                sha256: parse::<_, D::Error>(field("sha256"))?,
                dict: parse::<_, D::Error>(field("dict"))?,
            },
            other => return Err(D::Error::custom(format!("not a PdfValue: {other:?}"))),
        })
    }
}

/// `parsePdfValue`.
pub fn parse_pdf_value(text: &str) -> Result<PdfValue, String> {
    let mut parser = Parser { text: text.chars().collect(), pos: 0 };
    let value = parser.value()?;
    parser.skip_spaces();
    if !parser.at_end() {
        let rest: String = parser.text[parser.pos..].iter().take(20).collect();
        return Err(format!("trailing input at {}: {rest}", parser.pos));
    }
    Ok(value)
}

struct Parser {
    text: Vec<char>,
    pos: usize,
}

fn is_hex(c: char) -> bool {
    c.is_ascii_hexdigit()
}

/// `parseInt(s, 16)` over at most two chars: the leading hex digits, or 0
/// where JavaScript would answer `NaN` (which `fromCharCode` reads as 0).
fn parse_hex_prefix(s: &str) -> u32 {
    let digits: String = s.chars().take_while(|c| is_hex(*c)).collect();
    u32::from_str_radix(&digits, 16).unwrap_or(0)
}

impl Parser {
    fn at_end(&self) -> bool {
        self.pos >= self.text.len()
    }

    fn starts_with(&self, token: &str) -> bool {
        (self.pos..).zip(token.chars()).all(|(i, c)| self.text.get(i) == Some(&c))
    }

    fn peek(&self) -> Option<char> {
        self.text.get(self.pos).copied()
    }

    fn rest(&self) -> String {
        self.text[self.pos.min(self.text.len())..].iter().collect()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    fn unexpected(&self) -> String {
        let rest: String = self.text[self.pos.min(self.text.len())..].iter().take(20).collect();
        format!("unexpected input at {}: {rest}", self.pos)
    }

    fn value(&mut self) -> Result<PdfValue, String> {
        self.skip_spaces();
        if self.starts_with("<<") {
            return Ok(PdfValue::Dict(self.dict_entries()?));
        }
        if self.starts_with("stream(") {
            return self.stream();
        }
        match self.peek() {
            Some('[') => return self.array(),
            Some('/') => return Ok(PdfValue::Name(self.name()?)),
            Some('(') => return Ok(PdfValue::String(self.literal_string()?)),
            Some('<') => return Ok(PdfValue::String(self.hex_string()?)),
            _ => {}
        }
        if self.starts_with("true") {
            self.pos += 4;
            return Ok(PdfValue::Bool(true));
        }
        if self.starts_with("false") {
            self.pos += 5;
            return Ok(PdfValue::Bool(false));
        }
        if self.starts_with("null") {
            self.pos += 4;
            return Ok(PdfValue::Null);
        }
        // /^(\d+) 0 R(?![0-9A-Za-z])/
        let start = self.pos;
        let mut i = start;
        while self.text.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        if i > start {
            let tail: String = self.text[i..].iter().take(4).collect();
            let after = self.text.get(i + 4);
            if tail == " 0 R" && !after.is_some_and(|c| c.is_ascii_alphanumeric()) {
                let digits: String = self.text[start..i].iter().collect();
                if let Ok(num) = digits.parse::<u32>() {
                    self.pos = i + 4;
                    return Ok(PdfValue::Ref(num));
                }
            }
        }
        // /^[+-]?(?:\d+\.?\d*|\.\d+)/
        let mut i = start;
        if matches!(self.text.get(i), Some('+' | '-')) {
            i += 1;
        }
        let int_start = i;
        while self.text.get(i).is_some_and(char::is_ascii_digit) {
            i += 1;
        }
        let matched = if i > int_start {
            if self.text.get(i) == Some(&'.') {
                i += 1;
                while self.text.get(i).is_some_and(char::is_ascii_digit) {
                    i += 1;
                }
            }
            true
        } else if self.text.get(i) == Some(&'.')
            && self.text.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            i += 1;
            while self.text.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            true
        } else {
            false
        };
        if matched {
            let literal: String = self.text[start..i].iter().collect();
            let v = literal.parse::<f64>().map_err(|_| self.unexpected())?;
            self.pos = i;
            return Ok(PdfValue::Number(v));
        }
        Err(self.unexpected())
    }

    fn dict_entries(&mut self) -> Result<Entries, String> {
        self.expect("<<")?;
        let mut entries = Entries::default();
        loop {
            self.skip_spaces();
            if self.starts_with(">>") {
                self.pos += 2;
                return Ok(entries);
            }
            if self.peek() != Some('/') {
                return Err(format!("expected a key at {}", self.pos));
            }
            let key = self.name()?;
            self.skip_spaces();
            let value = self.value()?;
            entries.insert(key, value);
        }
    }

    fn stream(&mut self) -> Result<PdfValue, String> {
        // /^stream\((\d+),([0-9a-fA-F]*)\)/
        let malformed = || format!("malformed stream signature at {}", self.pos);
        let rest = self.rest();
        let body = rest.strip_prefix("stream(").ok_or_else(malformed)?;
        let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return Err(malformed());
        }
        let after = &body[digits.len()..];
        let after = after.strip_prefix(',').ok_or_else(malformed)?;
        let hex: String = after.chars().take_while(|c| is_hex(*c)).collect();
        if !after[hex.len()..].starts_with(')') {
            return Err(malformed());
        }
        let length = digits.parse::<u64>().map_err(|_| malformed())?;
        self.pos += "stream(".len() + digits.len() + 1 + hex.len() + 1;
        let dict = self.dict_entries()?;
        Ok(PdfValue::Stream { length, sha256: hex.to_lowercase(), dict })
    }

    fn array(&mut self) -> Result<PdfValue, String> {
        self.expect("[")?;
        let mut items = Vec::new();
        loop {
            self.skip_spaces();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(PdfValue::Array(items));
            }
            if self.at_end() {
                return Err(self.unexpected());
            }
            items.push(self.value()?);
        }
    }

    fn name(&mut self) -> Result<String, String> {
        self.expect("/")?;
        let mut out = String::new();
        while let Some(ch) = self.peek() {
            if matches!(ch, ' ' | '/' | '[' | ']' | '<' | '>' | '(' | ')') {
                break;
            }
            if ch == '#'
                && self.text.get(self.pos + 1).is_some_and(|c| is_hex(*c))
                && self.text.get(self.pos + 2).is_some_and(|c| is_hex(*c))
            {
                let hex: String = self.text[self.pos + 1..self.pos + 3].iter().collect();
                out.push(char::from_u32(parse_hex_prefix(&hex)).unwrap_or('\0'));
                self.pos += 3;
                continue;
            }
            out.push(ch);
            self.pos += 1;
        }
        Ok(out)
    }

    fn literal_string(&mut self) -> Result<String, String> {
        self.expect("(")?;
        let mut depth = 1;
        let mut out = String::new();
        while self.pos < self.text.len() {
            let ch = self.text[self.pos];
            self.pos += 1;
            if ch == '\\' {
                let next = self.text.get(self.pos).copied();
                self.pos += 1;
                match next {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('b') => out.push('\u{8}'),
                    Some('f') => out.push('\u{c}'),
                    Some('\n') => {}
                    Some('\r') => {
                        if self.peek() == Some('\n') {
                            self.pos += 1;
                        }
                    }
                    other => {
                        // /^[0-7]{1,3}/ from the escaped char.
                        let start = self.pos - 1;
                        let mut end = start;
                        while end < start + 3
                            && self.text.get(end).is_some_and(|c| ('0'..='7').contains(c))
                        {
                            end += 1;
                        }
                        if end > start {
                            let oct: String = self.text[start..end].iter().collect();
                            let code = u32::from_str_radix(&oct, 8).unwrap_or(0);
                            out.push(char::from_u32(code).unwrap_or('\0'));
                            self.pos += end - start - 1;
                        } else if let Some(c) = other {
                            out.push(c);
                        }
                    }
                }
                continue;
            }
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
                if depth == 0 {
                    return Ok(out);
                }
            }
            out.push(ch);
        }
        Err("unterminated string".into())
    }

    fn hex_string(&mut self) -> Result<String, String> {
        self.expect("<")?;
        let end = self.text[self.pos..]
            .iter()
            .position(|c| *c == '>')
            .map(|i| i + self.pos)
            .ok_or_else(|| "unterminated hex string".to_string())?;
        let hex: String = self.text[self.pos..end].iter().filter(|c| !c.is_whitespace()).collect();
        self.pos = end + 1;
        let chars: Vec<char> = hex.chars().collect();
        let mut out = String::new();
        for pair in chars.chunks(2) {
            let mut s: String = pair.iter().collect();
            while s.chars().count() < 2 {
                s.push('0');
            }
            out.push(char::from_u32(parse_hex_prefix(&s)).unwrap_or('\0'));
        }
        Ok(out)
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        if !self.starts_with(token) {
            return Err(format!("expected {token} at {}", self.pos));
        }
        self.pos += token.chars().count();
        Ok(())
    }
}

/// `pdfValueEquals`.
pub fn pdf_value_equals(a: Option<&PdfValue>, b: Option<&PdfValue>) -> bool {
    stable_stringify(a) == stable_stringify(b)
}

/// JavaScript's `String(number)` for the values a PDF holds.
fn js_number_text(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e21 { format!("{}", v as i128) } else { format!("{v}") }
}

/// `stableStringify`.
pub fn stable_stringify(value: Option<&PdfValue>) -> String {
    let Some(value) = value else { return "null".into() };
    match value {
        PdfValue::Dict(entries) => dict_text(entries),
        PdfValue::Stream { length, sha256, dict } => {
            format!("stream({length},{sha256}){}", dict_text(dict))
        }
        PdfValue::Array(items) => format!(
            "[{}]",
            items.iter().map(|i| stable_stringify(Some(i))).collect::<Vec<_>>().join(" ")
        ),
        PdfValue::Ref(num) => format!("{num} 0 R"),
        PdfValue::Name(v) => format!("/{}", encode_name(v)),
        PdfValue::String(v) => {
            let mut out = String::from("(");
            for ch in v.chars() {
                if matches!(ch, '\\' | '(' | ')') {
                    out.push('\\');
                }
                out.push(ch);
            }
            out.push(')');
            out
        }
        PdfValue::Number(v) => js_number_text(*v),
        PdfValue::Bool(v) => if *v { "true" } else { "false" }.into(),
        PdfValue::Null => "null".into(),
    }
}

fn dict_text(entries: &Entries) -> String {
    let mut keys = entries.keys();
    // `Object.keys(...).sort()`: UTF-16 code unit order, which for latin-1
    // keys is char order.
    keys.sort_unstable();
    let body: String =
        keys.iter().map(|k| format!("/{k} {}", stable_stringify(entries.get(k)))).collect();
    format!("<<{body}>>")
}

/// `dictEntries`.
pub fn dict_entries(value: Option<&PdfValue>) -> Option<&Entries> {
    match value? {
        PdfValue::Dict(entries) => Some(entries),
        PdfValue::Stream { dict, .. } => Some(dict),
        _ => None,
    }
}

/// `dictEntries(value)?.[key]`.
pub fn entry<'a>(value: Option<&'a PdfValue>, key: &str) -> Option<&'a PdfValue> {
    dict_entries(value)?.get(key)
}

/// `changedKeys`: keys whose value differs, in `new Set([...keys(a),
/// ...keys(b)])` order.
pub fn changed_keys(a: Option<&PdfValue>, b: Option<&PdfValue>) -> Vec<String> {
    let empty = Entries::default();
    let ea = dict_entries(a).unwrap_or(&empty);
    let eb = dict_entries(b).unwrap_or(&empty);
    let mut keys: Vec<&str> = ea.keys();
    for k in eb.keys() {
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.into_iter()
        .filter(|k| !pdf_value_equals(ea.get(k), eb.get(k)))
        .map(str::to_owned)
        .collect()
}

/// `refsOf`.
pub fn refs_of(value: Option<&PdfValue>) -> Vec<u32> {
    match value {
        Some(PdfValue::Ref(num)) => vec![*num],
        Some(PdfValue::Array(items)) => items.iter().flat_map(|i| refs_of(Some(i))).collect(),
        _ => Vec::new(),
    }
}

/// `encodeName`.
pub fn encode_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        let code = ch as u32;
        if !(0x21..=0x7e).contains(&code) || "#/[]<>(){}%".contains(ch) {
            out.push_str(&format!("#{:02X}", code));
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_forks_serialisation() {
        let v = parse_pdf_value("<</AP 12 0 R/Ff 1/T (A\\(b\\)) /V <4142>/Rect [0 .5 -1.25 3.]>>")
            .unwrap();
        let e = dict_entries(Some(&v)).unwrap();
        assert_eq!(e.get("AP"), Some(&PdfValue::Ref(12)));
        assert_eq!(e.get("T"), Some(&PdfValue::String("A(b)".into())));
        assert_eq!(e.get("V"), Some(&PdfValue::String("AB".into())));
        assert_eq!(
            e.get("Rect"),
            Some(&PdfValue::Array(vec![
                PdfValue::Number(0.0),
                PdfValue::Number(0.5),
                PdfValue::Number(-1.25),
                PdfValue::Number(3.0)
            ]))
        );
        let s = parse_pdf_value("stream(10,ABcd)<</Length 10>>").unwrap();
        assert_eq!(stable_stringify(Some(&s)), "stream(10,abcd)<</Length 10>>");
        assert_eq!(
            stable_stringify(Some(&v)),
            "<</AP 12 0 R/Ff 1/Rect [0 0.5 -1.25 3]/T (A\\(b\\))/V (AB)>>"
        );
    }

    #[test]
    fn changed_keys_follow_javascript_order() {
        let a = parse_pdf_value("<</B 1/A 2/1 3>>").unwrap();
        let b = parse_pdf_value("<</C 1/A 3/0 2>>").unwrap();
        assert_eq!(changed_keys(Some(&a), Some(&b)), vec!["1", "B", "A", "0", "C"]);
    }

    #[test]
    fn names_decode_and_encode() {
        let v = parse_pdf_value("/A#20B").unwrap();
        assert_eq!(v, PdfValue::Name("A B".into()));
        assert_eq!(encode_name("A B"), "A#20B");
    }
}
