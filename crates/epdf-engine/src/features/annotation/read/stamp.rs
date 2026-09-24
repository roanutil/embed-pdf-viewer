//! The stamp.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readStampAnnotation.ts`.
//!
//! The DTO carries no picture. A stamp's visual content is its `/AP` stream,
//! rendered like any other baked appearance, so what is on the wire is the
//! label and the rotation pair.
//!
//! The label is read by two routes. `FPDF_ANNOT_NAME` enumerates the
//! twenty-three standard labels, and a stamp whose `/Name` is not one of them
//! falls outside that enum entirely, so the raw entry is read instead. A
//! custom stamp is ordinary, not an error.
//!
//! On the fixtures here the two routes agree, and a version of this that
//! consulted only the raw entry passes every test. That is a real property
//! and not a gap the corpus could close: `FPDFAnnot_GetStringValue` reads the
//! name object's text for a standard label as readily as for a custom one, so
//! no `/Name` distinguishes them. The enum is consulted first anyway, because
//! it is the route the fork maintains and the raw read is the fallback, not
//! the other way round.

use pdfium::Annotation;
use serde::{Deserialize, Serialize};

use super::dictionary_read;
use super::transform::{read_rotation, read_unrotated_rect};
use crate::error::EngineError;
use crate::features::annotation::vocabulary::stamp_name_from_code;
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
    let name = match stamp_name_from_code(annotation.name_code()) {
        Some(known) => Some(known.to_owned()),
        None => dictionary_read("Name", annotation.string_value("Name"))?,
    };
    Ok(StampFields {
        name,
        rotation: read_rotation(annotation)?,
        unrotated_rect: read_unrotated_rect(annotation)?,
    })
}
