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

/// A standard face's program as a scene's font program (`standardFontProgram`):
/// its members print as the TypeScript object does, `load` a function.
#[derive(Debug)]
pub struct StandardSceneProgram(pub &'static epdf_fonts::FontProgram);

impl crate::types::SceneFontProgram for StandardSceneProgram {
    fn members(&self) -> crate::json::Object {
        let mut members = match serde_json::to_value(self.0) {
            Ok(serde_json::Value::Object(members)) => members,
            _ => crate::json::Object::new(),
        };
        members.insert("load".into(), serde_json::json!({ "$function": true }));
        members
    }
    fn family(&self) -> &str {
        &self.0.family
    }
    fn weight(&self) -> f64 {
        f64::from(self.0.weight)
    }
    fn style(&self) -> &str {
        match self.0.style {
            epdf_fonts::FontStyle::Normal => "normal",
            epdf_fonts::FontStyle::Italic => "italic",
        }
    }
    fn load(&self) -> &[u8] {
        self.0.load()
    }
}

/// A mountable program (`core-fonts`) as a scene's font program: its members
/// print as the TypeScript object does, `load` a function. A program the host
/// has draws no bytes here; whatever mounts it reads them through
/// [`epdf_fonts::MountableFontProgram::load`].
impl crate::types::SceneFontProgram for epdf_fonts::MountableFontProgram {
    fn members(&self) -> crate::json::Object {
        let mut members = match serde_json::to_value(self) {
            Ok(serde_json::Value::Object(members)) => members,
            _ => crate::json::Object::new(),
        };
        members.insert("load".into(), serde_json::json!({ "$function": true }));
        members
    }
    fn family(&self) -> &str {
        &self.family
    }
    fn weight(&self) -> f64 {
        f64::from(self.weight)
    }
    fn style(&self) -> &str {
        match self.style {
            epdf_fonts::FontStyle::Normal => "normal",
            epdf_fonts::FontStyle::Italic => "italic",
        }
    }
    fn load(&self) -> &[u8] {
        self.built_in_bytes().unwrap_or_default()
    }
}

/// What `standardPaintFont(font)` paints with: its CSS family list and its program.
pub struct StandardPaintFont {
    pub font_family: String,
    pub program: crate::types::FontProgramRef,
}

/// `standardPaintFont(font)` (`@embedpdf/core-text`), its CSS and program.
pub fn standard_paint_font(font: epdf_fonts::StandardFont) -> StandardPaintFont {
    let css = epdf_fonts::standard_font_css(font);
    StandardPaintFont {
        font_family: css.font_family,
        program: crate::types::FontProgramRef(std::rc::Rc::new(StandardSceneProgram(
            epdf_fonts::standard_font_program(font),
        ))),
    }
}

/// `standardPaintFont(standardFontOf(font))`: the font of a box, drawn in its
/// standard font, Helvetica for any other.
pub fn standard_paint_font_of(font: &str) -> StandardPaintFont {
    standard_paint_font(
        epdf_fonts::StandardFont::from_name(font).unwrap_or(epdf_fonts::StandardFont::Helvetica),
    )
}

/// `standardFontCss(standardFontOf(font))` as its object prints: the CSS
/// family list, weight and style, and the programs (`load` a function).
pub fn standard_font_css_value(font: &str) -> serde_json::Value {
    let standard =
        epdf_fonts::StandardFont::from_name(font).unwrap_or(epdf_fonts::StandardFont::Helvetica);
    let css = epdf_fonts::standard_font_css(standard);
    let program = StandardSceneProgram(epdf_fonts::standard_font_program(standard));
    serde_json::json!({
        "fontFamily": css.font_family,
        "fontWeight": css.font_weight,
        "fontStyle": css.font_style,
        "programs": [crate::types::SceneFontProgram::members(&program)],
    })
}
