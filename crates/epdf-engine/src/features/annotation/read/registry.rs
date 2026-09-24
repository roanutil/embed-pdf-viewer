//! Which reader handles which subtype.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/annotationReaderRegistry.ts`.
//!
//! The TypeScript table is a partial map with an `unsupported` fallback, and
//! so is this one. The difference is only which entries are filled in: this
//! build has none yet, so every subtype falls through, and each subtype reader
//! that lands moves one row out of [`FALLS_BACK_TO_UNSUPPORTED`] and into the
//! dispatch. That list is asserted by
//! `crates/epdf-engine/tests/annotation_reader.rs`, so the day a reader lands
//! without being registered, or is registered without the comparison being
//! widened, the test says which.

use pdfium::Annotation;

use super::dto::AnnotationKind;
use super::unsupported::read_unsupported;
use crate::error::EngineError;
use crate::features::annotation::subtype::AnnotationSubtype;

/// The subtypes this build has no dedicated reader for.
///
/// Every one of them is readable as a base shell; what is missing is the
/// subtype's own fields. `unsupported` itself is not listed, because for it
/// the fallback IS the reader.
pub const FALLS_BACK_TO_UNSUPPORTED: [AnnotationSubtype; 19] = [
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
];

/// Read the subtype-specific half of an annotation.
pub fn read_annotation_kind(
    annotation: &Annotation<'_>,
    raw_subtype_code: i32,
) -> Result<AnnotationKind, EngineError> {
    // Decoded and then discarded, for now. The decode is here rather than at
    // the one call site because this is where the dispatch will be, and
    // because a subtype the decoder maps to `Unsupported` and one this table
    // has no row for reach the same reader by different routes.
    let _subtype = AnnotationSubtype::from_code(raw_subtype_code);
    read_unsupported(annotation, raw_subtype_code)
}
