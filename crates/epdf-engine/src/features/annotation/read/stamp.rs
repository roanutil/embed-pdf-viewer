//! The stamp.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readStampAnnotation.ts`.
//!
//! The DTO carries no picture. A stamp's visual content is its `/AP` stream,
//! rendered like any other baked appearance, so what is on the wire is the
//! label and the rotation pair.
//!
//! The label is `/Name` as text (`EPDFAnnot_GetName`), a standard label or a
//! custom one alike, as `readAnnotName` reads it on the TypeScript side. A
//! custom stamp is ordinary, not an error.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::transform::{read_rotation, read_unrotated_rect};
use crate::error::EngineError;
use crate::geom::PdfRect;

/// A stamp annotation's own fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StampFields {
    /// `/Name`, the advisory label. The PDF's own spelling rather than a
    /// kebab-case wire word, because a stamp's label is already a name and
    /// translating it would have to be undone on write.
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unrotated_rect: Option<PdfRect>,
}

/// Read them.
pub fn read_stamp(annotation: &Annotation<'_>) -> Result<StampFields, EngineError> {
    Ok(StampFields {
        name: dictionary_read("Name", annotation.name())?,
        rotation: read_rotation(annotation)?,
        unrotated_rect: read_unrotated_rect(annotation)?,
    })
}
