//! The wire union.
//!
//! G7. Ports `core/src/annotation/kinds/*/dto.ts`, one variant at a time.
//!
//! An annotation on the wire is the base shell plus exactly the fields its
//! subtype has, flattened into one object and discriminated by `subtype`. The
//! shell is not nested under a key, because the union's consumers read
//! `annotation.rect` and `annotation.subtype` side by side and always have.

use serde::{Deserialize, Serialize};

use super::base::AnnotationBase;

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
/// One variant so far. The others land with their readers, and until then
/// every subtype takes the [`AnnotationKind::Unsupported`] road, which is the
/// same road the TypeScript takes for a subtype it has no reader for. That is
/// the point of the variant: it is a forward-compatibility hatch in the wire
/// format, not a defeat, and a client already handles it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "subtype", rename_all = "kebab-case")]
#[serde(rename_all_fields = "camelCase")]
pub enum AnnotationKind {
    Unsupported {
        /// `FPDFAnnot_GetSubtype`'s code, kept for diagnosis.
        raw_subtype_code: i32,
        /// `/Subtype` as the file spells it, when it can be read. The code
        /// says what PDFium made of the name; this says what the name was,
        /// and for a subtype PDFium also does not know the two disagree.
        raw_subtype_name: Option<String>,
    },
}
