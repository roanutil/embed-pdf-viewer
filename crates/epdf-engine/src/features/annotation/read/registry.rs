//! Which reader handles which subtype.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/annotationReaderRegistry.ts`.
//!
//! A `match` with an `unsupported` arm, which is the same shape as the
//! TypeScript's partial map with an `unsupported` fallback. The difference is
//! only which entries are filled in: every subtype without an arm here takes
//! the fallback, [`FALLS_BACK_TO_UNSUPPORTED`] says which those are, and
//! `crates/epdf-engine/tests/annotation_reader.rs` asserts that list against
//! the catalog. So a reader that lands without being registered fails, and so
//! does one registered without the comparison being widened to its fields.

use pdfium::Annotation;

use super::caret::read_caret;
use super::dto::AnnotationKind;
use super::free_text::read_free_text;
use super::ink_line::{read_ink, read_line, read_line_endings};
use super::redact::read_redact;
use super::shape::read_shape;
use super::stamp::read_stamp;
use super::text::read_text;
use super::text_markup::{DEFAULT_HIGHLIGHT_COLOR, DEFAULT_TEXT_MARKUP_COLOR, read_text_markup};
use super::unsupported::read_unsupported;
use super::vertex::read_vertex;
use super::{dictionary_read, read_cloudy_intensity};
use crate::error::EngineError;
use crate::features::annotation::intent::StrikeoutIntent;
use crate::features::annotation::subtype::AnnotationSubtype;

/// The subtypes this build has no dedicated reader for.
///
/// Every one of them is still readable as a base shell; what is missing is the
/// subtype's own fields. `unsupported` is not listed, because for it the
/// fallback IS the reader.
pub const FALLS_BACK_TO_UNSUPPORTED: [AnnotationSubtype; 4] = [
    AnnotationSubtype::Link,
    // Popup is on this list in the TypeScript too, and not because nobody got
    // to it: a popup is the presentation of another annotation's comment, and
    // the wire deliberately does not model it. `/Popup` and `/Open` are a
    // viewer's business.
    AnnotationSubtype::Popup,
    AnnotationSubtype::FileAttachment,
    AnnotationSubtype::Widget,
];

/// Read the subtype-specific half of an annotation.
pub fn read_annotation_kind(
    annotation: &Annotation<'_>,
    raw_subtype_code: i32,
) -> Result<AnnotationKind, EngineError> {
    Ok(match AnnotationSubtype::from_code(raw_subtype_code) {
        AnnotationSubtype::Highlight => AnnotationKind::Highlight {
            fields: read_text_markup(annotation, DEFAULT_HIGHLIGHT_COLOR)?,
        },
        AnnotationSubtype::Underline => AnnotationKind::Underline {
            fields: read_text_markup(annotation, DEFAULT_TEXT_MARKUP_COLOR)?,
        },
        AnnotationSubtype::Squiggly => AnnotationKind::Squiggly {
            fields: read_text_markup(annotation, DEFAULT_TEXT_MARKUP_COLOR)?,
        },
        AnnotationSubtype::Strikeout => AnnotationKind::Strikeout {
            fields: read_text_markup(annotation, DEFAULT_TEXT_MARKUP_COLOR)?,
            intent: StrikeoutIntent::from_name(
                dictionary_read("IT", annotation.intent())?.as_deref(),
            ),
        },
        AnnotationSubtype::Square => AnnotationKind::Square { fields: read_shape(annotation)? },
        AnnotationSubtype::Circle => AnnotationKind::Circle { fields: read_shape(annotation)? },
        AnnotationSubtype::Polygon => AnnotationKind::Polygon {
            fields: read_vertex(annotation)?,
            cloudy_intensity: read_cloudy_intensity(annotation),
        },
        AnnotationSubtype::Polyline => AnnotationKind::Polyline {
            fields: read_vertex(annotation)?,
            line_endings: read_line_endings(annotation),
        },
        AnnotationSubtype::Line => AnnotationKind::Line { fields: read_line(annotation)? },
        AnnotationSubtype::Ink => AnnotationKind::Ink { fields: read_ink(annotation)? },
        AnnotationSubtype::Text => AnnotationKind::Text { fields: read_text(annotation)? },
        AnnotationSubtype::FreeText => {
            AnnotationKind::FreeText { fields: read_free_text(annotation)? }
        }
        AnnotationSubtype::Stamp => AnnotationKind::Stamp { fields: read_stamp(annotation)? },
        AnnotationSubtype::Caret => AnnotationKind::Caret { fields: read_caret(annotation)? },
        AnnotationSubtype::Redact => AnnotationKind::Redact { fields: read_redact(annotation)? },
        _ => read_unsupported(annotation, raw_subtype_code)?,
    })
}
