//! `packages/core/form` in Rust: the form widget painter. What a widget looks
//! like for its facts, ported in TypeScript from PDFium's appearance generator
//! (`cpdf_generateap.cpp`), and ported here from the TypeScript; with
//! `packages/core/text` ([`text`]), the layout it draws its text with, as the
//! engine's shared text engine lays it out. Pure: no PDFium, no DOM, builds
//! for wasm32.
//!
//! | TypeScript | Rust |
//! |---|---|
//! | `widgetDrawing(widget, field, shown, fonts)` | [`widget_drawing`], [`widget_drawing_in`] |
//! | `widgetText(widget, field, shown)` | [`widget_text`] |
//! | `paints(field)` | [`paints`] |
//! | `textEditorLayout(widget, field, text, fonts)` | [`text_editor_layout`], [`text_editor_layout_in`] |
//! | `widgetScene(drawing, placement)` | [`widget_scene`] |
//! | `WidgetAnnotation`, `FormField` (what the painter reads of them) | [`WidgetRow`], [`FieldFacts`] |
//! | `SceneNode`, `Paint` (`core-annotation`; the nodes `widgetScene` makes) | [`SceneNode`], [`Paint`] |
//! | `@embedpdf/core-text`: `layoutField`, the paint fonts | [`text`] |
//!
//! Every coordinate the generator computes is rounded to single precision
//! where the TypeScript calls `Math.fround`, as PDFium computes in `float`,
//! so a width that only just fits breaks where the engine breaks it.

mod drawing;
mod editor;
mod scene;
pub mod text;

#[cfg(feature = "harness")]
pub mod harness;

pub use drawing::{
    Approximation, DrawOp, FieldFacts, FieldFamily, FieldKind, FieldOption, PathSegment, Rgb,
    ShownValue, TextAlign, TextFieldFacts, TextRun, WidgetBorderStyle, WidgetCheckStyle,
    WidgetDrawing, WidgetRow, paints, widget_drawing, widget_drawing_in, widget_text,
};
pub use editor::{Padding, TextEditorLayout, text_editor_layout, text_editor_layout_in};
pub use scene::{Paint, SceneNode, WidgetPlacement, widget_scene};

/// A rectangle in PDF space (y up). Prints its members in the order the
/// painter builds them: left, bottom, right, top.
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
