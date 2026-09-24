//! Where a page's two index spaces meet.
//!
//! G5. A transcription of `core/src/text/charmap.ts`, which stays in
//! TypeScript as well: web selection plugins consume it, so this is a
//! deliberate duplicate pinned by shared vectors rather than a port.
//!
//! A page has two index spaces and they are not the same size.
//!
//! - CHARACTER space is PDFium's internal character list. Geometry runs tile
//!   it, hit tests address it, selections live in it. Its size is
//!   [`PageTextSnapshot::char_count`].
//! - TEXT space is UTF-16 code-unit offsets into the extracted string, which
//!   is what search and slicing work on.
//!
//! They diverge in exactly two ways: a non-printing character occupies a
//! character slot and contributes zero text units, and a supplementary-plane
//! character contributes two. The anchors encode those and nothing else, so an
//! absent map means the spaces are identical, which is the common page and
//! costs nothing.
//!
//! # UTF-16 in a UTF-8 language
//!
//! Every offset here is a UTF-16 code unit, because that is what the wire
//! carries and what JavaScript produces. Rust strings are UTF-8, so slicing by
//! one of these offsets means converting first, and [`slice_text_by_chars`] is
//! the only place that conversion happens. A caller that indexes `text` with a
//! text offset directly is wrong in a way that only shows up on non-ASCII
//! pages.

use serde::{Deserialize, Serialize};

/// One anchor: from character `char` onward, until the next anchor, boundary
/// `c` maps to text offset `text + (c - char)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharMapAnchor {
    pub char: u32,
    pub text: u32,
}

impl CharMapAnchor {
    pub fn new(char: u32, text: u32) -> Self {
        Self { char, text }
    }
}

/// Which way to resolve a text offset sitting on a zero-width plateau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharBoundaryBias {
    /// The greatest boundary at or before the offset. Scans forward past
    /// zero-width characters, so a range start never includes a dropped
    /// character that precedes its first real unit.
    Start,
    /// The smallest boundary at or after it. Stops before zero-width
    /// characters, so a range end never swallows a dropped one that follows.
    End,
}

/// A page's extracted text and the map between its index spaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageTextSnapshot {
    /// The full page text in display order, UTF-16 faithful.
    pub text: String,
    /// The size of CHARACTER space, which is not the length of `text`.
    pub char_count: u32,
    /// Absent or empty means the two spaces are identical.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub char_map: Vec<CharMapAnchor>,
}

impl PageTextSnapshot {
    /// The length of `text` in UTF-16 code units.
    ///
    /// Not `text.len()`, which is bytes, and not `chars().count()`, which is
    /// scalar values. A supplementary character is one scalar, four bytes and
    /// two code units, and only the last is what TEXT space counts.
    pub fn text_len_utf16(&self) -> u32 {
        self.text.chars().map(char::len_utf16).sum::<usize>() as u32
    }

    /// The byte offset of a UTF-16 text offset, for slicing.
    ///
    /// An offset landing inside a surrogate pair has no byte offset of its
    /// own, and this reports the start of the character it fell into. Callers
    /// reach this through [`slice_text_by_chars`], which only ever asks for
    /// boundaries that are whole.
    fn byte_offset(&self, text_offset: u32) -> usize {
        let mut units = 0u32;
        for (byte, ch) in self.text.char_indices() {
            if units >= text_offset {
                return byte;
            }
            units += ch.len_utf16() as u32;
        }
        self.text.len()
    }
}

/// The greatest anchor at or before `boundary`, with an implicit head.
fn anchor_at_or_before(map: &[CharMapAnchor], boundary: u32) -> CharMapAnchor {
    // Binary search rather than a scan: a page of CJK text has an anchor per
    // character, and this runs once per boundary conversion.
    let mut lo = 0usize;
    let mut hi = map.len();
    let mut best = CharMapAnchor::new(0, 0);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if map[mid].char <= boundary {
            best = map[mid];
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    best
}

/// The text offset of a character boundary, clamped into range.
///
/// For a dropped character this is the offset of the next real text unit,
/// which is exactly what half-open slicing needs at either end.
pub fn boundary_text_offset(snapshot: &PageTextSnapshot, boundary: u32) -> u32 {
    let b = boundary.min(snapshot.char_count);
    if snapshot.char_map.is_empty() {
        return b.min(snapshot.text_len_utf16());
    }
    let anchor = anchor_at_or_before(&snapshot.char_map, b);
    anchor.text + (b - anchor.char)
}

/// The character boundary for a text offset, under an explicit bias.
///
/// An offset inside a surrogate pair resolves outward under both biases, so a
/// clipped supplementary character is always covered whole.
pub fn char_boundary_at_text_offset(
    snapshot: &PageTextSnapshot,
    offset: u32,
    bias: CharBoundaryBias,
) -> u32 {
    let target = offset.min(snapshot.text_len_utf16());
    let mut lo = 0u32;
    let mut hi = snapshot.char_count;

    match bias {
        CharBoundaryBias::Start => {
            // The greatest boundary whose text offset is at most the target.
            // Boundary zero always qualifies.
            while lo < hi {
                let mid = lo + (hi - lo).div_ceil(2);
                if boundary_text_offset(snapshot, mid) <= target {
                    lo = mid;
                } else {
                    hi = mid - 1;
                }
            }
            lo
        }
        CharBoundaryBias::End => {
            // The smallest boundary whose text offset is at least the target.
            // The final boundary always qualifies, since the offset is clamped.
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if boundary_text_offset(snapshot, mid) >= target {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            lo
        }
    }
}

/// A half-open character range, in CHARACTER space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharRange {
    pub start: u32,
    pub end: u32,
}

/// The character range whose text projection is exactly the given text slice.
///
/// The inverse used by search-hit conversion and select-by-text. Start is
/// right-biased and end left-biased, so a zero-width character adjacent to the
/// range falls outside it on both sides.
///
/// A non-empty text range cannot invert: `end > start` forces the boundaries
/// apart by monotonicity. An empty one normalizes to the end-rule boundary of
/// its start on both sides.
pub fn char_range_for_text_offsets(
    snapshot: &PageTextSnapshot,
    start_offset: u32,
    end_offset: u32,
) -> CharRange {
    if end_offset <= start_offset {
        let b = char_boundary_at_text_offset(snapshot, start_offset, CharBoundaryBias::End);
        return CharRange { start: b, end: b };
    }
    CharRange {
        start: char_boundary_at_text_offset(snapshot, start_offset, CharBoundaryBias::Start),
        end: char_boundary_at_text_offset(snapshot, end_offset, CharBoundaryBias::End),
    }
}

/// The text projection of a half-open character range. The copy primitive.
///
/// Dropped characters inside the range contribute nothing; a supplementary
/// character contributes its whole surrogate pair. Slicing the full range
/// always returns the whole text.
pub fn slice_text_by_chars(snapshot: &PageTextSnapshot, char_start: u32, char_end: u32) -> String {
    if char_end <= char_start {
        return String::new();
    }
    let start = boundary_text_offset(snapshot, char_start);
    let end = boundary_text_offset(snapshot, char_end);
    let (start_byte, end_byte) = (snapshot.byte_offset(start), snapshot.byte_offset(end));
    snapshot.text[start_byte..end_byte].to_string()
}

/// The first invariant a map violates, or `None` when it is sound.
///
/// Shared by the wire schema and the engine's own builders. A malformed map is
/// rejected rather than repaired, because a map that slices wrong produces
/// plausible text from the wrong characters, which nothing downstream can
/// detect.
///
/// The invariants, in the order they are checked:
///
/// 1. An absent map means identity, so the two spaces must be the same size.
/// 2. Anchor characters strictly increase, each within the page.
/// 3. Against the previous anchor, the character just before each anchor
///    contributed exactly zero units or exactly two.
/// 4. The map projects the final boundary onto the text's length.
pub fn char_map_violation(
    char_count: u32,
    text_len_utf16: u32,
    char_map: &[CharMapAnchor],
) -> Option<String> {
    if char_map.is_empty() {
        return (char_count != text_len_utf16).then(|| {
            format!(
                "identity map requires charCount ({char_count}) === text length ({text_len_utf16})"
            )
        });
    }

    let mut prev_char = 0u32;
    let mut prev_text = 0u32;
    for (index, anchor) in char_map.iter().enumerate() {
        if anchor.char <= prev_char {
            return Some(format!(
                "anchor {index} char {} not strictly after {prev_char}",
                anchor.char
            ));
        }
        if anchor.char > char_count {
            return Some(format!("anchor {index} char {} outside [1, {char_count}]", anchor.char));
        }
        // The step is how many units the character before this anchor
        // contributed. Signed, because a malformed map can claim a negative.
        let expected = i64::from(prev_text) + i64::from(anchor.char - prev_char - 1);
        let step = i64::from(anchor.text) - expected;
        if step != 0 && step != 2 {
            return Some(format!(
                "anchor {index} implies a character contributing {step} text units (expected 0 or 2)"
            ));
        }
        prev_char = anchor.char;
        prev_text = anchor.text;
    }

    let tail = prev_text + (char_count - prev_char);
    (tail != text_len_utf16)
        .then(|| format!("map projects charCount to {tail}, but text length is {text_len_utf16}"))
}
