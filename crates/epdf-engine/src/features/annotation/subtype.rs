//! The closed-world subtype catalog.
//!
//! G7. Ports `core/src/annotation/subtype.ts`.
//!
//! PDFium's `FPDF_ANNOTATION_SUBTYPE` is an open integer: a newer build can
//! add codes, and a document can carry a subtype nobody has implemented. The
//! wire vocabulary is closed, so the two are joined by a map with a
//! designated escape hatch. An unmapped code becomes
//! [`AnnotationSubtype::Unsupported`] and the raw code travels with it, which
//! is what keeps an unknown annotation visible on the wire instead of
//! vanishing between a read and a write.

use serde::{Deserialize, Serialize};

/// A subtype the v3 wire format knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnnotationSubtype {
    Text,
    Link,
    FreeText,
    Line,
    Square,
    Circle,
    Polygon,
    Polyline,
    Highlight,
    Underline,
    Squiggly,
    Strikeout,
    Stamp,
    Caret,
    Ink,
    Popup,
    FileAttachment,
    Redact,
    Widget,
    /// Any PDF subtype this build has no reader for. Never produced by
    /// [`AnnotationSubtype::code`] as anything but `UNKNOWN`.
    Unsupported,
}

/// Every subtype, in the order `ANNOTATION_SUBTYPES` declares them. The
/// declaration order is the wire catalog's order, so the vector comparison
/// covers it rather than only the set.
pub const ANNOTATION_SUBTYPES: [AnnotationSubtype; 20] = [
    AnnotationSubtype::Text,
    AnnotationSubtype::Link,
    AnnotationSubtype::FreeText,
    AnnotationSubtype::Line,
    AnnotationSubtype::Square,
    AnnotationSubtype::Circle,
    AnnotationSubtype::Polygon,
    AnnotationSubtype::Polyline,
    AnnotationSubtype::Highlight,
    AnnotationSubtype::Underline,
    AnnotationSubtype::Squiggly,
    AnnotationSubtype::Strikeout,
    AnnotationSubtype::Stamp,
    AnnotationSubtype::Caret,
    AnnotationSubtype::Ink,
    AnnotationSubtype::Popup,
    AnnotationSubtype::FileAttachment,
    AnnotationSubtype::Redact,
    AnnotationSubtype::Widget,
    AnnotationSubtype::Unsupported,
];

/// `FPDF_ANNOTATION_SUBTYPE` codes, mirroring `public/fpdf_annot.h`.
///
/// The codes with no wire subtype are named too. They are exactly the values
/// [`AnnotationSubtype::from_code`] has to map to `Unsupported`, and naming
/// them is what makes "sound is not implemented" different from "nobody
/// checked whether 18 means anything".
pub mod code {
    pub const UNKNOWN: i32 = 0;
    pub const TEXT: i32 = 1;
    pub const LINK: i32 = 2;
    pub const FREETEXT: i32 = 3;
    pub const LINE: i32 = 4;
    pub const SQUARE: i32 = 5;
    pub const CIRCLE: i32 = 6;
    pub const POLYGON: i32 = 7;
    pub const POLYLINE: i32 = 8;
    pub const HIGHLIGHT: i32 = 9;
    pub const UNDERLINE: i32 = 10;
    pub const SQUIGGLY: i32 = 11;
    pub const STRIKEOUT: i32 = 12;
    pub const STAMP: i32 = 13;
    pub const CARET: i32 = 14;
    pub const INK: i32 = 15;
    pub const POPUP: i32 = 16;
    pub const FILEATTACHMENT: i32 = 17;
    pub const SOUND: i32 = 18;
    pub const MOVIE: i32 = 19;
    pub const WIDGET: i32 = 20;
    pub const SCREEN: i32 = 21;
    pub const PRINTERMARK: i32 = 22;
    pub const TRAPNET: i32 = 23;
    pub const WATERMARK: i32 = 24;
    pub const THREED: i32 = 25;
    pub const RICHMEDIA: i32 = 26;
    pub const XFAWIDGET: i32 = 27;
    pub const REDACT: i32 = 28;
}

impl AnnotationSubtype {
    /// Decode one `FPDFAnnot_GetSubtype` result.
    pub fn from_code(value: i32) -> Self {
        match value {
            code::TEXT => Self::Text,
            code::LINK => Self::Link,
            code::FREETEXT => Self::FreeText,
            code::LINE => Self::Line,
            code::SQUARE => Self::Square,
            code::CIRCLE => Self::Circle,
            code::POLYGON => Self::Polygon,
            code::POLYLINE => Self::Polyline,
            code::HIGHLIGHT => Self::Highlight,
            code::UNDERLINE => Self::Underline,
            code::SQUIGGLY => Self::Squiggly,
            code::STRIKEOUT => Self::Strikeout,
            code::STAMP => Self::Stamp,
            code::CARET => Self::Caret,
            code::INK => Self::Ink,
            code::POPUP => Self::Popup,
            code::FILEATTACHMENT => Self::FileAttachment,
            code::WIDGET => Self::Widget,
            code::REDACT => Self::Redact,
            _ => Self::Unsupported,
        }
    }

    /// The code a write path passes back to PDFium.
    ///
    /// `Unsupported` answers `UNKNOWN`, which is not a round trip: the raw
    /// code an unsupported annotation carries lives on its DTO, not here.
    pub fn code(self) -> i32 {
        match self {
            Self::Text => code::TEXT,
            Self::Link => code::LINK,
            Self::FreeText => code::FREETEXT,
            Self::Line => code::LINE,
            Self::Square => code::SQUARE,
            Self::Circle => code::CIRCLE,
            Self::Polygon => code::POLYGON,
            Self::Polyline => code::POLYLINE,
            Self::Highlight => code::HIGHLIGHT,
            Self::Underline => code::UNDERLINE,
            Self::Squiggly => code::SQUIGGLY,
            Self::Strikeout => code::STRIKEOUT,
            Self::Stamp => code::STAMP,
            Self::Caret => code::CARET,
            Self::Ink => code::INK,
            Self::Popup => code::POPUP,
            Self::FileAttachment => code::FILEATTACHMENT,
            Self::Redact => code::REDACT,
            Self::Widget => code::WIDGET,
            Self::Unsupported => code::UNKNOWN,
        }
    }

    /// The wire spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Link => "link",
            Self::FreeText => "free-text",
            Self::Line => "line",
            Self::Square => "square",
            Self::Circle => "circle",
            Self::Polygon => "polygon",
            Self::Polyline => "polyline",
            Self::Highlight => "highlight",
            Self::Underline => "underline",
            Self::Squiggly => "squiggly",
            Self::Strikeout => "strikeout",
            Self::Stamp => "stamp",
            Self::Caret => "caret",
            Self::Ink => "ink",
            Self::Popup => "popup",
            Self::FileAttachment => "file-attachment",
            Self::Redact => "redact",
            Self::Widget => "widget",
            Self::Unsupported => "unsupported",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_pdfium_defines_but_this_build_cannot_read_are_unsupported() {
        for unread in [
            code::SOUND,
            code::MOVIE,
            code::SCREEN,
            code::PRINTERMARK,
            code::TRAPNET,
            code::WATERMARK,
            code::THREED,
            code::RICHMEDIA,
            code::XFAWIDGET,
        ] {
            assert_eq!(AnnotationSubtype::from_code(unread), AnnotationSubtype::Unsupported);
        }
        // And so is a code no PDFium has issued, which is the case that keeps
        // a future fork from making a document unreadable.
        assert_eq!(AnnotationSubtype::from_code(9_000), AnnotationSubtype::Unsupported);
        assert_eq!(AnnotationSubtype::from_code(-1), AnnotationSubtype::Unsupported);
    }

    #[test]
    fn every_readable_subtype_round_trips_through_its_code() {
        for subtype in ANNOTATION_SUBTYPES {
            if subtype == AnnotationSubtype::Unsupported {
                continue;
            }
            assert_eq!(AnnotationSubtype::from_code(subtype.code()), subtype, "{subtype:?}");
        }
        // The one that does not, stated rather than left to the loop's skip.
        assert_eq!(AnnotationSubtype::Unsupported.code(), code::UNKNOWN);
    }

    #[test]
    fn the_wire_spelling_is_what_serde_emits() {
        for subtype in ANNOTATION_SUBTYPES {
            assert_eq!(
                serde_json::to_value(subtype).unwrap(),
                serde_json::json!(subtype.as_str()),
                "{subtype:?}",
            );
        }
    }
}
