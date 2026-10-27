//! `editor.ts`: a text field's layout as an HTML editor needs it, so the text
//! being typed sits where the engine will draw it. The face, the size the
//! engine draws at (a fitted size resolved for this text), where the text's
//! box sits in the widget's, one line's height and a comb's spacing. In
//! points, in the widget's upright frame.

use epdf_fonts::{FontProgram, FontStyle, standard_font_css, standard_font_program};
use serde::Serialize;

use crate::drawing::{text_field_layout, text_look_of};
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
    /// The face the engine draws the text in: mounted, `font_family` reaches
    /// it.
    pub program: &'static FontProgram,
    pub font_family: String,
    pub font_weight: u32,
    pub font_style: FontStyle,
    /// The size the engine draws the text at.
    pub font_size: f64,
    /// One line, from the face's ascent to its descent.
    pub line_height: f64,
    pub padding: Padding,
    /// A comb's room between characters: its cell, less a character's width.
    /// A comb starts its first character at `padding.left`, whatever the
    /// field's alignment. `0` for a field without one.
    pub letter_spacing: f64,
    pub comb: bool,
}

/// The layout of `text` in the widget's field; `None` for a widget that
/// isn't a text field's.
pub fn text_editor_layout(
    row: &WidgetRow,
    field: &FieldFacts,
    text: &str,
) -> Option<TextEditorLayout> {
    let FieldFacts::Text(facts) = field else { return None };
    let (look, _) = text_look_of(row);
    let (plate, layout) = text_field_layout(&look, facts, text);
    let bbox = look.bbox;
    let font = look.font;
    let font_size = layout.font_size;
    let css = standard_font_css(font.font);
    let padding = Padding {
        top: bbox.top - plate.top,
        right: bbox.right - plate.right,
        bottom: plate.bottom - bbox.bottom,
        left: plate.left - bbox.left,
    };
    let base = TextEditorLayout {
        program: standard_font_program(font.font),
        font_family: css.font_family,
        font_weight: css.font_weight,
        font_style: css.font_style,
        font_size,
        line_height: (f64::from(font.ascent - font.descent) * font_size) / 1000.0,
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
    let placed = if text.is_empty() { text_field_layout(&look, facts, "0").1 } else { layout };
    let words: Vec<_> = placed.lines.iter().flat_map(|line| line.words.iter()).collect();
    let total = words.iter().fold(0.0, |sum, word| {
        sum + (f64::from(font.width(u32::from(word.code)).unwrap_or(0)) * placed.font_size) / 1000.0
    });
    let mean = total / (words.len().max(1) as f64);
    let cell = (plate.right - plate.left) / f64::from(cells);
    Some(TextEditorLayout {
        padding: Padding {
            left: words.first().map_or(plate.left, |word| word.x) - bbox.left,
            ..padding
        },
        letter_spacing: cell - mean,
        comb: true,
        ..base
    })
}
