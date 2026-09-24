//! The wire union.
//!
//! G7. Ports `core/src/annotation/kinds/*/dto.ts`.
//!
//! An annotation on the wire is the base shell plus exactly the fields its
//! subtype has, flattened into one object and discriminated by `subtype`. The
//! shell is not nested under a key, because the union's consumers read
//! `annotation.rect` and `annotation.subtype` side by side and always have.
//!
//! Variants that share a shape share a struct rather than repeating it: the
//! four text-markup subtypes differ only in their discriminator, and square
//! and circle only in theirs. That is the same factoring the TypeScript uses,
//! through `TextMarkupDTO<S>` and `ShapeDTO<S>`, and it is why a field added
//! to one of those families cannot be added to three of its four members.

use serde::{Deserialize, Serialize};

use super::base::AnnotationBase;
use super::caret::CaretFields;
use super::file_attachment::FileAttachmentFields;
use super::free_text::FreeTextFields;
use super::ink_line::{InkFields, LineFields};
use super::link::LinkFields;
use super::redact::RedactFields;
use super::shape::ShapeFields;
use super::stamp::StampFields;
use super::text::TextFields;
use super::text_markup::TextMarkupFields;
use super::vertex::VertexFields;
use super::widget::WidgetFields;
use crate::features::annotation::intent::StrikeoutIntent;
use crate::features::annotation::vocabulary::LineEndings;

/// One annotation, ready for the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotationDto {
    #[serde(flatten)]
    pub base: AnnotationBase,
    #[serde(flatten)]
    pub kind: AnnotationKind,
}

/// The subtype-specific half.
///
/// [`AnnotationKind::Unsupported`] is not a defeat. It is the wire format's
/// forward-compatibility hatch, produced for any subtype this build has no
/// reader for, and a client already handles it because a PDF can carry a
/// subtype no engine knows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum AnnotationKind {
    Highlight {
        #[serde(flatten)]
        fields: TextMarkupFields,
    },
    Underline {
        #[serde(flatten)]
        fields: TextMarkupFields,
    },
    Squiggly {
        #[serde(flatten)]
        fields: TextMarkupFields,
    },
    Strikeout {
        #[serde(flatten)]
        fields: TextMarkupFields,
        /// `/IT`. `Some` only when this strikeout is the subordinate half of
        /// a replace-text edit, which is why it is on this variant and not on
        /// the shared fields.
        intent: Option<StrikeoutIntent>,
    },
    Square {
        #[serde(flatten)]
        fields: ShapeFields,
    },
    Circle {
        #[serde(flatten)]
        fields: ShapeFields,
    },
    Polygon {
        #[serde(flatten)]
        fields: VertexFields,
        /// `/BE`. Polygon has it and polyline does not: an open path has no
        /// interior for a cloudy border to enclose.
        cloudy_intensity: Option<f64>,
    },
    Polyline {
        #[serde(flatten)]
        fields: VertexFields,
        line_endings: LineEndings,
    },
    Line {
        #[serde(flatten)]
        fields: LineFields,
    },
    Ink {
        #[serde(flatten)]
        fields: InkFields,
    },
    Text {
        #[serde(flatten)]
        fields: TextFields,
    },
    FreeText {
        #[serde(flatten)]
        fields: FreeTextFields,
    },
    Stamp {
        #[serde(flatten)]
        fields: StampFields,
    },
    Caret {
        #[serde(flatten)]
        fields: CaretFields,
    },
    Redact {
        #[serde(flatten)]
        fields: RedactFields,
    },
    Link {
        #[serde(flatten)]
        fields: LinkFields,
    },
    FileAttachment {
        #[serde(flatten)]
        fields: FileAttachmentFields,
    },
    Widget {
        #[serde(flatten)]
        fields: WidgetFields,
    },
    Unsupported {
        /// `FPDFAnnot_GetSubtype`'s code, kept for diagnosis.
        raw_subtype_code: i32,
        /// `/Subtype` as the file spells it, when it can be read. The code
        /// says what PDFium made of the name; this says what the name was,
        /// and for a subtype PDFium also does not know the two disagree.
        raw_subtype_name: Option<String>,
    },
}
