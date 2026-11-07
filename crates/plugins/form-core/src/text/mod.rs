//! `packages/core/text`: text laid out as the engine's shared text engine lays
//! it out, so the viewer draws what the engine will write before it has. The
//! font each character is drawn in, the Unicode bidirectional order, and a
//! form field's lines, size and runs. The browser shapes each run; this
//! decides what goes where.
//!
//! The TypeScript reads strings as UTF-16 code units and counts positions in
//! them; so does this, over `&[u16]`, and a run's text becomes a `String` only
//! when it is laid out (a run is whole graphemes, so it never splits a
//! surrogate pair).
//!
//! It lives in this crate because the form painter is its only Rust caller
//! today; `core-annotation` lays its free text and redaction labels out with
//! the same package, and a crate of its own would serve both.

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
    GraphemeMatch, LAST_RESORT_ADVANCE, LAST_RESORT_ASCENT, LAST_RESORT_DESCENT, LAST_RESORT_HASH,
    MeasureText, MountableFontProgram, PaintFont, PaintFonts, covers_grapheme, default_paint_fonts,
    last_resort_paint_font, match_grapheme, program_paint_font, standard_font_of,
    standard_font_stack, standard_paint_font,
};
pub(crate) use graphemes::{code_points_of, is_mark};
pub use graphemes::{grapheme_starts, graphemes_of, is_invisible_grapheme};
pub use joining::{
    JoiningType, Joins, grapheme_joining_type, joined_text, joining_type_of, joins_of,
};
pub use scripts::{is_simple_script, script_of};
