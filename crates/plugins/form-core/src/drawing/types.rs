//! `drawing/types.ts`: what a widget's appearance draws, in the generator's
//! own terms: fills, strokes, a clip and runs of text, in the widget's upright
//! frame (PDF points, y up, the origin at the frame's bottom left). One
//! operation for each thing the engine's appearance stream paints.

use epdf_fonts::StandardFont;
use serde::Serialize;

use crate::PdfRect;

/// A colour as the stream paints it: red, green and blue from 0 to 1.
pub type Rgb = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum PathSegment {
    Rect { x: f64, y: f64, width: f64, height: f64 },
    Move { x: f64, y: f64 },
    Line { x: f64, y: f64 },
    Curve { x1: f64, y1: f64, x2: f64, y2: f64, x: f64, y: f64 },
    Close,
}

/// A run of text on one baseline, each character after the one before.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TextRun {
    /// Where the baseline starts.
    pub x: f64,
    pub y: f64,
    /// The characters as drawn (a password field's are `*`).
    pub text: String,
    /// Each character's advance, in points.
    pub advances: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum DrawOp {
    Fill {
        path: Vec<PathSegment>,
        color: Rgb,
        #[serde(rename = "evenOdd")]
        even_odd: bool,
    },
    Stroke {
        path: Vec<PathSegment>,
        color: Rgb,
        width: f64,
        dash: Option<Vec<f64>>,
    },
    Clip {
        rect: PdfRect,
        ops: Vec<DrawOp>,
    },
    Text {
        font: StandardFont,
        size: f64,
        color: Rgb,
        runs: Vec<TextRun>,
    },
}

/// Why a drawing may differ from the engine's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Approximation {
    /// The widget's font isn't one of the 14: it lays out with Helvetica's
    /// numbers.
    Font,
    /// Symbol or ZapfDingbats: the engine writes their text through another
    /// encoding.
    SymbolicFont,
    /// A character its font can't write: the engine draws it in another font.
    Glyph,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WidgetDrawing {
    /// The upright frame's size, in points: the widget's box.
    pub width: f64,
    pub height: f64,
    pub ops: Vec<DrawOp>,
    /// Why the drawing may differ from the engine's picture; `None` when it
    /// doesn't.
    pub approximate: Option<Approximation>,
}
