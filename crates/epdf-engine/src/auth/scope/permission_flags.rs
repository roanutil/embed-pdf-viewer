//! ISO 32000 user-access permission flags.
//!
//! A transcription of `packages/engine/core/src/auth/scope/permission-flags.ts`.

use super::types::PdfPermissionFlags;

/// Bit positions in the spec are 1-indexed, so bit 3 is `1 << 2`
/// (`PDF_BITS`).
///
/// Bits 1, 2, 7, 8 and 13 upward are reserved and deliberately not surfaced.
pub mod mask {
    /// Bit 3, print.
    pub const PRINT: u32 = 1 << 2;
    /// Bit 4, modify content streams.
    pub const MODIFY: u32 = 1 << 3;
    /// Bit 5, copy or extract text and graphics.
    pub const COPY: u32 = 1 << 4;
    /// Bit 6, annotate and fill forms.
    pub const ANNOTATE_FILL: u32 = 1 << 5;
    /// Bit 9, fill existing forms even when bit 6 is clear.
    pub const FILL_FORMS: u32 = 1 << 8;
    /// Bit 10, accessibility extraction. Deprecated in PDF 2.0.
    pub const ACCESSIBILITY: u32 = 1 << 9;
    /// Bit 11, assemble.
    pub const ASSEMBLE: u32 = 1 << 10;
    /// Bit 12, high-quality print, refining bit 3.
    pub const PRINT_HIGH: u32 = 1 << 11;
}

/// The flags of a `/P` word, as the file is read (`permissionFlagsOf`).
///
/// `None` is a document not probed yet and gives every flag `false`, the
/// most restrictive reading: an unprobed document grants nothing through
/// `pdf.permissions` rather than everything.
///
/// The parameter is `i64` where TypeScript takes `number`, because the value
/// arrives from PDFium as a 32-bit word that both a JSON number and a
/// signed C int can carry. `FPDF_GetDocPermissions` returns `0xFFFFFFFF` for
/// an unencrypted document, and a host that routed that through a signed
/// integer hands over `-1`. Truncating to `u32` makes those the same word,
/// which is what TypeScript's `&` on a double already does.
pub fn permission_flags_of(word: Option<i64>) -> PdfPermissionFlags {
    let Some(word) = word else {
        return PdfPermissionFlags::default();
    };
    let word = word as u32;
    PdfPermissionFlags {
        print: word & mask::PRINT != 0,
        modify_contents: word & mask::MODIFY != 0,
        copy: word & mask::COPY != 0,
        modify_annotations: word & mask::ANNOTATE_FILL != 0,
        fill_forms: word & mask::FILL_FORMS != 0,
        copy_for_accessibility: word & mask::ACCESSIBILITY != 0,
        assemble: word & mask::ASSEMBLE != 0,
        print_high_quality: word & mask::PRINT_HIGH != 0,
    }
}
