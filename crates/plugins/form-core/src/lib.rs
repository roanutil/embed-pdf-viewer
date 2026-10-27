//! `packages/core/form` in Rust: the form widget painter. What a widget looks
//! like for its facts, ported in TypeScript from PDFium's appearance generator
//! (`cpdf_generateap.cpp`) and its text layout (`CPVT`), and ported here from
//! the TypeScript. Pure: no PDFium, no DOM, builds for wasm32.
//!
//! | TypeScript | Rust |
//! |---|---|
//! | `widgetDrawing(widget, field, shown)` | [`widget_drawing`] |
//! | `paints(field)` | [`paints`] |
//! | `textEditorLayout(widget, field, text)` | [`text_editor_layout`] |
//! | `layoutFontOf(fontFamily)` | [`layout_font_of`] |
//! | `widgetScene(drawing, placement)` | [`widget_scene`] |
//! | `layoutVariableText(text, font, options)` | [`layout_variable_text`] |
//! | `WidgetAnnotation`, `FormFieldDTO` (what the painter reads of them) | [`WidgetRow`], [`FieldFacts`] |
//! | `SceneNode`, `Paint` (`core-annotation`; the nodes `widgetScene` makes) | [`SceneNode`], [`Paint`] |
//!
//! Every coordinate the generator computes is rounded to single precision
//! where the TypeScript calls `Math.fround`, as PDFium computes in `float`,
//! so a width that only just fits breaks where the engine breaks it.

mod drawing;
mod editor;
mod font;
mod layout;
mod scene;

#[cfg(feature = "harness")]
pub mod harness;

pub use drawing::{
    Approximation, DrawOp, FieldFacts, FieldFamily, FieldOption, PathSegment, Rgb, ShownValue,
    TextAlign, TextFieldFacts, TextRun, WidgetBorderStyle, WidgetCheckStyle, WidgetDrawing,
    WidgetRow, paints, widget_drawing,
};
pub use editor::{Padding, TextEditorLayout, text_editor_layout};
pub use epdf_fonts::FontProgram;
pub use font::{LayoutFont, layout_font_of};
pub use layout::{
    Alignment, LaidLine, LaidWord, PdfRect, TextLayout, VariableTextOptions, layout_variable_text,
};
pub use scene::{Paint, SceneNode, StandardProgram, WidgetPlacement, widget_scene};

/// `Math.fround`: the nearest single-precision value, as PDFium's `float`
/// arithmetic keeps it.
pub(crate) fn fround(x: f64) -> f64 {
    x as f32 as f64
}
