//! `graphemes.ts`: characters as the engine's text layout cuts them
//! (`GraphemeStarts`, `IsInvisibleGrapheme`). A grapheme is a character with
//! the marks, variation selectors and joined characters that follow it.
//! Positions are UTF-16 code units, as in the TypeScript; a surrogate pair
//! stays whole.

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

const ZERO_WIDTH_JOINER: u32 = 0x200d;

/// How many code units `code_point` takes.
pub(crate) fn units_of(code_point: u32) -> usize {
    if code_point > 0xffff { 2 } else { 1 }
}

/// `String.prototype.codePointAt`: the code point at `at`, a surrogate pair
/// as one, a lone surrogate as itself.
pub(crate) fn code_point_at(text: &[u16], at: usize) -> u32 {
    let unit = u32::from(text[at]);
    if (0xd800..=0xdbff).contains(&unit)
        && let Some(&low) = text.get(at + 1)
        && (0xdc00..=0xdfff).contains(&low)
    {
        return 0x10000 + ((unit - 0xd800) << 10) + (u32::from(low) - 0xdc00);
    }
    unit
}

/// `/\p{M}/u`: a mark.
pub fn is_mark(code_point: u32) -> bool {
    char::from_u32(code_point)
        .is_some_and(|c| c.general_category_group() == GeneralCategoryGroup::Mark)
}

fn is_variation_selector(code_point: u32) -> bool {
    (0xfe00..=0xfe0f).contains(&code_point) || (0xe0100..=0xe01ef).contains(&code_point)
}

/// The code points of `text`, a surrogate pair as one.
pub fn code_points_of(text: &[u16]) -> Vec<u32> {
    let mut code_points = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let code_point = code_point_at(text, at);
        code_points.push(code_point);
        at += units_of(code_point);
    }
    code_points
}

/// Where each grapheme of `text` starts, per code unit: a mark, a variation
/// selector, a joiner and the character after a joiner continue the grapheme
/// before them. A surrogate pair's second unit never starts one.
pub fn grapheme_starts(text: &[u16]) -> Vec<bool> {
    let mut starts = vec![false; text.len()];
    let mut after_joiner = false;
    let mut at = 0;
    while at < text.len() {
        let code_point = code_point_at(text, at);
        let continues = at > 0
            && (is_mark(code_point)
                || is_variation_selector(code_point)
                || code_point == ZERO_WIDTH_JOINER
                || after_joiner);
        starts[at] = !continues;
        after_joiner = code_point == ZERO_WIDTH_JOINER;
        at += units_of(code_point);
    }
    starts
}

/// The graphemes of `text`, as `[begin, end)` code unit ranges.
pub fn graphemes_of(text: &[u16]) -> Vec<(usize, usize)> {
    let starts = grapheme_starts(text);
    let mut graphemes = Vec::new();
    let mut begin = 0;
    while begin < text.len() {
        let mut end = begin + 1;
        while end < text.len() && !starts[end] {
            end += 1;
        }
        graphemes.push((begin, end));
        begin = end;
    }
    graphemes
}

/// Whether a font needs a glyph for `code_point` to cover it: joiners and
/// variation selectors draw nothing of their own.
pub(crate) fn needs_glyph(code_point: u32) -> bool {
    code_point != ZERO_WIDTH_JOINER && !is_variation_selector(code_point)
}

/// A grapheme that draws nothing: controls, format characters, fillers.
pub fn is_invisible_grapheme(grapheme: &[u16]) -> bool {
    code_points_of(grapheme).into_iter().all(|cp| {
        cp < 0x20
            || (0x7f..=0x9f).contains(&cp)
            || cp == 0x00ad
            || cp == 0x034f
            || cp == 0x061c
            || (0x115f..=0x1160).contains(&cp)
            || (0x17b4..=0x17b5).contains(&cp)
            || (0x180b..=0x180f).contains(&cp)
            || (0x200b..=0x200f).contains(&cp)
            || (0x202a..=0x202e).contains(&cp)
            || (0x2060..=0x206f).contains(&cp)
            || cp == 0x3164
            || (0xfe00..=0xfe0f).contains(&cp)
            || cp == 0xfeff
            || cp == 0xffa0
            || (0xfff0..=0xfff8).contains(&cp)
            || (0x1bca0..=0x1bca3).contains(&cp)
            || (0x1d173..=0x1d17a).contains(&cp)
            || (0xe0000..=0xe0fff).contains(&cp)
    })
}
