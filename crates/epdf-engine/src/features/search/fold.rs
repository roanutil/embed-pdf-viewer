//! The normalization both sides of a literal search go through.
//!
//! G5. A transcription of `core/src/search/fold.ts`, which stays in
//! TypeScript as well and is pinned to this by
//! `tooling/engine-conformance/vectors/search/fold.json`.
//!
//! Folding is what makes "Café" find "cafe" and a line-wrapped
//! "hello\n  world" find "hello world". The page text and the needle go
//! through the same fold, so the comparison is between two normalized forms
//! rather than between a normalized one and a raw one.
//!
//! Fold version 1, in the order the steps apply:
//!
//! 1. A whitespace run collapses to a single space.
//! 2. Each code point is NFKD-decomposed, so a ligature splits and a
//!    superscript becomes its digit.
//! 3. Combining marks are stripped, unless marks are being kept.
//! 4. Case folds through an upper-then-lower round trip, unless case is being
//!    kept. The round trip is what catches the pairs a plain lowercase
//!    misses.
//!
//! Every folded code unit remembers which original code point produced it, so
//! a match found in folded space translates back to an exact range in the
//! original. The whole anchor stage rests on that map.

use unicode_normalization::UnicodeNormalization;

use crate::features::text::TextRange;

/// Bump on any semantic change, so a stored pre-folded corpus is rebuilt
/// rather than silently compared against differently folded needles.
pub const SEARCH_FOLD_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoldOptions {
    /// Preserve case. The `matchCase` query flag.
    pub keep_case: bool,
    /// Preserve combining marks. The `matchDiacritics` query flag.
    pub keep_marks: bool,
    /// Drop whitespace instead of collapsing it to one space. The
    /// `ignoreWhitespace` query flag.
    pub drop_whitespace: bool,
}

/// Folded text, and the way back to the original.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldedText {
    pub folded: String,
    /// For each folded UTF-16 code unit, the UTF-16 index of the original code
    /// point it came from.
    ///
    /// UTF-16 on both sides because that is the space the wire and JavaScript
    /// use. A Rust caller working in bytes has to convert, and
    /// [`to_original_range`] is where that is done once.
    pub map: Vec<u32>,
    pub original: String,
}

/// Whether a character is whitespace by the same rule JavaScript's `\s` uses.
///
/// Not `char::is_whitespace`, which follows `White_Space` and excludes
/// U+FEFF while JavaScript's `\s` includes it. The two disagree on exactly
/// that character, and a zero-width no-break space in a PDF is common enough
/// to matter.
fn is_search_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

/// Whether a character is a combining mark, Unicode general category M.
/// Whether a character is a combining mark, Unicode general category M.
///
/// Shared with [`super::literal`], whose diacritic-sensitive rule needs the
/// same answer the fold used when it decided whether to strip.
pub(super) fn is_combining_mark(c: char) -> bool {
    is_mark(c)
}

fn is_mark(c: char) -> bool {
    // The categories `\p{M}` covers: Mn, Mc, Me. Checked through a small
    // table rather than by pulling in a general-category crate for one
    // predicate.
    matches!(u32::from(c),
        0x0300..=0x036F | 0x0483..=0x0489 | 0x0591..=0x05BD | 0x05BF | 0x05C1..=0x05C2
        | 0x05C4..=0x05C5 | 0x05C7 | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670
        | 0x06D6..=0x06DC | 0x06DF..=0x06E4 | 0x06E7..=0x06E8 | 0x06EA..=0x06ED
        | 0x0711 | 0x0730..=0x074A | 0x07A6..=0x07B0 | 0x07EB..=0x07F3
        | 0x0816..=0x0819 | 0x081B..=0x0823 | 0x0825..=0x0827 | 0x0829..=0x082D
        | 0x0859..=0x085B | 0x08E3..=0x0903 | 0x093A..=0x093C | 0x093E..=0x094F
        | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x0981..=0x0983 | 0x09BC
        | 0x09BE..=0x09C4 | 0x09C7..=0x09C8 | 0x09CB..=0x09CD | 0x09D7
        | 0x09E2..=0x09E3 | 0x0A01..=0x0A03 | 0x0A3C | 0x0A3E..=0x0A42
        | 0x0A47..=0x0A48 | 0x0A4B..=0x0A4D | 0x0A51 | 0x0A70..=0x0A71 | 0x0A75
        | 0x0A81..=0x0A83 | 0x0ABC | 0x0ABE..=0x0AC5 | 0x0AC7..=0x0AC9
        | 0x0ACB..=0x0ACD | 0x0AE2..=0x0AE3 | 0x0B01..=0x0B03 | 0x0B3C
        | 0x0B3E..=0x0B44 | 0x0B47..=0x0B48 | 0x0B4B..=0x0B4D | 0x0B56..=0x0B57
        | 0x0B62..=0x0B63 | 0x0B82 | 0x0BBE..=0x0BC2 | 0x0BC6..=0x0BC8
        | 0x0BCA..=0x0BCD | 0x0BD7 | 0x0C00..=0x0C03 | 0x0C3E..=0x0C44
        | 0x0C46..=0x0C48 | 0x0C4A..=0x0C4D | 0x0C55..=0x0C56 | 0x0C62..=0x0C63
        | 0x0C81..=0x0C83 | 0x0CBC | 0x0CBE..=0x0CC4 | 0x0CC6..=0x0CC8
        | 0x0CCA..=0x0CCD | 0x0CD5..=0x0CD6 | 0x0CE2..=0x0CE3 | 0x0D01..=0x0D03
        | 0x0D3E..=0x0D44 | 0x0D46..=0x0D48 | 0x0D4A..=0x0D4D | 0x0D57
        | 0x0D62..=0x0D63 | 0x0D82..=0x0D83 | 0x0DCA | 0x0DCF..=0x0DD4 | 0x0DD6
        | 0x0DD8..=0x0DDF | 0x0DF2..=0x0DF3 | 0x0E31 | 0x0E34..=0x0E3A
        | 0x0E47..=0x0E4E | 0x0EB1 | 0x0EB4..=0x0EB9 | 0x0EBB..=0x0EBC
        | 0x0EC8..=0x0ECD | 0x0F18..=0x0F19 | 0x0F35 | 0x0F37 | 0x0F39
        | 0x0F3E..=0x0F3F | 0x0F71..=0x0F84 | 0x0F86..=0x0F87 | 0x0F8D..=0x0F97
        | 0x0F99..=0x0FBC | 0x0FC6 | 0x102B..=0x103E | 0x1056..=0x1059
        | 0x105E..=0x1060 | 0x1062..=0x1064 | 0x1067..=0x106D | 0x1071..=0x1074
        | 0x1082..=0x108D | 0x108F | 0x109A..=0x109D | 0x135D..=0x135F
        | 0x1712..=0x1714 | 0x1732..=0x1734 | 0x1752..=0x1753 | 0x1772..=0x1773
        | 0x17B4..=0x17D3 | 0x17DD | 0x180B..=0x180D | 0x18A9 | 0x1920..=0x192B
        | 0x1930..=0x193B | 0x1A17..=0x1A1B | 0x1A55..=0x1A5E | 0x1A60..=0x1A7C
        | 0x1A7F | 0x1AB0..=0x1ABE | 0x1B00..=0x1B04 | 0x1B34..=0x1B44
        | 0x1B6B..=0x1B73 | 0x1B80..=0x1B82 | 0x1BA1..=0x1BAD | 0x1BE6..=0x1BF3
        | 0x1C24..=0x1C37 | 0x1CD0..=0x1CD2 | 0x1CD4..=0x1CE8 | 0x1CED
        | 0x1CF2..=0x1CF4 | 0x1CF8..=0x1CF9 | 0x1DC0..=0x1DF5 | 0x1DFC..=0x1DFF
        | 0x20D0..=0x20F0 | 0x2CEF..=0x2CF1 | 0x2D7F | 0x2DE0..=0x2DFF
        | 0x302A..=0x302F | 0x3099..=0x309A | 0xA66F..=0xA672 | 0xA674..=0xA67D
        | 0xA69E..=0xA69F | 0xA6F0..=0xA6F1 | 0xA802 | 0xA806 | 0xA80B
        | 0xA823..=0xA827 | 0xA880..=0xA881 | 0xA8B4..=0xA8C4 | 0xA8E0..=0xA8F1
        | 0xA926..=0xA92D | 0xA947..=0xA953 | 0xA980..=0xA983 | 0xA9B3..=0xA9C0
        | 0xA9E5 | 0xAA29..=0xAA36 | 0xAA43 | 0xAA4C..=0xAA4D | 0xAA7B..=0xAA7D
        | 0xAAB0 | 0xAAB2..=0xAAB4 | 0xAAB7..=0xAAB8 | 0xAABE..=0xAABF | 0xAAC1
        | 0xAAEB..=0xAAEF | 0xAAF5..=0xAAF6 | 0xABE3..=0xABEA | 0xABEC..=0xABED
        | 0xFB1E | 0xFE00..=0xFE0F | 0xFE20..=0xFE2F
        | 0x101FD | 0x102E0 | 0x10376..=0x1037A | 0x10A01..=0x10A03
        | 0x10A05..=0x10A06 | 0x10A0C..=0x10A0F | 0x10A38..=0x10A3A | 0x10A3F
        | 0x10AE5..=0x10AE6 | 0x11000..=0x11002 | 0x11038..=0x11046
        | 0x1107F..=0x11082 | 0x110B0..=0x110BA | 0x11100..=0x11102
        | 0x11127..=0x11134 | 0x11173 | 0x11180..=0x11182 | 0x111B3..=0x111C0
        | 0x1122C..=0x11237 | 0x112DF..=0x112EA | 0x11300..=0x11303 | 0x1133C
        | 0x1133E..=0x11344 | 0x11347..=0x11348 | 0x1134B..=0x1134D | 0x11357
        | 0x11362..=0x11363 | 0x11366..=0x1136C | 0x11370..=0x11374
        | 0x114B0..=0x114C3 | 0x115AF..=0x115B5 | 0x115B8..=0x115C0
        | 0x115DC..=0x115DD | 0x11630..=0x11640 | 0x116AB..=0x116B7
        | 0x1171D..=0x1172B | 0x16AF0..=0x16AF4 | 0x16B30..=0x16B36 | 0x16F51..=0x16F7E
        | 0x16F8F..=0x16F92 | 0x1BC9D..=0x1BC9E | 0x1D165..=0x1D169
        | 0x1D16D..=0x1D172 | 0x1D17B..=0x1D182 | 0x1D185..=0x1D18B
        | 0x1D1AA..=0x1D1AD | 0x1D242..=0x1D244 | 0x1DA00..=0x1DA36
        | 0x1DA3B..=0x1DA6C | 0x1DA75 | 0x1DA84 | 0x1DA9B..=0x1DA9F
        | 0x1DAA1..=0x1DAAF | 0x1E8D0..=0x1E8D6 | 0xE0100..=0xE01EF)
}

/// Fold a string, keeping the way back.
pub fn fold_text(original: &str, options: FoldOptions) -> FoldedText {
    let mut folded = String::new();
    let mut map: Vec<u32> = Vec::new();
    let mut last_was_space = false;
    // The UTF-16 index of the code point being folded, which is what the map
    // records.
    let mut index: u32 = 0;

    for cp in original.chars() {
        let piece: String = if is_search_whitespace(cp) {
            " ".to_string()
        } else {
            let decomposed: String = cp.nfkd().collect();
            let stripped: String = if options.keep_marks {
                decomposed
            } else {
                decomposed.chars().filter(|c| !is_mark(*c)).collect()
            };
            if options.keep_case {
                stripped
            } else {
                // Upper then lower, not lower alone. The round trip is what
                // folds the pairs whose lowercase is not their fold: the sharp
                // s becomes two letters, and final sigma becomes sigma.
                stripped.to_uppercase().to_lowercase()
            }
        };

        // A decomposition can contain whitespace of its own, so runs collapse
        // (or drop) at the produced-unit level rather than only at the source
        // one.
        for unit in piece.chars() {
            if is_search_whitespace(unit) {
                if options.drop_whitespace || last_was_space {
                    continue;
                }
                folded.push(' ');
                map.push(index);
                last_was_space = true;
            } else {
                folded.push(unit);
                // One map entry per UTF-16 code unit, so a supplementary
                // character produced by a decomposition contributes two and a
                // BMP one contributes exactly one. The map is indexed by
                // folded code unit, not by folded character.
                for _ in 0..unit.len_utf16() {
                    map.push(index);
                }
                last_was_space = false;
            }
        }
        index += cp.len_utf16() as u32;
    }

    FoldedText { folded, map, original: original.to_string() }
}

/// Translate a match found in folded space back to a range of the original
/// text, in UTF-16 code units (`toOriginalRange`).
///
/// The range covers whole original code points: a hit on either half of a
/// ligature spans the whole ligature, and a hit ending on a collapsed space
/// extends only through the first whitespace character of the run, because the
/// rest is presentation rather than match.
pub fn to_original_range(
    folded: &FoldedText,
    folded_start: usize,
    folded_length: usize,
) -> Option<TextRange> {
    if folded_length == 0 {
        return None;
    }
    let start = *folded.map.get(folded_start)?;
    let last_original = *folded.map.get(folded_start + folded_length - 1)?;
    let end = last_original + code_point_len_utf16_at(&folded.original, last_original);
    Some(TextRange { start, count: end - start })
}

/// The UTF-16 length of the code point starting at a UTF-16 index.
fn code_point_len_utf16_at(text: &str, utf16_index: u32) -> u32 {
    let mut units = 0u32;
    for ch in text.chars() {
        if units == utf16_index {
            return ch.len_utf16() as u32;
        }
        units += ch.len_utf16() as u32;
        if units > utf16_index {
            // The index landed inside a surrogate pair, which the map never
            // produces; report the pair's length so a caller still gets a
            // whole character.
            return ch.len_utf16() as u32;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    //! `toOriginalRange` in `packages/engine/core/test/search/fold.test.ts`.

    use super::*;

    fn original(text: &str, start: usize, length: usize) -> Option<TextRange> {
        to_original_range(&fold_text(text, FoldOptions::default()), start, length)
    }

    #[test]
    fn maps_a_folded_range_back_to_original_code_units() {
        assert_eq!(original("Hello World", 6, 5), Some(TextRange { start: 6, count: 5 }));
    }

    #[test]
    fn a_hit_inside_a_ligature_covers_the_whole_original_char() {
        assert_eq!(original("\u{FB01}le", 0, 2), Some(TextRange { start: 0, count: 1 }));
        assert_eq!(original("\u{FB01}le", 0, 4), Some(TextRange { start: 0, count: 3 }));
    }

    #[test]
    fn a_range_across_collapsed_whitespace_spans_the_raw_whitespace() {
        assert_eq!(original("hello\n   world", 0, 11), Some(TextRange { start: 0, count: 14 }));
    }

    #[test]
    fn never_splits_a_surrogate_pair() {
        assert_eq!(original("\u{1F600}", 0, 1), Some(TextRange { start: 0, count: 2 }));
    }
}
