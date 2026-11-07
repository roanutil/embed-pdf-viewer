//! `drawing/types.ts`: what a widget's appearance draws, in the generator's
//! own terms: fills, strokes, a clip and runs of text, in the widget's upright
//! frame (PDF points, y up, the origin at the frame's bottom left). One
//! operation for each thing the engine's appearance stream paints.

use std::rc::Rc;

use serde::Serialize;

use crate::PdfRect;
use crate::text::PaintFont;

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

/// Text one font draws in one direction, on one baseline.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRun {
    /// Where its left edge meets the baseline.
    pub x: f64,
    pub y: f64,
    /// Its characters in typing order (a password field's are `*`).
    pub text: String,
    pub font: Rc<PaintFont>,
    /// Drawn right to left: its first character on the right.
    pub rtl: bool,
    /// Kerning and ligatures off: a simple script or a standard font, as the
    /// engine draws it.
    pub plain: bool,
    /// In points.
    pub width: f64,
    /// A comb cell's letter joined to the one before it, or after it (in
    /// typing order), as the engine shapes the whole text: drawn alone, it
    /// takes that joined form with a joiner (U+200D) on that side.
    pub joins_previous: bool,
    pub joins_next: bool,
}

#[derive(Clone, Debug, Serialize)]
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
        size: f64,
        color: Rgb,
        runs: Vec<TextRun>,
    },
}

/// Why a drawing may differ from the engine's picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Approximation {
    /// A font the painter wasn't given: one that isn't one of the 14 lays
    /// out with Helvetica's numbers.
    Font,
    /// Symbol or ZapfDingbats.
    SymbolicFont,
    /// A character drawn as a box, which a font the painter wasn't given may
    /// have.
    Glyph,
}

#[derive(Clone, Debug, Serialize)]
pub struct WidgetDrawing {
    /// The upright frame's size, in points: the widget's box.
    pub width: f64,
    pub height: f64,
    pub ops: Vec<DrawOp>,
    /// Why the drawing may differ from the engine's picture; `None` when it
    /// doesn't.
    pub approximate: Option<Approximation>,
}
