//! The SubmitForm `/Flags` word.
//!
//! G6. A transcription of `decodeSubmitFormFlags` in
//! `core/src/dto/PdfAction.ts`, which is ISO 32000-2:2020 Table 240.
//!
//! Pure, and the single highest-risk piece of the actions read: a bit read at
//! the wrong position produces a form submission that goes to the wrong place
//! in the wrong format, and nothing in the pipeline can tell. So it is pinned
//! to the TypeScript by vectors over every interesting word rather than
//! trusted to a careful reading.
//!
//! Two positions are easy to get wrong and are called out in the TypeScript
//! for that reason: bit 12 is `ExclFKey` rather than "ExclFDFTemplate", and
//! bit 13 is reserved, which puts `EmbedForm` at bit 14 rather than 13.

use serde::{Deserialize, Serialize};

/// Which wire format a submission uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubmitFormat {
    /// Table 240's default, when no format bit is set.
    Fdf,
    Html,
    Xfdf,
    Pdf,
}

/// Which HTTP method carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubmitMethod {
    Post,
    Get,
}

/// The decoded flags word, with the raw value retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFormFlags {
    pub raw: u32,
    /// Bit 1. When set, `fields` lists the fields to EXCLUDE.
    ///
    /// Derived here rather than stored separately, so it cannot disagree with
    /// `raw`.
    pub exclude: bool,
    /// Bit 2. Submit designated valueless fields as name-only entries.
    pub include_no_value_fields: bool,
    pub format: SubmitFormat,
    pub method: SubmitMethod,
    /// Bit 5. Meaningful only with an export format; passed through because
    /// this stack does not capture click coordinates.
    pub submit_coordinates: bool,
    /// Bit 7, FDF only. Include incremental updates as FDF `Differences`.
    pub include_append_saves: bool,
    /// Bit 8, FDF only. Include markup annotations.
    pub include_annotations: bool,
    /// Bit 10. Convert date-like values to the standard date format.
    pub canonical_format: bool,
    /// Bit 11, FDF with annotations. Only the current user's annotations.
    pub excl_non_user_annots: bool,
    /// Bit 12. The submitted FDF excludes its `F` entry.
    pub excl_f_key: bool,
    /// Bit 14. Bit 13 is reserved, which is why this is not 13.
    pub embed_form: bool,
}

/// Decode one `/Flags` word.
pub fn decode_submit_form_flags(raw: u32) -> SubmitFormFlags {
    // Positions are 1-indexed in the spec.
    let bit = |n: u32| raw & (1 << (n - 1)) != 0;

    // Bit 9 dominates: the spec says when SubmitPDF is set "all other flags
    // shall be ignored except GetMethod". Then XFDF, then HTML, then the
    // default. Checking them in another order silently reclassifies a
    // submission that set more than one.
    let format = if bit(9) {
        SubmitFormat::Pdf
    } else if bit(6) {
        SubmitFormat::Xfdf
    } else if bit(3) {
        SubmitFormat::Html
    } else {
        SubmitFormat::Fdf
    };

    // GetMethod is meaningful only with an export format, and bit 9's
    // exception keeps it alive for PDF. Everything else posts, whatever bit 4
    // says.
    let method = if bit(4) && matches!(format, SubmitFormat::Html | SubmitFormat::Pdf) {
        SubmitMethod::Get
    } else {
        SubmitMethod::Post
    };

    SubmitFormFlags {
        raw,
        exclude: bit(1),
        include_no_value_fields: bit(2),
        format,
        method,
        submit_coordinates: bit(5),
        include_append_saves: bit(7),
        include_annotations: bit(8),
        canonical_format: bit(10),
        excl_non_user_annots: bit(11),
        excl_f_key: bit(12),
        embed_form: bit(14),
    }
}
