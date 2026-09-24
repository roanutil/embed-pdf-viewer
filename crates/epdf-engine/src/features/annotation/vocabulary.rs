//! The integer codecs: PDFium enum codes to the wire vocabulary.
//!
//! G7. Ports `services/src/features/annotations/internal/{blendMode,colorType,
//! lineEnding,shapeBorderStyle,standardFont,textAlignment,replyType,
//! annotationIcon,stampName}.ts`.
//!
//! They live together because they are one shape of problem: a PDFium enum
//! whose numbering is the fork's `public/fpdf_annot.h` and whose names are the
//! wire's. Each decoder is total, because PDFium's codes are an open set and a
//! build that gains a value must not make a document unreadable. What each one
//! does with a value it does not know is the interesting part, and it differs:
//! a blend mode falls back to `normal`, a line ending to `none`, an icon to
//! nothing at all so its reader can supply a subtype-specific default.

use serde::{Deserialize, Serialize};

/// `FPDFANNOT_COLORTYPE`, the selector that says which color a read is asking
/// for. Not a wire value: it only ever travels into PDFium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationColorType {
    /// `/C`, the stroke or fill.
    Color = 0,
    /// `/IC`, the interior of a square, circle or polygon.
    InteriorColor = 1,
    /// The redaction overlay.
    OverlayColor = 2,
    /// The free-text `/DA` color.
    TextColor = 3,
}

/// The effective blend mode of an annotation's `/AP /N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl BlendMode {
    /// Every mode, in `FPDF_BLENDMODE` order, which is also the wire union's.
    pub const ALL: [Self; 16] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];

    /// Decode a `FPDF_BLENDMODE`. Anything unrecognized, code 0 included, is
    /// `Normal`: that is the PDF default and the only safe guess, since a
    /// wrong mode changes what the page looks like.
    pub fn from_code(code: i32) -> Self {
        Self::ALL.get(usize::try_from(code).unwrap_or(usize::MAX)).copied().unwrap_or(Self::Normal)
    }

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
            Self::ColorDodge => "color-dodge",
            Self::ColorBurn => "color-burn",
            Self::HardLight => "hard-light",
            Self::SoftLight => "soft-light",
            Self::Difference => "difference",
            Self::Exclusion => "exclusion",
            Self::Hue => "hue",
            Self::Saturation => "saturation",
            Self::Color => "color",
            Self::Luminosity => "luminosity",
        }
    }
}

/// A `/LE` line ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineEnding {
    None,
    Square,
    Circle,
    Diamond,
    OpenArrow,
    ClosedArrow,
    Butt,
    ROpenArrow,
    RClosedArrow,
    Slash,
}

impl LineEnding {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::Square,
        Self::Circle,
        Self::Diamond,
        Self::OpenArrow,
        Self::ClosedArrow,
        Self::Butt,
        Self::ROpenArrow,
        Self::RClosedArrow,
        Self::Slash,
    ];

    /// Decode a `FPDF_ANNOT_LINE_END`.
    ///
    /// Code 10 is that enum's `Unknown`, which has no wire spelling and so
    /// reads back as `None` alongside every out-of-range value.
    pub fn from_code(code: i32) -> Self {
        Self::ALL.get(usize::try_from(code).unwrap_or(usize::MAX)).copied().unwrap_or(Self::None)
    }

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Square => "square",
            Self::Circle => "circle",
            Self::Diamond => "diamond",
            Self::OpenArrow => "open-arrow",
            Self::ClosedArrow => "closed-arrow",
            Self::Butt => "butt",
            Self::ROpenArrow => "r-open-arrow",
            Self::RClosedArrow => "r-closed-arrow",
            Self::Slash => "slash",
        }
    }
}

/// The `/LE` pair a line or polyline carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEndings {
    pub start: LineEnding,
    pub end: LineEnding,
}

/// The `/BS /S` subset PDFium can author.
///
/// Cloudy is deliberately absent. `FPDF_ANNOT_BORDER_STYLE` has a `CLOUDY`
/// code, but a cloudy border is the `/BE` border effect and travels as the
/// shape DTO's `cloudyIntensity`, so decoding it here would give one document
/// property two wire homes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationBorderStyle {
    Solid,
    Dashed,
    Beveled,
    Inset,
}

impl AnnotationBorderStyle {
    pub const ALL: [Self; 4] = [Self::Solid, Self::Dashed, Self::Beveled, Self::Inset];

    /// Decode a `FPDF_ANNOT_BORDER_STYLE`. `UNKNOWN`, `UNDERLINE` and `CLOUDY`
    /// all read as `Solid`, which is §8.4.3.3's default.
    pub fn from_code(code: i32) -> Self {
        match code {
            2 => Self::Dashed,
            3 => Self::Beveled,
            4 => Self::Inset,
            _ => Self::Solid,
        }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::Solid => 1,
            Self::Dashed => 2,
            Self::Beveled => 3,
            Self::Inset => 4,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solid => "solid",
            Self::Dashed => "dashed",
            Self::Beveled => "beveled",
            Self::Inset => "inset",
        }
    }
}

/// One of the 14 standard PDF fonts a `/DA` can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

/// What a missing or unrecognized `/DA` font reads as.
pub const DEFAULT_STANDARD_FONT: StandardFont = StandardFont::Helvetica;

impl StandardFont {
    pub const ALL: [Self; 14] = [
        Self::Courier,
        Self::CourierBold,
        Self::CourierBoldOblique,
        Self::CourierOblique,
        Self::Helvetica,
        Self::HelveticaBold,
        Self::HelveticaBoldOblique,
        Self::HelveticaOblique,
        Self::TimesRoman,
        Self::TimesBold,
        Self::TimesBoldItalic,
        Self::TimesItalic,
        Self::Symbol,
        Self::ZapfDingbats,
    ];

    /// Decode a `FPDF_STANDARD_FONT`.
    pub fn from_code(code: i32) -> Self {
        Self::ALL
            .get(usize::try_from(code).unwrap_or(usize::MAX))
            .copied()
            .unwrap_or(DEFAULT_STANDARD_FONT)
    }

    pub fn code(self) -> i32 {
        self as i32
    }

    /// Narrow a free-text `fontFamily` to a standard font.
    ///
    /// `None` means the value names a font registered through `fonts`. The 14
    /// standard names are therefore reserved: a font registered under the key
    /// `helvetica` resolves here, as the standard font, and never reaches the
    /// registry.
    pub fn from_family(family: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|font| font.as_str() == family)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Courier => "courier",
            Self::CourierBold => "courier-bold",
            Self::CourierBoldOblique => "courier-bold-oblique",
            Self::CourierOblique => "courier-oblique",
            Self::Helvetica => "helvetica",
            Self::HelveticaBold => "helvetica-bold",
            Self::HelveticaBoldOblique => "helvetica-bold-oblique",
            Self::HelveticaOblique => "helvetica-oblique",
            Self::TimesRoman => "times-roman",
            Self::TimesBold => "times-bold",
            Self::TimesBoldItalic => "times-bold-italic",
            Self::TimesItalic => "times-italic",
            Self::Symbol => "symbol",
            Self::ZapfDingbats => "zapf-dingbats",
        }
    }
}

/// `/Q` quadding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

impl TextAlignment {
    pub const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    /// Decode a `/Q`. Anything outside 0..=2 is `Left`, §12.7.3.3's default.
    pub fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Center,
            2 => Self::Right,
            _ => Self::Left,
        }
    }

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// The `/RT` relationship toward the annotation `/IRT` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationReplyType {
    Reply,
    Group,
}

/// `FPDF_ANNOT_REPLY_TYPE` codes.
pub const RT_UNKNOWN: i32 = 0;
pub const RT_REPLY: i32 = 1;
pub const RT_GROUP: i32 = 2;

impl AnnotationReplyType {
    pub const ALL: [Self; 2] = [Self::Reply, Self::Group];

    /// Decode a `FPDF_ANNOT_REPLY_TYPE`, GATED on `/IRT` being present.
    ///
    /// `EPDFAnnot_GetReplyType` answers `RT_REPLY` for an annotation with no
    /// `/IRT` at all, because it reports the `/RT` default in isolation. So
    /// this cannot distinguish "replies to nothing" from "replies"; the caller
    /// must have found an `/IRT` before asking.
    pub fn from_code(code: i32) -> Self {
        if code == RT_GROUP { Self::Group } else { Self::Reply }
    }

    pub fn code(self) -> i32 {
        match self {
            Self::Reply => RT_REPLY,
            Self::Group => RT_GROUP,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reply => "reply",
            Self::Group => "group",
        }
    }
}

/// A text annotation's `/Name` icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NoteIcon {
    Comment,
    Key,
    Note,
    Help,
    NewParagraph,
    Paragraph,
    Insert,
}

/// What an absent or foreign text `/Name` reads as (§12.5.6.4).
pub const DEFAULT_NOTE_ICON: NoteIcon = NoteIcon::Note;

impl NoteIcon {
    pub const ALL: [Self; 7] = [
        Self::Comment,
        Self::Key,
        Self::Note,
        Self::Help,
        Self::NewParagraph,
        Self::Paragraph,
        Self::Insert,
    ];

    /// Decode a `FPDF_ANNOT_NAME`, or `None` for a code outside the note
    /// block. The reader substitutes [`DEFAULT_NOTE_ICON`]; the distinction is
    /// kept here so a file-attachment code reaching a text annotation is
    /// visible as a miss rather than silently becoming `note` inside the map.
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code).ok()?).copied()
    }

    pub fn code(self) -> i32 {
        self as i32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Comment => "comment",
            Self::Key => "key",
            Self::Note => "note",
            Self::Help => "help",
            Self::NewParagraph => "new-paragraph",
            Self::Paragraph => "paragraph",
            Self::Insert => "insert",
        }
    }
}

/// A file-attachment annotation's `/Name` icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileAttachmentIcon {
    Graph,
    PushPin,
    Paperclip,
    Tag,
}

/// What an absent or foreign file-attachment `/Name` reads as (§12.5.6.15).
pub const DEFAULT_FILE_ATTACHMENT_ICON: FileAttachmentIcon = FileAttachmentIcon::PushPin;

impl FileAttachmentIcon {
    pub const ALL: [Self; 4] = [Self::Graph, Self::PushPin, Self::Paperclip, Self::Tag];

    /// Decode a `FPDF_ANNOT_NAME`. The file-attachment block starts at 7,
    /// straight after the note block's seven.
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.get(usize::try_from(code - 7).ok()?).copied()
    }

    pub fn code(self) -> i32 {
        self as i32 + 7
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::PushPin => "push-pin",
            Self::Paperclip => "paperclip",
            Self::Tag => "tag",
        }
    }
}

/// The stamp `/Name` labels `FPDF_ANNOT_NAME` enumerates, code 13 upward.
///
/// Literal PDF names rather than a wire vocabulary, because §12.5.6.12 Table
/// 181 and Adobe's extended set are already the names a stamp carries, and a
/// kebab-case translation would have to be undone on write. A stamp whose
/// `/Name` is not in this table is not an error: its reader falls back to the
/// raw string entry, which is how a custom stamp survives.
pub const STAMP_NAMES: [(&str, i32); 23] = [
    ("Approved", 13),
    ("Experimental", 14),
    ("NotApproved", 15),
    ("AsIs", 16),
    ("Expired", 17),
    ("NotForPublicRelease", 18),
    ("Confidential", 19),
    ("Final", 20),
    ("Sold", 21),
    ("Departmental", 22),
    ("ForComment", 23),
    ("TopSecret", 24),
    ("Draft", 25),
    ("ForPublicRelease", 26),
    ("Completed", 27),
    ("Void", 28),
    ("PreliminaryResults", 29),
    ("InformationOnly", 30),
    ("Rejected", 31),
    ("Witness", 32),
    ("InitialHere", 33),
    ("SignHere", 34),
    ("Accepted", 35),
];

/// The label for a stamp `/Name` code, or `None` for a custom stamp.
pub fn stamp_name_from_code(code: i32) -> Option<&'static str> {
    STAMP_NAMES.iter().find(|(_, c)| *c == code).map(|(name, _)| *name)
}

/// The code for a stamp label, or `None` if the fork's enum has no entry.
pub fn stamp_name_to_code(name: &str) -> Option<i32> {
    STAMP_NAMES.iter().find(|(n, _)| *n == name).map(|(_, code)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_round_trips_within_its_own_enum() {
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::from_code(mode.code()), mode);
        }
        for ending in LineEnding::ALL {
            assert_eq!(LineEnding::from_code(ending.code()), ending);
        }
        for style in AnnotationBorderStyle::ALL {
            assert_eq!(AnnotationBorderStyle::from_code(style.code()), style);
        }
        for font in StandardFont::ALL {
            assert_eq!(StandardFont::from_code(font.code()), font);
            assert_eq!(StandardFont::from_family(font.as_str()), Some(font));
        }
        for align in TextAlignment::ALL {
            assert_eq!(TextAlignment::from_code(align.code()), align);
        }
        for reply in AnnotationReplyType::ALL {
            assert_eq!(AnnotationReplyType::from_code(reply.code()), reply);
        }
        for icon in NoteIcon::ALL {
            assert_eq!(NoteIcon::from_code(icon.code()), Some(icon));
        }
        for icon in FileAttachmentIcon::ALL {
            assert_eq!(FileAttachmentIcon::from_code(icon.code()), Some(icon));
        }
        for (name, code) in STAMP_NAMES {
            assert_eq!(stamp_name_from_code(code), Some(name));
            assert_eq!(stamp_name_to_code(name), Some(code));
        }
    }

    #[test]
    fn a_negative_code_does_not_wrap_into_a_valid_value() {
        // usize::try_from on a negative i32 is where a sloppy cast would turn
        // -1 into the last entry of the table.
        assert_eq!(BlendMode::from_code(-1), BlendMode::Normal);
        assert_eq!(LineEnding::from_code(-1), LineEnding::None);
        assert_eq!(StandardFont::from_code(-1), DEFAULT_STANDARD_FONT);
        assert_eq!(NoteIcon::from_code(-1), None);
        assert_eq!(FileAttachmentIcon::from_code(-1), None);
        assert_eq!(FileAttachmentIcon::from_code(6), None);
        assert_eq!(stamp_name_from_code(-1), None);
    }

    #[test]
    fn the_two_icon_blocks_do_not_overlap() {
        // The note block is 0..=6 and the file block is 7..=10. A code from
        // one must miss in the other, which is what makes the `Option` worth
        // having.
        for icon in NoteIcon::ALL {
            assert_eq!(FileAttachmentIcon::from_code(icon.code()), None, "{icon:?}");
        }
        for icon in FileAttachmentIcon::ALL {
            assert_eq!(NoteIcon::from_code(icon.code()), None, "{icon:?}");
        }
        // And neither reaches the stamp block, which starts at 13.
        assert_eq!(NoteIcon::from_code(13), None);
        assert_eq!(FileAttachmentIcon::from_code(13), None);
    }

    #[test]
    fn the_border_styles_pdfium_names_but_the_wire_does_not_read_as_solid() {
        for unmapped in [0, 5, 6] {
            assert_eq!(
                AnnotationBorderStyle::from_code(unmapped),
                AnnotationBorderStyle::Solid,
                "{unmapped}",
            );
        }
    }

    #[test]
    fn a_registered_font_key_is_not_a_standard_font() {
        assert_eq!(StandardFont::from_family("Inter"), None);
        assert_eq!(StandardFont::from_family("Helvetica"), None);
        assert_eq!(StandardFont::from_family(""), None);
    }
}
