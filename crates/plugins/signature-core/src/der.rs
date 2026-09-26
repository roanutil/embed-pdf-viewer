//! BER in, DER out: the slice of `asn1js` this package leans on.
//!
//! The reader follows `asn1js.fromBER`: it decodes the FIRST element of the
//! input and ignores what follows it (a PDF's `/Contents` is zero-padded),
//! accepts definite and indefinite lengths, and fails the whole parse when
//! any element inside it is truncated or its length cannot be read.
//!
//! The writer builds the handful of shapes `pkijs` writes, byte for byte:
//! DER lengths, SETs in the order given (asn1js never sorts a SET), and an
//! INTEGER written from bytes exactly as given.

use epdf_kernel::Error;

pub const UNIVERSAL: u8 = 0;
pub const CONTEXT: u8 = 2;

pub const INTEGER: u32 = 2;
pub const BIT_STRING: u32 = 3;
pub const OCTET_STRING: u32 = 4;
pub const NULL: u32 = 5;
pub const OID: u32 = 6;
pub const UTF8_STRING: u32 = 12;
pub const SEQUENCE: u32 = 16;
pub const SET: u32 = 17;
pub const UTC_TIME: u32 = 23;
pub const GENERALIZED_TIME: u32 = 24;

/// The universal string types `pkijs`'s name comparison treats as text.
const STRING_TAGS: &[u32] = &[12, 30, 28, 18, 19, 20, 21, 22, 25, 26, 27, 29];

/// One decoded element.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// 0 universal, 1 application, 2 context-specific, 3 private.
    pub class: u8,
    pub constructed: bool,
    pub tag: u32,
    /// The whole element as it arrived: header, content, end-of-contents.
    pub raw: Vec<u8>,
    /// A primitive element's content octets.
    pub content: Vec<u8>,
    /// A constructed element's members.
    pub children: Vec<Node>,
}

impl Node {
    pub fn is(&self, class: u8, tag: u32) -> bool {
        self.class == class && self.tag == tag
    }

    pub fn is_universal(&self, tag: u32) -> bool {
        self.is(UNIVERSAL, tag)
    }

    pub fn is_sequence(&self) -> bool {
        self.is_universal(SEQUENCE) && self.constructed
    }

    pub fn is_set(&self) -> bool {
        self.is_universal(SET) && self.constructed
    }

    pub fn is_context(&self, tag: u32) -> bool {
        self.is(CONTEXT, tag)
    }

    /// An OBJECT IDENTIFIER's dotted text.
    pub fn oid(&self) -> Option<String> {
        if !self.is_universal(OID) || self.constructed {
            return None;
        }
        decode_oid(&self.content)
    }

    pub fn is_octet_string(&self) -> bool {
        self.is_universal(OCTET_STRING)
    }

    /// An OCTET STRING's bytes: the content of a primitive one, the joined
    /// content of a constructed one.
    pub fn octets(&self) -> Vec<u8> {
        if self.constructed {
            self.children.iter().flat_map(|c| c.octets()).collect()
        } else {
            self.content.clone()
        }
    }

    /// A BIT STRING's bits after the unused-bits octet.
    pub fn bits(&self) -> Vec<u8> {
        if self.content.is_empty() { Vec::new() } else { self.content[1..].to_vec() }
    }

    pub fn is_string(&self) -> bool {
        self.class == UNIVERSAL && STRING_TAGS.contains(&self.tag)
    }

    /// A string type's text, as `asn1js` decodes it.
    pub fn text(&self) -> Option<String> {
        if !self.is_string() {
            return None;
        }
        Some(match self.tag {
            // BMPString: UTF-16BE.
            30 => {
                let units: Vec<u16> = self
                    .content
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]))
                    .collect();
                String::from_utf16_lossy(&units)
            }
            // UniversalString: UTF-32BE.
            28 => self
                .content
                .chunks(4)
                .filter_map(|c| {
                    let mut b = [0u8; 4];
                    b[..c.len()].copy_from_slice(c);
                    char::from_u32(u32::from_be_bytes(b))
                })
                .collect(),
            12 => String::from_utf8_lossy(&self.content).into_owned(),
            // The single-byte string types decode one character per octet.
            _ => self.content.iter().map(|&b| b as char).collect(),
        })
    }

    /// A UTCTime's or GeneralizedTime's instant, in epoch milliseconds.
    pub fn time(&self) -> Option<f64> {
        let text: String = self.content.iter().map(|&b| b as char).collect();
        match (self.class, self.tag) {
            (UNIVERSAL, UTC_TIME) => parse_utc_time(&text),
            (UNIVERSAL, GENERALIZED_TIME) => parse_generalized_time(&text),
            _ => None,
        }
    }
}

// ── reading ──────────────────────────────────────────────────────────────────

/// `asn1js.fromBER(bytes)`: the first element, or `None` where asn1js answers
/// `offset: -1`.
pub fn from_ber(bytes: &[u8]) -> Option<Node> {
    read(bytes, 0).map(|(node, _)| node)
}

fn read(bytes: &[u8], start: usize) -> Option<(Node, usize)> {
    let mut at = start;
    let first = *bytes.get(at)?;
    at += 1;
    let class = first >> 6;
    let constructed = first & 0x20 != 0;
    let mut tag = u32::from(first & 0x1f);
    if tag == 0x1f {
        tag = 0;
        loop {
            let b = *bytes.get(at)?;
            at += 1;
            tag = tag.checked_mul(128)?.checked_add(u32::from(b & 0x7f))?;
            if b & 0x80 == 0 {
                break;
            }
        }
    }
    let len_byte = *bytes.get(at)?;
    at += 1;
    let length: Option<usize> = if len_byte == 0x80 {
        None
    } else if len_byte & 0x80 == 0 {
        Some(usize::from(len_byte))
    } else {
        let count = usize::from(len_byte & 0x7f);
        if count == 0x7f || count > 8 {
            return None;
        }
        let mut n: usize = 0;
        for _ in 0..count {
            n = n.checked_mul(256)?.checked_add(usize::from(*bytes.get(at)?))?;
            at += 1;
        }
        Some(n)
    };
    match length {
        Some(n) => {
            let end = at.checked_add(n)?;
            if end > bytes.len() {
                return None;
            }
            let body = &bytes[at..end];
            let mut children = Vec::new();
            if constructed {
                let mut cursor = 0;
                while cursor < body.len() {
                    let (child, next) = read(body, cursor)?;
                    children.push(child);
                    cursor = next;
                }
            }
            Some((
                Node {
                    class,
                    constructed,
                    tag,
                    raw: bytes[start..end].to_vec(),
                    content: if constructed { Vec::new() } else { body.to_vec() },
                    children,
                },
                end,
            ))
        }
        None => {
            if !constructed {
                return None;
            }
            let mut children = Vec::new();
            loop {
                if bytes.get(at..at + 2)? == [0, 0] {
                    at += 2;
                    break;
                }
                let (child, next) = read(bytes, at)?;
                children.push(child);
                at = next;
            }
            Some((
                Node {
                    class,
                    constructed,
                    tag,
                    raw: bytes[start..at].to_vec(),
                    content: Vec::new(),
                    children,
                },
                at,
            ))
        }
    }
}

pub fn decode_oid(content: &[u8]) -> Option<String> {
    if content.is_empty() {
        return None;
    }
    let mut arcs: Vec<u128> = Vec::new();
    let mut value: u128 = 0;
    for (i, &b) in content.iter().enumerate() {
        value = value.checked_mul(128)? | u128::from(b & 0x7f);
        if b & 0x80 == 0 {
            arcs.push(value);
            value = 0;
        } else if i == content.len() - 1 {
            return None;
        }
    }
    let first = arcs[0];
    let (a, b) = if first < 40 {
        (0, first)
    } else if first < 80 {
        (1, first - 40)
    } else {
        (2, first - 80)
    };
    let mut out = format!("{a}.{b}");
    for arc in &arcs[1..] {
        out.push('.');
        out.push_str(&arc.to_string());
    }
    Some(out)
}

// ── writing ──────────────────────────────────────────────────────────────────

fn length(n: usize) -> Vec<u8> {
    if n < 0x80 {
        return vec![n as u8];
    }
    let bytes: Vec<u8> = n.to_be_bytes().iter().copied().skip_while(|&b| b == 0).collect();
    let mut out = vec![0x80 | bytes.len() as u8];
    out.extend(bytes);
    out
}

/// One element: an identifier octet and the DER length of `content`.
pub fn tlv(identifier: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![identifier];
    out.extend(length(content.len()));
    out.extend_from_slice(content);
    out
}

pub fn sequence(members: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x30, &members.concat())
}

pub fn set(members: &[Vec<u8>]) -> Vec<u8> {
    tlv(0x31, &members.concat())
}

/// `[n]` constructed, context-specific: an EXPLICIT tag, or an IMPLICIT one
/// over a constructed type.
pub fn context(n: u8, members: &[Vec<u8>]) -> Vec<u8> {
    tlv(0xa0 | n, &members.concat())
}

pub fn octet_string(bytes: &[u8]) -> Vec<u8> {
    tlv(0x04, bytes)
}

pub fn null() -> Vec<u8> {
    vec![0x05, 0x00]
}

pub fn boolean(value: bool) -> Vec<u8> {
    tlv(0x01, &[if value { 0xff } else { 0x00 }])
}

pub fn utf8_string(text: &str) -> Vec<u8> {
    tlv(0x0c, text.as_bytes())
}

pub fn bit_string(unused: u8, bits: &[u8]) -> Vec<u8> {
    let mut content = vec![unused];
    content.extend_from_slice(bits);
    tlv(0x03, &content)
}

/// `new asn1js.Integer({ valueHex })`: the bytes as given.
pub fn integer_bytes(bytes: &[u8]) -> Vec<u8> {
    tlv(0x02, bytes)
}

/// `new asn1js.Integer({ value })` for a small non-negative value.
pub fn integer(value: u64) -> Vec<u8> {
    let mut bytes: Vec<u8> = value.to_be_bytes().iter().copied().skip_while(|&b| b == 0).collect();
    if bytes.is_empty() || bytes[0] & 0x80 != 0 {
        bytes.insert(0, 0);
    }
    integer_bytes(&bytes)
}

/// `Integer.convertToDER()`, exactly as asn1js does it: a set high bit gets
/// one zero byte in front, and otherwise ONE leading zero is dropped when
/// the next byte's high bit is clear. Further leading zeros stay.
pub fn integer_der(bytes: &[u8]) -> Vec<u8> {
    let first = bytes.first().copied().unwrap_or(0);
    let second = bytes.get(1).copied().unwrap_or(0);
    let out = if first & 0x80 != 0 {
        let mut out = vec![0];
        out.extend_from_slice(bytes);
        out
    } else if first == 0 && second & 0x80 == 0 && !bytes.is_empty() {
        bytes[1..].to_vec()
    } else {
        bytes.to_vec()
    };
    integer_bytes(&out)
}

/// `Integer.convertFromDER()`: a leading zero that only carried the sign is
/// dropped.
pub fn integer_from_der(content: &[u8]) -> Vec<u8> {
    if content.len() > 1 && content[0] == 0 && content[1] & 0x80 != 0 {
        content[1..].to_vec()
    } else {
        content.to_vec()
    }
}

pub fn oid(dotted: &str) -> Vec<u8> {
    let arcs: Vec<u128> = dotted.split('.').map(|a| a.parse().unwrap_or(0)).collect();
    let mut content = Vec::new();
    let mut push = |mut value: u128| {
        let mut chunk = vec![(value & 0x7f) as u8];
        value >>= 7;
        while value > 0 {
            chunk.push(0x80 | (value & 0x7f) as u8);
            value >>= 7;
        }
        chunk.reverse();
        content.extend(chunk);
    };
    push(arcs[0] * 40 + arcs.get(1).copied().unwrap_or(0));
    for &arc in arcs.iter().skip(2) {
        push(arc);
    }
    tlv(0x06, &content)
}

// ── time ─────────────────────────────────────────────────────────────────────

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `(year, month, day, hour, minute, second)` of an instant, in UTC.
pub fn civil(ms: f64) -> (i64, i64, i64, i64, i64, i64) {
    let secs = (ms / 1000.0).floor() as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d, rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn epoch_ms(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64) -> f64 {
    ((days_from_civil(y, mo, d) * 86_400 + h * 3600 + mi * 60 + s) * 1000) as f64
}

fn digits(text: &str, at: usize, n: usize) -> Option<i64> {
    let part = text.get(at..at + n)?;
    if !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    part.parse().ok()
}

/// asn1js reads `YYMMDDhhmmssZ`; a two-digit year of 50 or more is 19xx.
pub fn parse_utc_time(text: &str) -> Option<f64> {
    let yy = digits(text, 0, 2)?;
    let year = if yy >= 50 { 1900 + yy } else { 2000 + yy };
    Some(epoch_ms(
        year,
        digits(text, 2, 2)?,
        digits(text, 4, 2)?,
        digits(text, 6, 2)?,
        digits(text, 8, 2)?,
        digits(text, 10, 2)?,
    ))
}

/// `YYYYMMDDhhmmss[.fff]Z`.
pub fn parse_generalized_time(text: &str) -> Option<f64> {
    let base = epoch_ms(
        digits(text, 0, 4)?,
        digits(text, 4, 2)?,
        digits(text, 6, 2)?,
        digits(text, 8, 2)?,
        digits(text, 10, 2)?,
        digits(text, 12, 2)?,
    );
    let fraction = text
        .get(14..)
        .and_then(|rest| rest.strip_prefix('.'))
        .map(|rest| rest.chars().take_while(char::is_ascii_digit).collect::<String>())
        .filter(|f| !f.is_empty())
        .map(|f| format!("0.{f}").parse::<f64>().unwrap_or(0.0) * 1000.0)
        .unwrap_or(0.0);
    Some(base + fraction.floor())
}

/// `new asn1js.UTCTime({ valueDate })`.
pub fn utc_time(ms: f64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = civil(ms);
    tlv(0x17, format!("{:02}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z", y % 100).as_bytes())
}

/// `new asn1js.GeneralizedTime({ valueDate })`, as pkijs's `Time` writes a
/// date from 2050 on.
pub fn generalized_time(ms: f64) -> Vec<u8> {
    let (y, mo, d, h, mi, s) = civil(ms);
    tlv(0x18, format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}{s:02}Z").as_bytes())
}

/// A malformed-input error shaped like asn1js's.
pub fn schema_error(target: &str) -> Error {
    Error::new(format!("Object's schema was not verified against input data for {target}"))
}
