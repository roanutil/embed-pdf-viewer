//! `standard/metrics.ts`: the numbers the engine lays text out with in a
//! standard font: each character's advance, and the ascent and descent. They
//! are the built-in face's own (PDFium draws a standard font a document
//! doesn't embed with it), not AFM's: the advances come from the face's
//! glyphs, the ascent and descent from its font box.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::{StandardFont, face};

#[derive(Debug)]
pub struct StandardFontMetrics {
    pub font: StandardFont,
    /// In 1/1000 em.
    pub ascent: i32,
    /// In 1/1000 em, below the baseline (negative).
    pub descent: i32,
    advances: HashMap<u32, u32>,
}

impl StandardFontMetrics {
    /// A character's advance in 1/1000 em; `None` when the face's encoding
    /// can't write it (the engine draws it with another font).
    pub fn advance(&self, code_point: u32) -> Option<u32> {
        self.advances.get(&code_point).copied()
    }
}

/// The face's metrics, built the first time they're asked for and the same
/// value every time after.
pub fn standard_font_metrics(font: StandardFont) -> &'static StandardFontMetrics {
    static CACHE: [OnceLock<StandardFontMetrics>; 14] = [const { OnceLock::new() }; 14];
    CACHE[font as usize].get_or_init(|| {
        let face = face(font);
        StandardFontMetrics {
            font,
            ascent: face.ascent,
            descent: face.descent,
            advances: face.code_points.iter().copied().zip(face.advances.iter().copied()).collect(),
        }
    })
}
