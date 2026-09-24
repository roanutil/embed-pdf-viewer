//! The reader for a subtype nobody claims.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readUnsupportedAnnotation.ts`.

use pdfium::Annotation;

use super::dictionary_read;
use super::dto::AnnotationKind;
use crate::error::EngineError;

/// Keep what is known about a subtype this build does not read.
///
/// Both the code and the name, because they answer different questions. The
/// code says what PDFium made of the `/Subtype`; the name says what the file
/// wrote. For a subtype PDFium also does not recognize the code is zero and
/// the name is the only thing left.
pub fn read_unsupported(
    annotation: &Annotation<'_>,
    raw_subtype_code: i32,
) -> Result<AnnotationKind, EngineError> {
    Ok(AnnotationKind::Unsupported {
        raw_subtype_code,
        raw_subtype_name: dictionary_read("Subtype", annotation.string_value("Subtype"))?,
    })
}
