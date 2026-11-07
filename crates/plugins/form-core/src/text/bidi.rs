//! `bidi.ts`: the Unicode bidirectional algorithm (UAX #9) as the engine runs
//! it with ICU (`CFX_BidiParagraph`). A paragraph's levels at a given
//! direction, the levels of one of its lines (rule L1) and the order its runs
//! are drawn in (rule L2).
//!
//! The TypeScript takes its levels from `bidi-js`; this takes them from
//! `unicode-bidi`. Both pass Unicode's conformance tests, which leave the
//! levels of the characters rule X9 removes open, so the two places `bidi-js`
//! decides those are done here as it does them: a removed character takes the
//! level of the code unit before it (the paragraph's at the text's start),
//! and rule L1 resets each segment or paragraph separator, and the
//! whitespace, isolate and removed characters before it or the paragraph's
//! end, to the paragraph's level.

use unicode_bidi::{BidiClass, Level, bidi_class};

use super::graphemes::{code_point_at, units_of};

/// A code point's bidi class; a lone surrogate reads as `L`, as `bidi-js`
/// reads one.
pub(crate) fn class_of(code_point: u32) -> BidiClass {
    char::from_u32(code_point).map_or(BidiClass::L, bidi_class)
}

/// `BN_LIKE_TYPES`: the classes rule X9 removes.
fn is_bn_like(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, BN | RLE | LRE | RLO | LRO | PDF)
}

/// `bidi-js`'s `TRAILING_TYPES`: what L1 resets before a separator or a
/// paragraph's end.
fn is_bidi_js_trailing(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, S | WS | B | LRI | RLI | FSI | PDI) || is_bn_like(class)
}

/// `TRAILING`: classes that take the paragraph's level at the end of a line
/// (rule L1).
fn is_trailing(class: BidiClass) -> bool {
    use BidiClass::*;
    matches!(class, WS | S | B | BN | LRE | LRO | RLE | RLO | PDF | LRI | RLI | FSI | PDI)
}

/// Each code unit's level in a paragraph read left to right (level 0) or
/// right to left (level 1). Whitespace ending the paragraph is at its level,
/// as ICU gives it.
pub fn paragraph_levels(text: &[u16], rtl: bool) -> Vec<u8> {
    let base = u8::from(rtl);
    if text.is_empty() {
        return Vec::new();
    }
    let level = if rtl { Level::rtl() } else { Level::ltr() };
    let info = unicode_bidi::utf16::BidiInfo::new(text, Some(level));
    let classes = &info.original_classes;
    let mut levels: Vec<u8> = info.levels.iter().map(|level| level.number()).collect();
    for paragraph in &info.paragraphs {
        let para_level = paragraph.level.number();
        let end = paragraph.range.end - 1;
        for i in paragraph.range.clone() {
            if is_bn_like(classes[i]) {
                levels[i] = if i == 0 { para_level } else { levels[i - 1] };
            }
            if i == end || matches!(classes[i], BidiClass::S | BidiClass::B) {
                let mut j = i as isize;
                while j >= 0 && is_bidi_js_trailing(classes[j as usize]) {
                    levels[j as usize] = para_level;
                    j -= 1;
                }
            }
        }
    }
    let length = text.len();
    trailing_at_level(text, levels, 0, length, base)
}

/// The levels of the line `[begin, end)` of a paragraph at `base`:
/// whitespace ending it at the paragraph's (L1).
pub(crate) fn line_levels(
    text: &[u16],
    levels: &[u8],
    begin: usize,
    end: usize,
    base: u8,
) -> Vec<u8> {
    trailing_at_level(text, levels[begin..end].to_vec(), begin, end, base)
}

fn trailing_at_level(
    text: &[u16],
    mut levels: Vec<u8>,
    begin: usize,
    end: usize,
    base: u8,
) -> Vec<u8> {
    let mut at = end;
    while at > begin {
        let low = text[at - 1];
        let start =
            if (0xdc00..=0xdfff).contains(&low) && at >= begin + 2 { at - 2 } else { at - 1 };
        if !is_trailing(class_of(code_point_at(text, start))) {
            break;
        }
        for unit in start..at {
            levels[unit - begin] = base;
        }
        at = start;
    }
    levels
}

/// Whether `text` reads right to left by its first strong character, outside
/// isolates; `None` when it has none.
pub fn first_strong_is_rtl(text: &[u16]) -> Option<bool> {
    use BidiClass::*;
    let mut isolates = 0;
    let mut at = 0;
    while at < text.len() {
        let code_point = code_point_at(text, at);
        match class_of(code_point) {
            L if isolates == 0 => return Some(false),
            R | AL if isolates == 0 => return Some(true),
            LRI | RLI | FSI => isolates += 1,
            PDI if isolates > 0 => isolates -= 1,
            _ => {}
        }
        at += units_of(code_point);
    }
    None
}

/// The order runs at `levels` are drawn in, left to right, as indices into
/// `levels` (rule L2): from the highest level down to the lowest odd one,
/// every stretch at that level or above is reversed.
pub fn visual_order(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    if levels.is_empty() {
        return order;
    }
    let highest = *levels.iter().max().expect("levels");
    let lowest_odd = levels
        .iter()
        .map(|&level| if level % 2 == 1 { level } else { level + 1 })
        .min()
        .expect("levels");
    let mut level = i32::from(highest);
    while level >= i32::from(lowest_odd) {
        let mut start = 0;
        while start < order.len() {
            if i32::from(levels[order[start]]) < level {
                start += 1;
                continue;
            }
            let mut end = start + 1;
            while end < order.len() && i32::from(levels[order[end]]) >= level {
                end += 1;
            }
            order[start..end].reverse();
            start = end;
        }
        level -= 1;
    }
    order
}

/// The character a right-to-left run draws for `code_point`: its mirror, such
/// as `)` for `(`.
pub fn mirror_of(code_point: u32) -> u32 {
    char::from_u32(code_point)
        .and_then(unicode_bidi_mirroring::get_mirrored)
        .map_or(code_point, u32::from)
}
