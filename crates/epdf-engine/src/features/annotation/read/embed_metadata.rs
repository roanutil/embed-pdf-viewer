//! `/EMBD_Metadata`, the dictionary only EmbedPDF writes.
//!
//! G7. Ports
//! `services/src/features/annotations/internal/read/readEmbedMetadata.ts`.
//!
//! Every field is optional and absence is ordinary, not an error: an
//! annotation from Acrobat has no such dictionary, and one written by an
//! anonymous local session has the dictionary with almost nothing in it.
//!
//! # Absent and empty are not distinguished here, and that is PDFium's doing
//!
//! `EPDFAnnot_GetEmbedMetadataString` reports a byte length that counts the
//! terminator, and it reports two, not zero, for a key the dictionary does not
//! have. So a missing `/GroupID` and a `/GroupID ()` both read as the empty
//! string. The TypeScript has the same behaviour for the same reason, and it
//! shows in the vectors: `annotation_base.pdf` sets `UserID` and `CreatedBy`
//! only, and the recorded read carries `groupId: ""` and `updatedBy: ""`.
//! Fixing it would need a presence probe the fork does not expose.

use pdfium::Annotation;

use super::dictionary_read;
use crate::error::EngineError;

/// The decoded dictionary. `None` from [`read_embed_metadata`] means the
/// annotation has no dictionary at all, which is different from having one
/// with nothing in it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmbedMetadata {
    pub schema_version: Option<f32>,
    pub user_id: Option<String>,
    pub group_id: Option<String>,
    pub created_by: Option<String>,
    pub updated_by: Option<String>,
}

/// Read the dictionary, or `None` when there is none.
pub fn read_embed_metadata(
    annotation: &Annotation<'_>,
) -> Result<Option<EmbedMetadata>, EngineError> {
    if !annotation.has_embed_metadata() {
        return Ok(None);
    }
    let number = |key: &str| dictionary_read(key, annotation.embed_metadata_number(key));
    let text = |key: &str| dictionary_read(key, annotation.embed_metadata_string(key));
    Ok(Some(EmbedMetadata {
        schema_version: number("SchemaVersion")?,
        user_id: text("UserID")?,
        group_id: text("GroupID")?,
        created_by: text("CreatedBy")?,
        updated_by: text("UpdatedBy")?,
    }))
}
