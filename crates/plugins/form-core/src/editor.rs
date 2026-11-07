//! `editor.ts`: a text field's layout as an HTML editor needs it, so the text
//! being typed sits where the engine will draw it. The face, the size the
//! engine draws at (a fitted size resolved for this text), where the text's
//! box sits in the widget's, one line's height and a comb's spacing. In
//! points, in the widget's upright frame.

use std::rc::Rc;

use epdf_fonts::FontStyle;
use serde::Serialize;

use crate::drawing::{text_field_layout, text_look_of};
use crate::text::{
    FieldText, MountableFontProgram, PaintFont, PaintFonts, standard_font_of, standard_font_stack,
};
use crate::{FieldFacts, WidgetRow};

/// Where the text starts inside the widget's box, from each edge.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Padding {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEditorLayout {
    /// The programs the engine draws the text in, the face's first: mounted,
    /// `font_family` reaches them.
    pub programs: Vec<Rc<MountableFontProgram>>,
    pub font_family: String,
    pub font_weight: u32,
    pub font_style: FontStyle,
    /// The size the engine draws the text at.
    pub font_size: f64,
    /// One line, from the face's ascent to its descent.
    pub line_height: f64,
    /// The text reads right to left, as the engine draws it: its first strong
    /// character is (Hebrew, Arabic). An editor reads the line in the same
    /// direction, so a line of mixed scripts is ordered as the engine orders
    /// it.
    pub rtl: bool,
    pub padding: Padding,
    /// A comb's room between characters: its cell, less a character's width.
    /// A comb starts its first character at `padding.left`, whatever the
    /// field's alignment. `0` for a field without one.
    pub letter_spacing: f64,
    pub comb: bool,
}

/// The CSS family list that draws text in `programs`, then the look-alikes
/// of the face's standard font (Helvetica's for any other) for before they
/// load.
fn family_list(fonts: &PaintFonts, programs: &[Rc<MountableFontProgram>]) -> String {
    let stack = standard_font_stack(standard_font_of(&fonts.face.key));
    programs
        .iter()
        .map(|program| format!("\"{}\"", program.family))
        .chain(std::iter::once(stack.to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The programs that draw `laid`, each once, in the order they're tried: the
/// face's first, then the fallbacks its text is drawn in (a browser loads no
/// other), then the last resort's.
fn programs_of(fonts: &PaintFonts, laid: &FieldText) -> Vec<Rc<MountableFontProgram>> {
    let drawn = |font: &Rc<PaintFont>| {
        laid.lines.iter().flat_map(|line| &line.runs).any(|run| Rc::ptr_eq(&run.font, font))
    };
    let all = std::iter::once(&fonts.face)
        .chain(fonts.fallbacks.iter().filter(|font| drawn(font)))
        .chain(fonts.last_resort.iter());
    // `new Map(all.map((font) => [font.program.family, font.program])).values()`:
    // each family at its first place, holding its last program.
    let mut programs: Vec<Rc<MountableFontProgram>> = Vec::new();
    for font in all {
        match programs.iter_mut().find(|program| program.family == font.program.family) {
            Some(known) => *known = font.program.clone(),
            None => programs.push(font.program.clone()),
        }
    }
    programs
}

/// The layout of `text` in the widget's field, in the widget's standard font;
/// `None` for a widget that isn't a text field's.
pub fn text_editor_layout(
    row: &WidgetRow,
    field: &FieldFacts,
    text: &str,
) -> Option<TextEditorLayout> {
    text_editor_layout_in(row, field, text, None)
}

/// [`text_editor_layout`] in `fonts`, as the form plugin asks when it knows
/// the document's fonts.
pub fn text_editor_layout_in(
    row: &WidgetRow,
    field: &FieldFacts,
    text: &str,
    fonts: Option<&PaintFonts>,
) -> Option<TextEditorLayout> {
    let FieldFacts::Text(facts) = field else { return None };
    let (look, _) = text_look_of(row, fonts);
    let (plate, laid) = text_field_layout(&look, facts, text);
    let bbox = look.bbox;
    let face = &look.fonts.face;
    let font_size = laid.font_size;
    let programs = programs_of(&look.fonts, &laid);
    let padding = Padding {
        top: bbox.top - plate.top,
        right: bbox.right - plate.right,
        bottom: plate.bottom - bbox.bottom,
        left: plate.left - bbox.left,
    };
    let base = TextEditorLayout {
        font_family: family_list(&look.fonts, &programs),
        programs,
        font_weight: face.font_weight,
        font_style: face.font_style,
        font_size,
        line_height: ((face.ascent - face.descent) * font_size) / 1000.0,
        rtl: laid.rtl,
        padding,
        letter_spacing: 0.0,
        comb: false,
    };
    let cells = if facts.comb { facts.max_length.unwrap_or(0) } else { 0 };
    if cells == 0 {
        return Some(base);
    }

    // A comb centres each character in its cell: CSS can only space them
    // evenly, so the room is the cell less the characters' mean width (exact
    // for a fixed-width face). An empty comb places a stand-in, for where the
    // first character will go.
    let placed = if text.is_empty() { text_field_layout(&look, facts, "0").1 } else { laid };
    let runs: Vec<_> = placed.lines.iter().flat_map(|line| line.runs.iter()).collect();
    let total = runs.iter().fold(0.0, |sum, run| sum + run.width);
    let mean = total / (runs.len().max(1) as f64);
    let cell = (plate.right - plate.left) / f64::from(cells);
    Some(TextEditorLayout {
        padding: Padding {
            left: runs.first().map_or(plate.left, |run| run.x) - bbox.left,
            ..padding
        },
        letter_spacing: cell - mean,
        comb: true,
        ..base
    })
}
