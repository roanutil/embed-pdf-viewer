//! `standard/css.ts`: a standard face as CSS, its program's family first,
//! then the platform's look-alikes for the moment before the program is
//! mounted.

use serde::Serialize;

use crate::{FontStyle, StandardFamily, StandardFont, standard_font_program};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StandardFontCss {
    pub font_family: String,
    pub font_weight: u32,
    pub font_style: FontStyle,
}

/// `STACKS`.
fn stack(family: StandardFamily) -> &'static str {
    match family {
        StandardFamily::Helvetica => "Helvetica, Arial, sans-serif",
        StandardFamily::Times => "\"Times New Roman\", Times, serif",
        StandardFamily::Courier => "\"Courier New\", Courier, monospace",
        StandardFamily::Symbol => "Symbol, serif",
        StandardFamily::ZapfDingbats => "\"Zapf Dingbats\", ZapfDingbats, serif",
    }
}

pub fn standard_font_css(font: StandardFont) -> StandardFontCss {
    let program = standard_font_program(font);
    StandardFontCss {
        font_family: format!("\"{}\", {}", program.family, stack(font.face().family)),
        font_weight: program.weight,
        font_style: program.style,
    }
}
