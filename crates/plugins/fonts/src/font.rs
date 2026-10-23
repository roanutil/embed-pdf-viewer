//! The 14 names and their faces: `engine-core`'s `STANDARD_FONTS` and
//! `STANDARD_FACES` (`packages/engine/core/src/annotation/primitives.ts`,
//! `fontFaces.ts`). `epdf-engine` has the same table, but it links PDFium and
//! can't build for wasm32, where the form painter that reads these runs.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// One of the 14 standard PDF fonts, by its kebab-case name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StandardFont {
    Courier,
    CourierBold,
    CourierBoldOblique,
    CourierOblique,
    Helvetica,
    HelveticaBold,
    HelveticaBoldOblique,
    HelveticaOblique,
    TimesRoman,
    TimesBold,
    TimesBoldItalic,
    TimesItalic,
    Symbol,
    ZapfDingbats,
}

/// A standard face's family: the PDF name, not a CSS one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StandardFamily {
    Helvetica,
    Times,
    Courier,
    Symbol,
    ZapfDingbats,
}

/// A CSS font style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontStyle {
    Normal,
    Italic,
}

/// What a standard name means: its family, weight and italic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StandardFace {
    pub family: StandardFamily,
    pub weight: u32,
    pub italic: bool,
}

/// A name that isn't one of the 14 (they compare exactly: `"Helvetica"` isn't one).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownStandardFont(pub String);

impl fmt::Display for UnknownStandardFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a standard font: {:?}", self.0)
    }
}

impl std::error::Error for UnknownStandardFont {}

impl StandardFont {
    /// `STANDARD_FONTS`, in its order.
    pub const ALL: [StandardFont; 14] = [
        StandardFont::Courier,
        StandardFont::CourierBold,
        StandardFont::CourierBoldOblique,
        StandardFont::CourierOblique,
        StandardFont::Helvetica,
        StandardFont::HelveticaBold,
        StandardFont::HelveticaBoldOblique,
        StandardFont::HelveticaOblique,
        StandardFont::TimesRoman,
        StandardFont::TimesBold,
        StandardFont::TimesBoldItalic,
        StandardFont::TimesItalic,
        StandardFont::Symbol,
        StandardFont::ZapfDingbats,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StandardFont::Courier => "courier",
            StandardFont::CourierBold => "courier-bold",
            StandardFont::CourierBoldOblique => "courier-bold-oblique",
            StandardFont::CourierOblique => "courier-oblique",
            StandardFont::Helvetica => "helvetica",
            StandardFont::HelveticaBold => "helvetica-bold",
            StandardFont::HelveticaBoldOblique => "helvetica-bold-oblique",
            StandardFont::HelveticaOblique => "helvetica-oblique",
            StandardFont::TimesRoman => "times-roman",
            StandardFont::TimesBold => "times-bold",
            StandardFont::TimesBoldItalic => "times-bold-italic",
            StandardFont::TimesItalic => "times-italic",
            StandardFont::Symbol => "symbol",
            StandardFont::ZapfDingbats => "zapf-dingbats",
        }
    }

    /// The font a name is, exactly as `isStandardFontName` matches it.
    pub fn from_name(name: &str) -> Option<StandardFont> {
        StandardFont::ALL.into_iter().find(|font| font.as_str() == name)
    }

    /// `STANDARD_FACES[font]`.
    pub fn face(self) -> StandardFace {
        use StandardFamily as F;
        let (family, weight, italic) = match self {
            StandardFont::Courier => (F::Courier, 400, false),
            StandardFont::CourierBold => (F::Courier, 700, false),
            StandardFont::CourierBoldOblique => (F::Courier, 700, true),
            StandardFont::CourierOblique => (F::Courier, 400, true),
            StandardFont::Helvetica => (F::Helvetica, 400, false),
            StandardFont::HelveticaBold => (F::Helvetica, 700, false),
            StandardFont::HelveticaBoldOblique => (F::Helvetica, 700, true),
            StandardFont::HelveticaOblique => (F::Helvetica, 400, true),
            StandardFont::TimesRoman => (F::Times, 400, false),
            StandardFont::TimesBold => (F::Times, 700, false),
            StandardFont::TimesBoldItalic => (F::Times, 700, true),
            StandardFont::TimesItalic => (F::Times, 400, true),
            StandardFont::Symbol => (F::Symbol, 400, false),
            StandardFont::ZapfDingbats => (F::ZapfDingbats, 400, false),
        };
        StandardFace { family, weight, italic }
    }
}

impl fmt::Display for StandardFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StandardFont {
    type Err = UnknownStandardFont;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        StandardFont::from_name(name).ok_or_else(|| UnknownStandardFont(name.to_string()))
    }
}
