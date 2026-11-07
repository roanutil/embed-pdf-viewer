//! `field.ts`: a form field's text laid out as the engine lays it out
//! (`CPDF_RichTextLayout::ArrangeField`). The geometry of PDFium's form filler
//! (`CPVT`) over the shared text engine's characters. The text splits into
//! sections at its line breaks (one unless the field is multi-line), each
//! section into lines where CPVT breaks them, and each line into runs in the
//! order they are drawn. A font size of 0 is fitted: the biggest of CPVT's
//! sizes the text fits in. A comb puts one character in the middle of each
//! cell, a letter of a cursive script in the form the whole text gives it
//! (joined to the letters around it). A field reads right to left when its
//! first strong character does.
//!
//! Positions are PDF points, y up, as the field's appearance stream has
//! them. The arithmetic is single precision, as the engine's, so a line
//! breaks where the engine breaks it.

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::bidi::first_strong_is_rtl;
use crate::breaks::{is_open_punctuation, is_space, needs_break};
use crate::fonts::{PaintFont, PaintFonts};
use crate::graphemes::grapheme_starts;
use crate::items::{Paragraph, Run, graphemes_width, itemize, runs_of, scaled};
use crate::joining::{Joins, joined_text, joins_of};
use crate::{PdfRect, fround};

fn is_float_zero(value: f64) -> bool {
    value < 0.0001 && value > -0.0001
}

fn is_float_bigger(left: f64, right: f64) -> bool {
    left > right && !is_float_zero(fround(left - right))
}

/// The sizes a fitted font tries, smallest first; a multi-line field tries
/// the first quarter.
const FONT_SIZE_STEPS: [f64; 25] = [
    4.0, 6.0, 8.0, 9.0, 10.0, 12.0, 14.0, 18.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0,
    60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 144.0,
];

/// `/Q`: where a line sits across its plate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left,
    Center,
    Right,
}

impl Serialize for Alignment {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for Alignment {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u8::deserialize(deserializer)? {
            0 => Ok(Alignment::Left),
            1 => Ok(Alignment::Center),
            2 => Ok(Alignment::Right),
            other => Err(serde::de::Error::custom(format!("alignment {other}"))),
        }
    }
}

/// `FieldTextOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldTextOptions {
    /// The box the text is laid out in.
    pub plate: PdfRect,
    /// In points; 0 fits the text to the plate.
    pub font_size: f64,
    /// `/Q`.
    pub alignment: Alignment,
    /// Line breaks start new lines, and long lines wrap.
    pub multiline: bool,
    /// Every character shows as `*`.
    pub password: bool,
    /// At most this many code units, line breaks counted; 0 for no limit.
    pub limit: u32,
    /// A comb: this many cells, one character in each; 0 for none.
    pub cells: u32,
    /// The lines' middle at the plate's middle: a one-line field, a combo
    /// box, a caption.
    pub centered: bool,
}

/// Text one font draws, in one direction.
#[derive(Clone, Debug)]
pub struct FieldRun {
    pub font: Rc<PaintFont>,
    /// Its characters, in the order typed.
    pub text: String,
    /// Drawn right to left: its first character on the right.
    pub rtl: bool,
    /// A simple script, or a standard font: drawn with kerning and ligatures
    /// off, as the engine draws it.
    pub plain: bool,
    /// Where its left edge is.
    pub x: f64,
    /// In points.
    pub width: f64,
    /// A comb cell's letter, joined to the letter in the cell before it (in
    /// typing order), as the engine shapes the whole text: drawn alone, it
    /// takes that joined form only with a joiner (U+200D) on that side.
    pub joins_previous: bool,
    /// The same, to the letter in the cell after it.
    pub joins_next: bool,
}

#[derive(Clone, Debug)]
pub struct FieldLine {
    /// The baseline's y.
    pub baseline: f64,
    /// In the order drawn, left to right.
    pub runs: Vec<FieldRun>,
}

#[derive(Clone, Debug)]
pub struct FieldText {
    /// The size it is drawn at: the field's, or the fitted one.
    pub font_size: f64,
    /// The height its lines take.
    pub content_height: f64,
    pub lines: Vec<FieldLine>,
    /// The direction its lines read in: right to left when its first strong
    /// character is (Hebrew, Arabic), left to right otherwise.
    pub rtl: bool,
    /// Characters no font has, drawn as boxes: each a grapheme, first seen
    /// first.
    pub missing: Vec<String>,
}

/// A grapheme of a section, as CPVT's word breaking and line metrics see it.
struct Unit {
    /// Its grapheme's index in its paragraph.
    grapheme: usize,
    /// Its first code unit, as CPVT's breaking reads it.
    code: u32,
    /// In thousandths of the font size.
    ascent: f64,
    descent: f64,
}

struct Line {
    /// Units; `None` for an empty section's one line.
    begin: Option<usize>,
    end: usize,
    width: f64,
    ascent: f64,
    descent: f64,
}

struct Section {
    paragraph: Paragraph,
    units: Vec<Unit>,
}

struct Placed<'a> {
    section: &'a Section,
    lines: Vec<Line>,
    cell_x: Vec<f64>,
    cell_joins: Vec<Joins>,
    left: f64,
    top: f64,
}

fn string_of(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// Lays `text` out in `fonts`.
pub fn layout_field(text: &str, fonts: &PaintFonts, options: &FieldTextOptions) -> FieldText {
    let text: Vec<u16> = text.encode_utf16().collect();
    layout_field_utf16(&text, fonts, options)
}

/// [`layout_field`] over UTF-16 code units, as the TypeScript reads a string.
pub fn layout_field_utf16(
    text: &[u16],
    fonts: &PaintFonts,
    options: &FieldTextOptions,
) -> FieldText {
    let FieldTextOptions { plate, alignment, multiline, cells, .. } = *options;
    let plate_width = fround(plate.right - plate.left);
    let plate_height = fround(plate.top - plate.bottom);
    let typeset_width = plate_width.max(0.0);
    let texts = sections_of(text, options);
    let rtl = first_strong_is_rtl(&texts.concat()).unwrap_or(false);
    let comb = cells > 0;
    let sections: Vec<Section> = texts
        .iter()
        .map(|section_text| {
            let paragraph = itemize(section_text, rtl, fonts);
            let units = paragraph
                .graphemes
                .iter()
                .enumerate()
                .map(|(index, grapheme)| {
                    let font = &paragraph.items[grapheme.item].font;
                    Unit {
                        grapheme: index,
                        code: u32::from(section_text[grapheme.begin]),
                        ascent: font.ascent,
                        descent: font.descent,
                    }
                })
                .collect();
            Section { paragraph, units }
        })
        .collect();
    let mut missing: Vec<String> = Vec::new();
    for section in &sections {
        for grapheme in &section.paragraph.missing {
            let grapheme = string_of(grapheme);
            if !missing.contains(&grapheme) {
                missing.push(grapheme);
            }
        }
    }

    let unit_width = |section: &Section, unit: &Unit, size: f64| {
        graphemes_width(&section.paragraph, unit.grapheme, unit.grapheme + 1, size)
    };
    // Where each of a comb's characters joins the ones in the cells around
    // it: shaped as one text.
    let cell_joins_of = |section: &Section| -> Vec<Joins> {
        let paragraph = &section.paragraph;
        let grapheme_of = |unit: &Unit| &paragraph.graphemes[unit.grapheme];
        let texts: Vec<&[u16]> = section
            .units
            .iter()
            .map(|unit| &paragraph.text[grapheme_of(unit).begin..grapheme_of(unit).end])
            .collect();
        joins_of(&texts, |a, b| {
            grapheme_of(&section.units[a]).item == grapheme_of(&section.units[b]).item
        })
    };
    // A comb character's width: its joined form's, where it joins a
    // neighbour.
    let cell_width = |section: &Section, unit: &Unit, joins: Joins, size: f64| {
        if !joins.previous && !joins.next {
            return unit_width(section, unit, size);
        }
        let paragraph = &section.paragraph;
        let grapheme = &paragraph.graphemes[unit.grapheme];
        let item = &paragraph.items[grapheme.item];
        let text = joined_text(&paragraph.text[grapheme.begin..grapheme.end], joins);
        scaled(item.font.width(&text, item.level % 2 == 1), size)
    };
    let face_ascent = |size: f64| scaled(fonts.face.ascent, size);
    let face_descent = |size: f64| scaled(fonts.face.descent, size);

    // CPVT's `SplitLines` over a section's units at `size`: the lines they
    // break into in a multi-line field (before the word that would pass the
    // plate's width, or before the unit when one word is wider), and the box
    // they take. `lines` receives them; `None` only measures.
    let split_lines =
        |section: &Section, size: f64, mut lines: Option<&mut Vec<Line>>| -> (f64, f64) {
            let units = &section.units;
            if units.is_empty() {
                let ascent = face_ascent(size);
                let descent = face_descent(size);
                if let Some(lines) = lines {
                    lines.push(Line { begin: None, end: 0, width: 0.0, ascent, descent });
                }
                return (0.0, fround(ascent - descent));
            }
            let mut head = 0usize;
            let mut max_x: f64 = 0.0;
            let mut max_y = 0.0;
            let mut line_width = 0.0;
            let mut backup_width = 0.0;
            let mut line_ascent: f64 = 0.0;
            let mut backup_ascent = 0.0;
            let mut line_descent: f64 = 0.0;
            let mut backup_descent = 0.0;
            let mut word_start = 0usize;
            let mut full_word = false;
            let mut full_words = 0;
            let mut char_index = 0;
            let mut opened = false;
            let mut index = 0usize;
            while index < units.len() {
                let unit = &units[index];
                let previous = &units[if index > 0 { index - 1 } else { index }];
                line_ascent = line_ascent.max(scaled(unit.ascent, size));
                line_descent = line_descent.min(scaled(unit.descent, size));
                if !opened {
                    if is_open_punctuation(unit.code) {
                        opened = true;
                        full_word = true;
                    } else if needs_break(previous.code, unit.code) {
                        full_word = true;
                    }
                } else if !is_space(unit.code) && !is_open_punctuation(unit.code) {
                    opened = false;
                }
                if full_word {
                    full_word = false;
                    if char_index > 0 {
                        full_words += 1;
                    }
                    word_start = index;
                    backup_width = line_width;
                    backup_ascent = line_ascent;
                    backup_descent = line_descent;
                }
                char_index += 1;
                let width = unit_width(section, unit, size);
                if multiline && typeset_width > 0.0 && fround(line_width + width) > typeset_width {
                    if full_words > 0 {
                        index = word_start;
                        line_width = backup_width;
                        line_ascent = backup_ascent;
                        line_descent = backup_descent;
                    }
                    if char_index == 1 {
                        line_width = width;
                        index += 1;
                    }
                    // `addLine(index - 1)`.
                    if let Some(lines) = lines.as_deref_mut() {
                        lines.push(Line {
                            begin: Some(head),
                            end: index.wrapping_sub(1),
                            width: line_width,
                            ascent: line_ascent,
                            descent: line_descent,
                        });
                    }
                    max_y = fround(max_y + line_ascent);
                    max_y = fround(max_y - line_descent);
                    max_x = js_max(line_width, max_x);
                    head = index;
                    line_width = 0.0;
                    line_ascent = 0.0;
                    line_descent = 0.0;
                    char_index = 0;
                    full_words = 0;
                    full_word = false;
                } else {
                    line_width = fround(line_width + width);
                    index += 1;
                }
            }
            if head < units.len() {
                if let Some(lines) = lines.as_deref_mut() {
                    lines.push(Line {
                        begin: Some(head),
                        end: units.len() - 1,
                        width: line_width,
                        ascent: line_ascent,
                        descent: line_descent,
                    });
                }
                max_y = fround(max_y + line_ascent);
                max_y = fround(max_y - line_descent);
                max_x = js_max(line_width, max_x);
            }
            (max_x, max_y)
        };

    // ── The size ──
    let mut font_size = options.font_size;
    if is_float_zero(font_size) {
        font_size = 0.0;
        if plate_width > 0.0 {
            let steps: &[f64] = if multiline {
                &FONT_SIZE_STEPS[..FONT_SIZE_STEPS.len() / 4]
            } else {
                &FONT_SIZE_STEPS
            };
            let too_big = |size: f64| {
                let mut width: f64 = 0.0;
                let mut height = 0.0;
                for section in &sections {
                    let extent = split_lines(section, size, None);
                    width = js_max(extent.0, width);
                    height = fround(height + extent.1);
                    if is_float_bigger(width, plate_width) || is_float_bigger(height, plate_height)
                    {
                        return true;
                    }
                }
                false
            };
            let mut first = 0usize;
            let mut count = steps.len();
            while count > 0 {
                let step = count >> 1;
                if !too_big(steps[first + step]) {
                    first += step + 1;
                    count -= step + 1;
                } else {
                    count = step;
                }
            }
            font_size = if first == steps.len() {
                steps[steps.len() - 1]
            } else {
                steps[if first == 0 { 0 } else { first - 1 }]
            };
        }
    }

    // ── Each section's lines, placed as CPVT places them, one section under
    // the other ──
    let mut placed: Vec<Placed> = Vec::new();
    let mut position_y = 0.0;
    let mut content_height = 0.0;
    for section in &sections {
        let mut lines: Vec<Line> = Vec::new();
        let mut cell_x: Vec<f64> = Vec::new();
        let mut joins: Vec<Joins> = Vec::new();
        let mut left = 0.0;
        let height;
        if comb {
            let node_width = fround(plate_width / f64::from(cells));
            let units = section.units.len() as i64;
            let cells_i = i64::from(cells);
            let start = match alignment {
                // `Math.trunc((cells - units) / 2)`.
                Alignment::Center => (cells_i - units) / 2,
                Alignment::Right => cells_i - units,
                Alignment::Left => 0,
            };
            let ascent = face_ascent(font_size);
            let mut line_descent = face_descent(font_size);
            joins = cell_joins_of(section);
            let mut index = 0;
            while (index as i64) < units && index < cells as usize {
                let unit = &section.units[index];
                let width = cell_width(section, unit, joins[index], font_size);
                cell_x.push(fround(
                    node_width * (index as f64 + start as f64 + 0.5) - fround(width * 0.5),
                ));
                line_descent = line_descent.min(scaled(unit.descent, font_size));
                index += 1;
            }
            lines.push(Line {
                begin: if units > 0 { Some(0) } else { None },
                end: (units - 1).max(0) as usize,
                width: 0.0,
                ascent,
                descent: line_descent,
            });
            height = fround(ascent - line_descent);
        } else {
            let extent = split_lines(section, font_size, Some(&mut lines));
            left = alignment_offset(alignment, typeset_width, extent.0);
            height = extent.1;
        }
        placed.push(Placed { section, lines, cell_x, cell_joins: joins, left, top: position_y });
        position_y = fround(position_y + height);
        content_height = fround(content_height + height);
    }

    // A one-line field's text has its middle at the plate's middle.
    let offset_y =
        if options.centered { fround(fround(content_height - plate_height) / 2.0) } else { 0.0 };

    let field_run = |run: Run, x: f64| FieldRun {
        font: run.font,
        text: string_of(&run.text),
        rtl: run.rtl,
        plain: run.plain,
        x: fround(plate.left + x),
        width: run.width,
        joins_previous: false,
        joins_next: false,
    };
    let mut out: Vec<FieldLine> = Vec::new();
    for Placed { section, lines, cell_x, cell_joins: joins, left, top } in placed {
        let mut line_top = 0.0;
        for line in &lines {
            line_top = fround(line_top + line.ascent);
            let baseline_y = fround(fround(line_top + top) - offset_y);
            line_top = fround(line_top - line.descent);
            let mut runs: Vec<FieldRun> = Vec::new();
            if let Some(begin) = line.begin {
                if comb {
                    for (index, &x) in cell_x.iter().enumerate() {
                        let unit = &section.units[index];
                        let cell = joins[index];
                        for run in
                            runs_of(&section.paragraph, unit.grapheme, unit.grapheme + 1, font_size)
                        {
                            runs.push(FieldRun {
                                width: cell_width(section, unit, cell, font_size),
                                joins_previous: cell.previous,
                                joins_next: cell.next,
                                ..field_run(run, x)
                            });
                        }
                    }
                } else {
                    let mut x = alignment_offset(alignment, typeset_width, line.width);
                    let from = section.units[begin].grapheme;
                    let to = section.units[line.end].grapheme + 1;
                    for run in runs_of(&section.paragraph, from, to, font_size) {
                        let width = run.width;
                        runs.push(field_run(run, fround(fround(x - left) + left)));
                        x = fround(x + width);
                    }
                }
            }
            out.push(FieldLine { baseline: fround(plate.top - baseline_y), runs });
        }
    }
    FieldText { font_size, content_height, lines: out, rtl, missing }
}

/// `Math.max` of two numbers that are never NaN here.
fn js_max(left: f64, right: f64) -> f64 {
    if left > right { left } else { right }
}

/// Where a line `width` wide starts in `typeset_width`, by `/Q`.
fn alignment_offset(alignment: Alignment, typeset_width: f64, width: f64) -> f64 {
    match alignment {
        Alignment::Center => fround(fround(typeset_width - width) * 0.5),
        Alignment::Right => fround(typeset_width - width),
        Alignment::Left => 0.0,
    }
}

/// The text's sections as CPVT takes it (`FieldSectionsOf`): line breaks (CR,
/// LF or both) start a section in a multi-line field and are dropped in any
/// other, a tab is a space, and the limit or the comb's cells stop it.
/// Graphemes stay whole; a password's are `*`.
fn sections_of(text: &[u16], options: &FieldTextOptions) -> Vec<Vec<u16>> {
    let FieldTextOptions { limit, cells, multiline, password, .. } = *options;
    let (limit, cells) = (limit as usize, cells as usize);
    let starts = grapheme_starts(text);
    let mut sections: Vec<Vec<u16>> = vec![Vec::new()];
    // What CPVT counts: the text taken, line breaks included, and what it
    // kept, a break between sections included; the limit in code units, the
    // cells in characters.
    let mut taken_units = 0;
    let mut taken_characters = 0;
    let mut kept_units = 0;
    let mut kept_characters = 0;
    let mut begin = 0;
    while begin < text.len() {
        let mut end = begin + 1;
        while end < text.len() && !starts[end] {
            end += 1;
        }
        let units = end - begin;
        if limit > 0 && taken_units + units > limit {
            break;
        }
        if cells > 0 && taken_characters >= cells {
            break;
        }
        let first = text[begin];
        if first == u16::from(b'\r') || first == u16::from(b'\n') {
            if multiline {
                // CR LF is one break.
                let pair = if first == u16::from(b'\r') { b'\n' } else { b'\r' };
                if text.get(end) == Some(&u16::from(pair)) {
                    end += 1;
                }
                sections.push(Vec::new());
                kept_units += 1;
                kept_characters += 1;
            }
        } else {
            let full = (limit > 0 && kept_units + units > limit)
                || (cells > 0 && kept_characters >= cells);
            if !full {
                let last = sections.last_mut().expect("a section");
                if password {
                    last.push(u16::from(b'*'));
                } else if first == u16::from(b'\t') {
                    last.push(u16::from(b' '));
                } else {
                    last.extend_from_slice(&text[begin..end]);
                }
                kept_units += units;
                kept_characters += 1;
            }
        }
        taken_units += units;
        taken_characters += 1;
        begin = end;
    }
    sections
}
