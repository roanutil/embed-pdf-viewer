//! `layout/variable-text.ts`: text laid out as the engine lays out a widget's
//! text. A port of PDFium's variable text (`CPVT_VariableText`,
//! `CPVT_Section`), the layout the appearance generator draws every text
//! field, combo box, list box row and push button caption with.
//!
//! The text is split into sections at its line breaks (one line unless the
//! field is multi-line), each section into lines where the words would pass
//! the plate's width ([`needs_break`], CPVT's `NeedDivision`), and each line
//! is aligned in the plate. A font size of 0 is fitted: the biggest of CPVT's
//! sizes the text fits in. A comb field puts one character in the middle of
//! each of its cells instead.
//!
//! Positions are in the generator's space: PDF points, y up. A line's `y` is
//! its baseline.

use std::cell::Cell;

use epdf_geometry::js;
use serde::{Serialize, Serializer};

use super::breaks::{is_open_punctuation, is_space, needs_break};
use crate::LayoutFont;
use crate::fround;

/// The sizes a fitted font tries, smallest first; a multi-line field tries the
/// first quarter.
const FONT_SIZE_STEPS: [f64; 25] = [
    4.0, 6.0, 8.0, 9.0, 10.0, 12.0, 14.0, 18.0, 20.0, 25.0, 30.0, 35.0, 40.0, 45.0, 50.0, 55.0,
    60.0, 70.0, 80.0, 90.0, 100.0, 110.0, 120.0, 130.0, 144.0,
];

/// A rectangle in PDF space (y up). Prints its members in the order the
/// painter builds them: left, bottom, right, top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// `/Q`, as the layout takes it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Left = 0,
    Center = 1,
    Right = 2,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VariableTextOptions {
    /// The box the text is laid out in.
    pub plate: PdfRect,
    /// In points; 0 fits the text to the plate.
    pub font_size: f64,
    pub alignment: Alignment,
    /// Line breaks start new lines, and long lines wrap.
    pub multiline: bool,
    /// Every character shows as this one (a password field's `*`).
    pub substitute: Option<char>,
    /// At most this many characters, line breaks counted; 0 for no limit.
    pub limit: u32,
    /// A comb: this many cells, one character in each; 0 for none.
    pub cells: u32,
}

/// One character as laid out: its UTF-16 code unit, where its baseline
/// starts, and its advance.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct LaidWord {
    pub code: u16,
    pub x: f64,
    pub y: f64,
    /// In points, a comb's gap to the next cell included.
    pub width: f64,
}

/// One line: where it starts, and its characters.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LaidLine {
    pub x: f64,
    pub y: f64,
    pub words: Vec<LaidWord>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextLayout {
    pub font_size: f64,
    pub lines: Vec<LaidLine>,
    /// The box the lines take, in the same space. Prints left, right, top,
    /// bottom, the order the TypeScript builds it in.
    #[serde(serialize_with = "content_order")]
    pub content: PdfRect,
    /// A character the font can't write: the engine draws it in another font.
    pub approximate: bool,
}

fn content_order<S: Serializer>(rect: &PdfRect, serializer: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Content {
        left: f64,
        right: f64,
        top: f64,
        bottom: f64,
    }
    Content { left: rect.left, right: rect.right, top: rect.top, bottom: rect.bottom }
        .serialize(serializer)
}

#[derive(Clone, Copy, Debug)]
struct Word {
    code: u16,
    /// A comb's gap to the next cell.
    tail: f64,
    x: f64,
    y: f64,
}

#[derive(Clone, Copy, Debug)]
struct Line {
    /// -1 for a section without words.
    begin: isize,
    end: isize,
    x: f64,
    y: f64,
    width: f64,
    ascent: f64,
    descent: f64,
}

impl Line {
    /// The words it holds, as the TypeScript walks them: from `begin` while
    /// at most `end` and not negative.
    fn range(&self) -> Option<std::ops::RangeInclusive<usize>> {
        (self.begin >= 0 && self.end >= self.begin)
            .then_some(self.begin as usize..=self.end as usize)
    }
}

/// In layout space: y down from the plate's top.
#[derive(Clone, Copy, Debug, Default)]
struct LayoutRect {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[derive(Debug, Default)]
struct Section {
    words: Vec<Word>,
    lines: Vec<Line>,
    rect: LayoutRect,
}

fn is_float_zero(value: f64) -> bool {
    value < 0.0001 && value > -0.0001
}

fn is_float_bigger(left: f64, right: f64) -> bool {
    left > right && !is_float_zero(fround(left - right))
}

/// The pieces of one layout, which the TypeScript keeps as closures over
/// `layoutVariableText`'s locals.
struct VariableText<'a> {
    font: &'a LayoutFont,
    substitute: u16,
    fallback_width: f64,
    plate_width: f64,
    plate_height: f64,
    alignment: Alignment,
    multiline: bool,
    cells: u32,
    font_size: f64,
    approximate: Cell<bool>,
}

impl VariableText<'_> {
    fn char_width(&self, word: &Word) -> f64 {
        let code = if self.substitute != 0 { self.substitute } else { word.code };
        match self.font.width(u32::from(code)) {
            Some(width) => f64::from(width),
            None => {
                self.approximate.set(true);
                self.fallback_width
            }
        }
    }

    fn word_width(&self, word: &Word, size: f64) -> f64 {
        let scale = fround(0.001);
        fround(fround(fround(self.char_width(word) * size) * scale) + word.tail)
    }

    fn font_ascent(&self, size: f64) -> f64 {
        fround(fround(f64::from(self.font.ascent) * size) * fround(0.001))
    }

    fn font_descent(&self, size: f64) -> f64 {
        fround(fround(f64::from(self.font.descent) * size) * fround(0.001))
    }

    /// The biggest step the text fits in (`GetAutoFontSize`): the step before
    /// the first one that's too big, searched as `std::lower_bound` searches.
    fn auto_font_size(&self, sections: &mut [Section]) -> f64 {
        if self.plate_width <= 0.0 {
            return 0.0;
        }
        let steps: &[f64] = if self.multiline {
            &FONT_SIZE_STEPS[..FONT_SIZE_STEPS.len() / 4]
        } else {
            &FONT_SIZE_STEPS
        };
        let mut first = 0;
        let mut count = steps.len();
        while count > 0 {
            let step = count >> 1;
            if !self.is_bigger(sections, steps[first + step]) {
                first += step + 1;
                count -= step + 1;
            } else {
                count = step;
            }
        }
        if first == steps.len() {
            return steps[steps.len() - 1];
        }
        if first == 0 {
            return steps[0];
        }
        steps[first - 1]
    }

    fn is_bigger(&self, sections: &mut [Section], size: f64) -> bool {
        let mut width = 0.0;
        let mut height = 0.0;
        for section in sections {
            let rect = self.split_lines(section, false, size);
            width = js::max(fround(rect.right - rect.left), width);
            height = fround(height + fround(rect.bottom - rect.top));
            if is_float_bigger(width, self.plate_width)
                || is_float_bigger(height, self.plate_height)
            {
                return true;
            }
        }
        false
    }

    /// CPVT's `SplitLines`: the lines a section breaks into; `typeset` keeps
    /// them.
    fn split_lines(&self, section: &mut Section, typeset: bool, size: f64) -> LayoutRect {
        let line_size = if typeset { self.font_size } else { size };
        let count = section.words.len();
        if count == 0 {
            let ascent = self.font_ascent(line_size);
            let descent = self.font_descent(line_size);
            if typeset {
                section.lines.push(Line {
                    begin: -1,
                    end: -1,
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    ascent,
                    descent,
                });
            }
            return LayoutRect { bottom: fround(ascent - descent), ..LayoutRect::default() };
        }
        let mut line_head: usize = 0;
        let mut max_x = 0.0;
        let mut max_y = 0.0;
        let mut line_width = 0.0;
        let mut backup_width = 0.0;
        let mut line_ascent = 0.0;
        let mut backup_ascent = 0.0;
        let mut line_descent = 0.0;
        let mut backup_descent = 0.0;
        let mut word_start: usize = 0;
        let mut full_word = false;
        let mut full_words = 0;
        let mut char_index = 0;
        let mut opened = false;
        let typeset_width = js::max(self.plate_width, 0.0);
        let mut push_line = |end: isize,
                             line_head: usize,
                             width: f64,
                             ascent: f64,
                             descent: f64,
                             lines: &mut Vec<Line>| {
            if typeset {
                lines.push(Line {
                    begin: line_head as isize,
                    end,
                    x: 0.0,
                    y: 0.0,
                    width,
                    ascent,
                    descent,
                });
            }
            max_y = fround(fround(max_y + ascent) - descent);
            max_x = js::max(width, max_x);
        };
        let mut index: usize = 0;
        while index < count {
            let word = section.words[index];
            let previous = if index > 0 { section.words[index - 1] } else { word };
            line_ascent = js::max(line_ascent, self.font_ascent(line_size));
            line_descent = js::min(line_descent, self.font_descent(line_size));
            let width = self.word_width(&word, line_size);
            let code = u32::from(word.code);
            if !opened {
                if is_open_punctuation(code) {
                    opened = true;
                    full_word = true;
                } else if needs_break(u32::from(previous.code), code) {
                    full_word = true;
                }
            } else if !is_space(code) && !is_open_punctuation(code) {
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
            if self.multiline && typeset_width > 0.0 && fround(line_width + width) > typeset_width {
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
                push_line(
                    index as isize - 1,
                    line_head,
                    line_width,
                    line_ascent,
                    line_descent,
                    &mut section.lines,
                );
                line_head = index;
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
        if line_head < count {
            push_line(
                count as isize - 1,
                line_head,
                line_width,
                line_ascent,
                line_descent,
                &mut section.lines,
            );
        }
        LayoutRect { left: 0.0, top: 0.0, right: max_x, bottom: max_y }
    }

    /// The offset of a line `width` wide in the plate, by the alignment.
    fn aligned(&self, typeset_width: f64, width: f64) -> f64 {
        match self.alignment {
            Alignment::Left => 0.0,
            Alignment::Center => fround(fround(typeset_width - width) * 0.5),
            Alignment::Right => fround(typeset_width - width),
        }
    }

    /// CPVT's `RearrangeTypeset` and `OutputLines`: the lines, aligned, each
    /// word placed.
    fn arrange_typeset(&self, section: &mut Section) -> LayoutRect {
        section.lines.clear();
        let rect = self.split_lines(section, true, 0.0);
        let typeset_width = js::max(self.plate_width, 0.0);
        let rect_width = fround(rect.right - rect.left);
        let min_x = self.aligned(typeset_width, rect_width);
        let mut position_y = 0.0;
        for line in &mut section.lines {
            let mut position_x = self.aligned(typeset_width, line.width);
            position_y = fround(position_y + line.ascent);
            line.x = fround(position_x - min_x);
            line.y = position_y;
            if let Some(range) = line.range() {
                for word in &mut section.words[range] {
                    word.x = fround(position_x - min_x);
                    word.y = position_y;
                    position_x = fround(position_x + self.word_width(word, self.font_size));
                }
            }
            position_y = fround(position_y - line.descent);
        }
        LayoutRect {
            left: min_x,
            top: 0.0,
            right: fround(min_x + rect_width),
            bottom: fround(rect.bottom - rect.top),
        }
    }

    /// CPVT's `RearrangeCharArray`: a comb, one character in the middle of
    /// each cell.
    fn arrange_cells(&self, section: &mut Section) -> LayoutRect {
        let cells = f64::from(self.cells);
        let node_width = fround(self.plate_width / if self.cells == 0 { 1.0 } else { cells });
        let mut line_ascent = self.font_ascent(self.font_size);
        let mut line_descent = self.font_descent(self.font_size);
        let y = line_ascent;
        let count = section.words.len();
        let (start, mut line_x) = match self.alignment {
            Alignment::Left => (0.0, fround(node_width * 0.5)),
            Alignment::Center => {
                let start = ((cells - count as f64) / 2.0).trunc();
                (start, fround(fround(node_width * start) - fround(node_width * 0.5)))
            }
            Alignment::Right => {
                let start = cells - count as f64;
                (start, fround(fround(node_width * start) - fround(node_width * 0.5)))
            }
        };
        let mut x = 0.0;
        let mut index = 0;
        while index < count && (index as f64) < cells {
            let mut next_width = 0.0;
            if index + 1 < count {
                section.words[index + 1].tail = 0.0;
                next_width = self.word_width(&section.words[index + 1], self.font_size);
            }
            section.words[index].tail = 0.0;
            let width = self.word_width(&section.words[index], self.font_size);
            x = fround(node_width * (index as f64 + start + 0.5) - fround(width * 0.5));
            let word = &mut section.words[index];
            word.x = x;
            word.y = y;
            if index == 0 {
                line_x = x;
            }
            let gap = fround(node_width - fround(fround(width + next_width) * 0.5));
            word.tail = if index != count - 1 && gap > 0.0 { gap } else { 0.0 };
            x = fround(x + width);
            line_ascent = js::max(line_ascent, self.font_ascent(self.font_size));
            line_descent = js::min(line_descent, self.font_descent(self.font_size));
            index += 1;
        }
        section.lines = vec![Line {
            begin: 0,
            end: count as isize - 1,
            x: line_x,
            y,
            width: fround(x - line_x),
            ascent: line_ascent,
            descent: line_descent,
        }];
        LayoutRect { left: 0.0, top: 0.0, right: x, bottom: fround(y - line_descent) }
    }
}

/// Lays `text` out in `font` as the engine's variable text does.
pub fn layout_variable_text(
    text: &str,
    font: &LayoutFont,
    options: &VariableTextOptions,
) -> TextLayout {
    let VariableTextOptions { plate, alignment, multiline, limit, cells, .. } = *options;
    let substitute = options.substitute.map_or(0, |c| c.encode_utf16(&mut [0; 2])[0]);
    let mut layout = VariableText {
        font,
        substitute,
        fallback_width: font.width(0x6e).map_or(500.0, f64::from),
        plate_width: fround(plate.right - plate.left),
        plate_height: fround(plate.top - plate.bottom),
        alignment,
        multiline,
        cells,
        font_size: options.font_size,
        approximate: Cell::new(false),
    };

    // ── SetText ──
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut sections = vec![Section::default()];
    let total_words =
        |sections: &[Section]| sections.iter().map(|s| s.words.len() + 1).sum::<usize>() - 1;
    let limit = limit as usize;
    let cells_count = cells as usize;
    let mut count = 0;
    let mut index = 0;
    while index < units.len() {
        if limit > 0 && count >= limit {
            break;
        }
        if cells_count > 0 && count >= cells_count {
            break;
        }
        let mut code = units[index];
        if code == 0x0d || code == 0x0a {
            if multiline {
                let pair = if code == 0x0d { 0x0a } else { 0x0d };
                if units.get(index + 1) == Some(&pair) {
                    index += 1;
                }
                sections.push(Section::default());
            }
        } else {
            if code == 0x09 {
                code = 0x20;
            }
            let full = (limit > 0 && total_words(&sections) >= limit)
                || (cells_count > 0 && total_words(&sections) >= cells_count);
            if !full {
                let last = sections.len() - 1;
                sections[last].words.push(Word { code, tail: 0.0, x: 0.0, y: 0.0 });
            }
        }
        count += 1;
        index += 1;
    }

    // ── Rearrange ──
    if is_float_zero(layout.font_size) {
        layout.font_size = layout.auto_font_size(&mut sections);
    }
    let mut content = LayoutRect::default();
    let mut position_y = 0.0;
    for (index, section) in sections.iter_mut().enumerate() {
        let mut rect =
            if cells > 0 { layout.arrange_cells(section) } else { layout.arrange_typeset(section) };
        rect.top = fround(rect.top + position_y);
        rect.bottom = fround(rect.bottom + position_y);
        section.rect = rect;
        if index == 0 {
            content = rect;
        } else {
            content.left = js::min(rect.left, content.left);
            content.top = js::min(rect.top, content.top);
            content.right = js::max(rect.right, content.right);
            content.bottom = js::max(rect.bottom, content.bottom);
        }
        position_y = fround(position_y + fround(rect.bottom - rect.top));
    }

    // In layout space y runs down from the plate's top; out of it, PDF space.
    let out_x = |x: f64| fround(x + plate.left);
    let out_y = |y: f64| fround(plate.top - y);
    let mut lines = Vec::new();
    for section in &sections {
        for line in &section.lines {
            let mut words = Vec::new();
            if let Some(range) = line.range() {
                for word in &section.words[range] {
                    words.push(LaidWord {
                        code: if substitute != 0 { substitute } else { word.code },
                        x: out_x(fround(word.x + section.rect.left)),
                        y: out_y(fround(word.y + section.rect.top)),
                        width: layout.word_width(word, layout.font_size),
                    });
                }
            }
            lines.push(LaidLine {
                x: out_x(fround(line.x + section.rect.left)),
                y: out_y(fround(line.y + section.rect.top)),
                words,
            });
        }
    }
    TextLayout {
        font_size: layout.font_size,
        lines,
        content: PdfRect {
            left: out_x(content.left),
            bottom: out_y(content.bottom),
            right: out_x(content.right),
            top: out_y(content.top),
        },
        approximate: layout.approximate.get(),
    }
}
