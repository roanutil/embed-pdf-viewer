//! `font-css.ts`: the CSS font family the browser draws an annotation's font
//! with. A standard font gets a web stack with its metrics; any other font
//! is a registered key, drawn in the face mounted for it.

use crate::engine::text::standard_face;

/// `STANDARD_FONT_STACKS[family]`.
pub fn standard_font_stack(family: &str) -> Option<&'static str> {
    Some(match family {
        "Helvetica" => "Helvetica, Arial, sans-serif",
        "Times" => "\"Times New Roman\", Times, serif",
        "Courier" => "\"Courier New\", Courier, monospace",
        "Symbol" | "ZapfDingbats" => "serif",
        _ => return None,
    })
}

/// `mountedFontFamily(key)`: the family a registered font's key is mounted under.
pub fn mounted_font_family(key: &str) -> String {
    format!("\"epdf-{key}\"")
}

/// `cssFontFamilyForFont(font)`.
pub fn css_font_family_for_font(font: &str) -> String {
    match standard_face(font).and_then(|face| standard_font_stack(face.family)) {
        Some(stack) => stack.into(),
        None => format!("{}, sans-serif", mounted_font_family(font)),
    }
}
