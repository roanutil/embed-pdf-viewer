//! `font.ts`: a font as the engine's text layout reads it, a character's width
//! and the ascent and descent, all in 1/1000 em. The same three questions
//! PDFium's layout asks its font (`CPVT_VariableText::Provider`).
//!
//! Only the 14 standard fonts have their numbers, from the built-in faces the
//! engine draws them with (`epdf-fonts`). Any other font gives `None`, and a
//! widget in it draws approximately.

use epdf_fonts::{StandardFont, StandardFontMetrics, standard_font_metrics};

#[derive(Clone, Copy, Debug)]
pub struct LayoutFont {
    /// The standard font it is, as the engine draws it.
    pub font: StandardFont,
    /// In 1/1000 em.
    pub ascent: i32,
    /// In 1/1000 em, below the baseline (negative).
    pub descent: i32,
    metrics: &'static StandardFontMetrics,
}

impl LayoutFont {
    /// A UTF-16 code unit's width; `None` when the font can't write it (the
    /// engine then draws it in another font, so the layout is approximate).
    pub fn width(&self, code: u32) -> Option<u32> {
        self.metrics.advance(code)
    }
}

/// The layout numbers of a widget's `/DA` font; `None` for a font that isn't
/// one of the 14.
pub fn layout_font_of(font_family: Option<&str>) -> Option<LayoutFont> {
    let font = StandardFont::from_name(font_family.filter(|name| !name.is_empty())?)?;
    let metrics = standard_font_metrics(font);
    Some(LayoutFont { font, ascent: metrics.ascent, descent: metrics.descent, metrics })
}
