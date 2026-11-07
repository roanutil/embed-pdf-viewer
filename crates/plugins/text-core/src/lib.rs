//! `packages/core/text` in Rust: text laid out as the engine's shared text
//! engine lays it out, so the viewer draws what the engine will write before
//! it has. The font each character is drawn in, the Unicode bidirectional
//! order, and a form field's lines, size and runs. The browser shapes each
//! run; this decides what goes where. Pure: no PDFium, no DOM, builds for
//! wasm32.
//!
//! The TypeScript reads strings as UTF-16 code units and counts positions in
//! them; so does this, over `&[u16]`, and a run's text becomes a `String` only
//! when it is laid out (a run is whole graphemes, so it never splits a
//! surrogate pair).
//!
//! | TypeScript | Rust |
//! |---|---|
//! | `layoutField`, `FieldText`, `FieldLine`, `FieldRun`, `FieldTextOptions` | [`layout_field`], [`layout_field_utf16`], [`FieldText`], [`FieldLine`], [`FieldRun`], [`FieldTextOptions`] |
//! | `TextFont`, `TextFonts`, `PaintFont`, `PaintFonts` | [`PaintFont`], [`PaintFonts`] |
//! | `matchGrapheme`, `coversGrapheme` | [`match_grapheme`], [`covers_grapheme`] |
//! | `standardPaintFont`, `programPaintFont`, `lastResortPaintFont`, `defaultPaintFonts` | [`standard_paint_font`], [`program_paint_font`], [`last_resort_paint_font`], [`default_paint_fonts`] |
//! | `firstStrongIsRtl`, `mirrorOf`, `paragraphLevels`, `visualOrder` | the same names, snake case |
//! | `graphemeStarts`, `graphemesOf`, `isInvisibleGrapheme` | the same names, snake case |
//! | `joinsOf`, `joinedText`, `joiningTypeOf`, `graphemeJoiningType` | the same names, snake case |
//! | `isSimpleScript`, `scriptOf` | the same names, snake case |
//! | `PdfRect` (`engine-core`; the plate) | [`PdfRect`] |
//!
//! Every coordinate the layout computes is rounded to single precision where
//! the TypeScript calls `Math.fround`, as PDFium computes in `float`, so a
//! width that only just fits breaks where the engine breaks it.

mod bidi;
mod breaks;
mod field;
mod fonts;
mod graphemes;
mod items;
mod joining;
mod scripts;

pub use bidi::{first_strong_is_rtl, mirror_of, paragraph_levels, visual_order};
pub use breaks::needs_break;
pub use field::{
    Alignment, FieldLine, FieldRun, FieldText, FieldTextOptions, layout_field, layout_field_utf16,
};
pub use fonts::{
    GraphemeMatch, MeasureText, PaintFont, PaintFonts, covers_grapheme, default_paint_fonts,
    last_resort_paint_font, match_grapheme, program_paint_font, standard_font_of,
    standard_font_stack, standard_paint_font,
};
/// The TypeScript keeps these two inside the package; the form painter's
/// conformance adapter counts a measure's characters with them.
pub use graphemes::{code_points_of, is_mark};
pub use graphemes::{grapheme_starts, graphemes_of, is_invisible_grapheme};
pub use joining::{
    JoiningType, Joins, grapheme_joining_type, joined_text, joining_type_of, joins_of,
};
pub use scripts::{is_simple_script, script_of};

/// A rectangle in PDF space (y up). Prints its members in the order the
/// form painter builds them: left, bottom, right, top.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PdfRect {
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
    pub top: f64,
}

/// `Math.fround`: the nearest single-precision value, as PDFium's `float`
/// arithmetic keeps it.
pub(crate) fn fround(x: f64) -> f64 {
    x as f32 as f64
}
