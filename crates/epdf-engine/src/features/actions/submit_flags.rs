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

/// The flags word, named. The engine reads and writes the bits; the API only
/// names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFormFlags {
    /// Bit 1. When set, `fields` lists the fields to EXCLUDE.
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

/// A write's flags (`Partial<SubmitFormFlags>`): each optional, off (or
/// `fdf`, `post`) when left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitFormFlagWrites {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_no_value_fields: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<SubmitFormat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<SubmitMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submit_coordinates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_append_saves: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_annotations: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_format: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excl_non_user_annots: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excl_f_key: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embed_form: Option<bool>,
}

impl From<SubmitFormFlags> for SubmitFormFlagWrites {
    fn from(flags: SubmitFormFlags) -> Self {
        Self {
            exclude: Some(flags.exclude),
            include_no_value_fields: Some(flags.include_no_value_fields),
            format: Some(flags.format),
            method: Some(flags.method),
            submit_coordinates: Some(flags.submit_coordinates),
            include_append_saves: Some(flags.include_append_saves),
            include_annotations: Some(flags.include_annotations),
            canonical_format: Some(flags.canonical_format),
            excl_non_user_annots: Some(flags.excl_non_user_annots),
            excl_f_key: Some(flags.excl_f_key),
            embed_form: Some(flags.embed_form),
        }
    }
}

/// The `/Flags` word for named flags (`encodeSubmitFormFlags`), the inverse
/// of [`decode_submit_form_flags`]: a flag left out is off, `format` is
/// `fdf` and `method` is `post`.
pub fn encode_submit_form_flags(flags: &SubmitFormFlagWrites) -> u32 {
    let on = |flag: Option<bool>| flag.unwrap_or(false);
    let bits: [(u32, bool); 13] = [
        (1, on(flags.exclude)),
        (2, on(flags.include_no_value_fields)),
        (3, flags.format == Some(SubmitFormat::Html)),
        (4, flags.method == Some(SubmitMethod::Get)),
        (5, on(flags.submit_coordinates)),
        (6, flags.format == Some(SubmitFormat::Xfdf)),
        (7, on(flags.include_append_saves)),
        (8, on(flags.include_annotations)),
        (9, flags.format == Some(SubmitFormat::Pdf)),
        (10, on(flags.canonical_format)),
        (11, on(flags.excl_non_user_annots)),
        (12, on(flags.excl_f_key)),
        (14, on(flags.embed_form)),
    ];
    bits.iter().fold(0, |word, &(bit, on)| if on { word | (1 << (bit - 1)) } else { word })
}
