//! Whether `asn1js.fromBER` (asn1js 3.0.10) accepts a buffer, refuses it
//! (`offset: -1`), or throws while reading it.
//!
//! `der::from_ber` builds the tree the rest of the package reads; this walk
//! decides accept or refuse the way asn1js does, so a CMS or TSTInfo that
//! asn1js refuses is refused here too. It follows `localFromBER` step for
//! step, offsets included:
//!
//! - the limits of `createFromBerContext`: 100 levels, 10,000 elements and
//!   16 MiB of content per element;
//! - the identifier and length rules (`0xFF` length, more than 8 length
//!   octets, a constructed encoding of a primitive-only universal type,
//!   reserved universal tags, an indefinite primitive);
//! - the value blocks that can refuse: an OBJECT IDENTIFIER whose last arc
//!   runs off the end, a BIT STRING whose unused-bits octet is past 7, a
//!   constructed OCTET or BIT STRING with a member of another type, and the
//!   bounds checks each block makes against the whole buffer (a member may
//!   run past its parent's declared length while the buffer lasts);
//! - the speculative decode of every primitive OCTET STRING's contents, and
//!   of a BIT STRING's when its unused-bits octet is 0. asn1js keeps neither
//!   result nor error, but the elements it counts and the levels it enters
//!   there count toward the limits;
//! - the readers that throw: `GeneralizedTime.fromString`, a BMPString of
//!   odd length, a UniversalString whose length is not a multiple of 4. A
//!   throw inside a speculative decode is caught there, as asn1js catches
//!   it.
//!
//! Not modelled: `String.fromCharCode.apply` over a string of more than
//! about 120,000 octets throws a RangeError whose threshold is the host's
//! stack size.

use epdf_kernel::Error;

use crate::der;

/// asn1js 3.0.10's `DEFAULT_MAX_DEPTH`.
pub const MAX_DEPTH: usize = 100;
/// asn1js 3.0.10's `DEFAULT_MAX_NODES`.
pub const MAX_NODES: usize = 10_000;
/// asn1js 3.0.10's `DEFAULT_MAX_CONTENT_LENGTH`.
const MAX_CONTENT_LENGTH: i64 = 16 * 1024 * 1024;

/// What `asn1js.fromBER(bytes)` does.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// An offset other than -1.
    Accepted,
    /// `offset: -1`.
    Refused,
    /// The read throws this error.
    Throws(Error),
}

/// `asn1js.fromBER(bytes)` with the default limits.
pub fn from_ber(bytes: &[u8]) -> Verdict {
    if bytes.is_empty() {
        return Verdict::Refused;
    }
    let mut walk = Walk { depth: 0, nodes: 0 };
    match walk.element(bytes, 0, len(bytes)) {
        Ok(Some(_)) => Verdict::Accepted,
        Ok(None) => Verdict::Refused,
        Err(error) => Verdict::Throws(error),
    }
}

/// The type `localFromBER` picks for an element.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    EndOfContent,
    OctetString,
    BitString,
    Other,
}

/// One element read: where the next one starts, and what `constructor.NAME`
/// and `valueBlock.unusedBits` the parent's checks look at.
struct Read {
    end: i64,
    kind: Kind,
    unused_bits: u8,
}

/// `Ok(None)` is asn1js's `offset: -1`; `Err` is a throw.
type Step = Result<Option<Read>, Error>;

/// How the value block of each universal type reads.
#[derive(Clone, Copy, PartialEq)]
enum Block {
    EndOfContent,
    /// `HexBlock`: Boolean, Integer, Enumerated, a primitive of no known
    /// type, and the strings whose `fromBuffer` cannot throw.
    Hex,
    BitString,
    OctetString,
    Null,
    /// OBJECT IDENTIFIER and RELATIVE-OID: a run of arcs.
    Arcs,
    GeneralizedTime,
    BmpString,
    UniversalString,
    Constructed,
}

fn len(bytes: &[u8]) -> i64 {
    bytes.len() as i64
}

/// `checkBufferParams`: bounds against the whole buffer, not the parent.
fn in_bounds(bytes: &[u8], offset: i64, length: i64) -> bool {
    !bytes.is_empty() && offset >= 0 && length >= 0 && len(bytes) - offset - length >= 0
}

fn slice(bytes: &[u8], offset: i64, length: i64) -> &[u8] {
    let start = (offset.max(0) as usize).min(bytes.len());
    let end = ((offset + length).max(0) as usize).clamp(start, bytes.len());
    &bytes[start..end]
}

struct Walk {
    depth: usize,
    nodes: usize,
}

impl Walk {
    /// `localFromBERWithChildContext`.
    fn child(&mut self, bytes: &[u8], offset: i64, length: i64) -> Step {
        if self.depth + 1 > MAX_DEPTH {
            return Ok(None);
        }
        self.depth += 1;
        let step = self.element(bytes, offset, length);
        self.depth -= 1;
        step
    }

    /// A speculative decode: its verdict and any throw are dropped, its
    /// counts stay.
    fn speculate(&mut self, contents: &[u8]) {
        if !contents.is_empty() {
            let _ = self.child(contents, 0, len(contents));
        }
    }

    /// `localFromBER`.
    fn element(&mut self, bytes: &[u8], offset: i64, mut length: i64) -> Step {
        if !in_bounds(bytes, offset, length) || length == 0 {
            return Ok(None);
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Ok(None);
        }

        // The identification block.
        let id = slice(bytes, offset, length);
        let first = id[0];
        let class = first >> 6;
        let constructed = first & 0x20 != 0;
        // `None` for a tag too long to decode (asn1js's hex-only, -1).
        let mut tag = Some(u64::from(first & 0x1f));
        let mut id_length = 1;
        if first & 0x1f == 0x1f {
            let mut count = 0;
            loop {
                let at = count + 1;
                if at >= id.len() {
                    return Ok(None);
                }
                count += 1;
                if id[at] & 0x80 == 0 {
                    break;
                }
            }
            id_length = count + 1;
            tag = (id_length <= 9)
                .then(|| id[1..=count].iter().fold(0u64, |n, b| (n << 7) | u64::from(b & 0x7f)));
        }
        if class == der::UNIVERSAL
            && constructed
            && tag.is_some_and(|t| {
                matches!(t, 1 | 2 | 5 | 6 | 9 | 13 | 14 | 23 | 24 | 31 | 32 | 33 | 34)
            })
        {
            return Ok(None);
        }
        let id_end = offset + id_length as i64;
        length -= id_length as i64;

        // The length block.
        if !in_bounds(bytes, id_end, length) || length == 0 {
            return Ok(None);
        }
        let lb = slice(bytes, id_end, length);
        if lb[0] == 0xff {
            return Ok(None);
        }
        let indefinite = lb[0] == 0x80;
        let (declared, length_length): (i64, i64) = if indefinite {
            (0, 1)
        } else if lb[0] & 0x80 == 0 {
            (i64::from(lb[0]), 1)
        } else {
            let count = usize::from(lb[0] & 0x7f);
            if count > 8 || count + 1 > lb.len() {
                return Ok(None);
            }
            // Past 2^63 is past the content limit too.
            let n = lb[1..=count].iter().fold(0u64, |n, &b| (n << 8) | u64::from(b));
            (i64::try_from(n).unwrap_or(i64::MAX), count as i64 + 1)
        };
        let value_offset = id_end + length_length;
        length -= length_length;
        let value_length = if indefinite { length } else { declared };
        if value_length > MAX_CONTENT_LENGTH {
            return Ok(None);
        }
        if !constructed && indefinite {
            return Ok(None);
        }

        // The type, and its value block.
        let block = if class == der::UNIVERSAL {
            match tag {
                Some(t) if t >= 37 => return Ok(None),
                Some(0) => {
                    if constructed && declared > 0 {
                        return Ok(None);
                    }
                    Block::EndOfContent
                }
                Some(3) => Block::BitString,
                Some(4) => Block::OctetString,
                Some(5) => Block::Null,
                Some(6 | 13) => Block::Arcs,
                Some(15) => return Ok(None),
                Some(16 | 17) => Block::Constructed,
                Some(24) => Block::GeneralizedTime,
                Some(28) => Block::UniversalString,
                Some(30) => Block::BmpString,
                Some(1 | 2 | 10 | 12 | 14 | 18..=23 | 25..=27 | 29 | 31..=34) => Block::Hex,
                _ if constructed => Block::Constructed,
                _ => Block::Hex,
            }
        } else if constructed {
            Block::Constructed
        } else {
            Block::Hex
        };
        let kind = match (class, tag) {
            (der::UNIVERSAL, Some(0)) => Kind::EndOfContent,
            (der::UNIVERSAL, Some(3)) => Kind::BitString,
            (der::UNIVERSAL, Some(4)) => Kind::OctetString,
            _ => Kind::Other,
        };
        let (start, n) = (value_offset, value_length);
        let mut unused_bits = 0;
        let end = match block {
            Block::EndOfContent => Some(start),
            Block::Hex => hex(bytes, start, n),
            Block::Null => (start + n <= len(bytes)).then_some(start + n),
            Block::Arcs => arcs(bytes, start, n),
            Block::GeneralizedTime | Block::BmpString | Block::UniversalString => {
                let Some(end) = hex(bytes, start, n) else { return Ok(None) };
                let content = slice(bytes, start, end - start);
                match block {
                    Block::GeneralizedTime => {
                        if der::asn1js_generalized_time(content).is_none() {
                            return Err(Error::new("Wrong input string for conversion"));
                        }
                    }
                    Block::BmpString if !content.len().is_multiple_of(2) => {
                        return Err(Error::named(
                            "RangeError",
                            "Offset is outside the bounds of the DataView",
                        ));
                    }
                    Block::UniversalString if !content.len().is_multiple_of(4) => {
                        return Err(Error::named(
                            "RangeError",
                            "byte length of Uint32Array should be a multiple of 4",
                        ));
                    }
                    _ => {}
                }
                Some(end)
            }
            Block::Constructed => self.members(bytes, start, n, indefinite)?.map(|(end, _)| end),
            Block::OctetString => {
                if n == 0 {
                    Some(start)
                } else if constructed {
                    match self.members(bytes, start, n, indefinite)? {
                        Some((end, members))
                            if members.iter().all(|m| m.kind == Kind::OctetString) =>
                        {
                            Some(end)
                        }
                        _ => None,
                    }
                } else {
                    self.speculate(slice(bytes, start, n));
                    hex(bytes, start, n)
                }
            }
            Block::BitString => {
                if n == 0 {
                    Some(start)
                } else if constructed {
                    let Some((end, members)) = self.members(bytes, start, n, indefinite)? else {
                        return Ok(None);
                    };
                    for member in &members {
                        if member.kind != Kind::BitString
                            || (unused_bits > 0 && member.unused_bits > 0)
                        {
                            return Ok(None);
                        }
                        unused_bits = member.unused_bits;
                    }
                    Some(end)
                } else {
                    if !in_bounds(bytes, start, n) {
                        return Ok(None);
                    }
                    unused_bits = bytes[start as usize];
                    if unused_bits > 7 {
                        return Ok(None);
                    }
                    if unused_bits == 0 {
                        self.speculate(slice(bytes, start + 1, n - 1));
                    }
                    Some(start + n)
                }
            }
        };
        Ok(end.map(|end| Read { end, kind, unused_bits }))
    }

    /// `LocalConstructedValueBlock.fromBER`: the members, without the
    /// end-of-contents that closes an indefinite length.
    fn members(
        &mut self,
        bytes: &[u8],
        offset: i64,
        mut length: i64,
        indefinite: bool,
    ) -> Result<Option<(i64, Vec<Read>)>, Error> {
        if !in_bounds(bytes, offset, length) {
            return Ok(None);
        }
        let mut members = Vec::new();
        let mut at = offset;
        if length == 0 {
            return Ok(Some((at, members)));
        }
        while indefinite || length > 0 {
            let Some(member) = self.child(bytes, at, length)? else { return Ok(None) };
            length -= member.end - at;
            at = member.end;
            if indefinite && member.kind == Kind::EndOfContent {
                break;
            }
            members.push(member);
        }
        Ok(Some((at, members)))
    }
}

/// `HexBlock.fromBER`.
fn hex(bytes: &[u8], offset: i64, length: i64) -> Option<i64> {
    in_bounds(bytes, offset, length).then_some(offset + length)
}

/// `LocalObjectIdentifierValueBlock.fromBER`: each arc ends on an octet
/// without the continuation bit.
fn arcs(bytes: &[u8], offset: i64, mut length: i64) -> Option<i64> {
    let mut at = offset;
    while length > 0 {
        if !in_bounds(bytes, at, length) {
            return None;
        }
        let arc = slice(bytes, at, length);
        let used = arc.iter().position(|b| b & 0x80 == 0).map_or(arc.len(), |i| i + 1);
        if arc[used - 1] & 0x80 != 0 {
            return None;
        }
        at += used as i64;
        length -= used as i64;
    }
    Some(at)
}
