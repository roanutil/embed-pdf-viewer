//! `items.ts`: a paragraph cut into items as the engine cuts it (`Itemize`):
//! stretches of one font, one bidi level and one script, each measured as the
//! engine shapes it.

use std::rc::Rc;

use epdf_geometry::js;

use crate::bidi::{line_levels, paragraph_levels, visual_order};
use crate::fonts::{PaintFont, PaintFonts, match_grapheme};
use crate::fround;
use crate::graphemes::{code_point_at, code_points_of, graphemes_of, is_invisible_grapheme};
use crate::scripts::{is_common_script, is_simple_script, script_of};

#[derive(Clone, Debug)]
pub(crate) struct Item {
    /// Code units of the paragraph's text.
    pub begin: usize,
    pub end: usize,
    pub font: Rc<PaintFont>,
    pub level: u8,
    pub script: &'static str,
}

/// A grapheme of an itemized paragraph, with the widths of the glyphs it
/// draws.
#[derive(Clone, Debug)]
pub(crate) struct Grapheme {
    pub begin: usize,
    pub end: usize,
    /// Its item's index.
    pub item: usize,
    /// In thousandths of the font size, one per glyph as the engine adds them
    /// up.
    pub widths: Vec<f64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Paragraph {
    pub text: Vec<u16>,
    /// 0 for a paragraph read left to right, 1 right to left.
    pub base: u8,
    /// Per code unit.
    pub levels: Vec<u8>,
    pub items: Vec<Item>,
    pub graphemes: Vec<Grapheme>,
    /// Graphemes no font has, first seen first.
    pub missing: Vec<Vec<u16>>,
}

/// `/^\s+$/u`: every code point is JavaScript white space.
fn is_space_text(text: &[u16]) -> bool {
    !text.is_empty()
        && code_points_of(text).into_iter().all(|cp| {
            matches!(
                cp,
                0x09..=0x0d
                    | 0x20
                    | 0xa0
                    | 0x1680
                    | 0x2000..=0x200a
                    | 0x2028
                    | 0x2029
                    | 0x202f
                    | 0x205f
                    | 0x3000
                    | 0xfeff
            )
        })
}

/// The items of `text` as a paragraph read right to left when `rtl`, in
/// `fonts`.
pub(crate) fn itemize(text: &[u16], rtl: bool, fonts: &PaintFonts) -> Paragraph {
    let levels = paragraph_levels(text, rtl);
    let cuts = graphemes_of(text);
    let mut missing: Vec<Vec<u16>> = Vec::new();

    // Per grapheme: script, level, font. Characters of no script of their own
    // follow the one before them; leading ones, the one after.
    struct Piece {
        begin: usize,
        end: usize,
        script: &'static str,
        level: u8,
        font: Rc<PaintFont>,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut last_script = "Common";
    for &(begin, end) in &cuts {
        let grapheme = &text[begin..end];
        let mut script = script_of(code_point_at(text, begin));
        if is_common_script(script) {
            script = last_script;
        } else {
            last_script = script;
        }
        let found = match_grapheme(grapheme, fonts);
        if !found.covered
            && !is_invisible_grapheme(grapheme)
            && !missing.iter().any(|seen| seen == grapheme)
        {
            missing.push(grapheme.to_vec());
        }
        pieces.push(Piece { begin, end, script, level: levels[begin], font: found.font });
    }
    let mut index = pieces.len();
    while index > 1 {
        if is_common_script(pieces[index - 2].script) {
            pieces[index - 2].script = pieces[index - 1].script;
        }
        index -= 1;
    }

    let mut items: Vec<Item> = Vec::new();
    for piece in pieces {
        if let Some(last) = items.last_mut()
            && Rc::ptr_eq(&last.font, &piece.font)
            && last.level == piece.level
            && (last.script == piece.script || is_common_script(piece.script))
        {
            last.end = piece.end;
            continue;
        }
        items.push(Item {
            begin: piece.begin,
            end: piece.end,
            font: piece.font,
            level: piece.level,
            script: piece.script,
        });
    }

    let mut graphemes = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let in_item: Vec<(usize, usize)> = cuts
            .iter()
            .copied()
            .filter(|&(begin, _)| begin >= item.begin && begin < item.end)
            .collect();
        let widths = widths_of(text, item, &in_item);
        for (&(begin, end), widths) in in_item.iter().zip(widths) {
            graphemes.push(Grapheme { begin, end, item: index, widths });
        }
    }
    Paragraph { text: text.to_vec(), base: u8::from(rtl), levels, items, graphemes, missing }
}

/// Each grapheme's glyph widths in `item`. A standard font draws a glyph per
/// character from its advances. A simple script is drawn with kerning and
/// ligatures off, so each grapheme is as wide as its own glyph, which the
/// engine rounds to the thousandths its PDF widths hold. Any other script is
/// shaped: each word is measured whole (shaping starts again after a space),
/// each space alone, and a word's graphemes take what each adds to its
/// prefixes.
fn widths_of(text: &[u16], item: &Item, cuts: &[(usize, usize)]) -> Vec<Vec<f64>> {
    let rtl = item.level % 2 == 1;
    let font = &item.font;
    if font.standard {
        return cuts
            .iter()
            .map(|&(begin, end)| {
                code_points_of(&text[begin..end])
                    .into_iter()
                    .map(|cp| {
                        let mut units = [0u16; 2];
                        let one = match char::from_u32(cp) {
                            Some(c) => &*c.encode_utf16(&mut units),
                            None => {
                                units[0] = cp as u16;
                                &units[..1]
                            }
                        };
                        font.width(one, rtl)
                    })
                    .collect()
            })
            .collect();
    }
    if is_simple_script(item.script) {
        return cuts
            .iter()
            .map(|&(begin, end)| vec![js::round(font.width(&text[begin..end], rtl))])
            .collect();
    }
    let mut widths = Vec::new();
    let is_space_at = |at: usize| is_space_text(&text[cuts[at].0..cuts[at].1]);
    let mut at = 0;
    while at < cuts.len() {
        if is_space_at(at) {
            widths.push(vec![font.width(&text[cuts[at].0..cuts[at].1], rtl)]);
            at += 1;
            continue;
        }
        let mut end = at + 1;
        while end < cuts.len() && !is_space_at(end) {
            end += 1;
        }
        let word_begin = cuts[at].0;
        let mut before = 0.0;
        for grapheme in at..end {
            let prefix = font.width(&text[word_begin..cuts[grapheme].1], rtl);
            widths.push(vec![prefix - before]);
            before = prefix;
        }
        at = end;
    }
    widths
}

/// `f32(0.001)`.
fn thousandth() -> f64 {
    fround(0.001)
}

/// A glyph's width at `size` points, in the engine's single precision.
pub(crate) fn scaled(thousandths: f64, size: f64) -> f64 {
    fround(fround(thousandths * size) * thousandth())
}

/// How wide the graphemes `[from, to)` of `paragraph` are at `size` points.
pub(crate) fn graphemes_width(paragraph: &Paragraph, from: usize, to: usize, size: f64) -> f64 {
    let mut width = 0.0;
    for grapheme in &paragraph.graphemes[from..to] {
        for &glyph in &grapheme.widths {
            width = fround(width + scaled(glyph, size));
        }
    }
    width
}

/// A run of a line: one item's characters at one level.
#[derive(Clone, Debug)]
pub(crate) struct Run {
    pub font: Rc<PaintFont>,
    /// Its characters, in the order typed.
    pub text: Vec<u16>,
    /// Drawn right to left.
    pub rtl: bool,
    /// A simple script, or a standard font: drawn with kerning and ligatures
    /// off, as the engine draws it.
    pub plain: bool,
    /// In points.
    pub width: f64,
}

/// The runs of the line of graphemes `[from, to)`, in the order they are
/// drawn, left to right (`RunsFor`): each item's part of the line, cut where
/// the line's levels change, ordered by rule L2.
pub(crate) fn runs_of(paragraph: &Paragraph, from: usize, to: usize, size: f64) -> Vec<Run> {
    if from >= to {
        return Vec::new();
    }
    let Paragraph { text, graphemes, items, .. } = paragraph;
    let begin = graphemes[from].begin;
    let end = graphemes[to - 1].end;
    let levels = line_levels(text, &paragraph.levels, begin, end, paragraph.base);
    let mut runs = Vec::new();
    let mut run_levels = Vec::new();
    let mut index = from;
    while index < to {
        let grapheme = &graphemes[index];
        let level = levels[grapheme.begin - begin];
        let mut next = index + 1;
        while next < to
            && graphemes[next].item == grapheme.item
            && levels[graphemes[next].begin - begin] == level
        {
            next += 1;
        }
        let item = &items[grapheme.item];
        let run_end = graphemes[next - 1].end;
        runs.push(Run {
            font: item.font.clone(),
            text: text[grapheme.begin..run_end].to_vec(),
            rtl: item.level % 2 == 1,
            plain: item.font.standard || is_simple_script(item.script),
            width: graphemes_width(paragraph, index, next, size),
        });
        run_levels.push(level);
        index = next;
    }
    visual_order(&run_levels).into_iter().map(|at| runs[at].clone()).collect()
}
