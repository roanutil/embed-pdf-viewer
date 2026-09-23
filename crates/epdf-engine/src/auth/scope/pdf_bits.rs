//! ISO 32000 user-access permission bits.
//!
//! A transcription of `packages/engine/core/src/auth/scope/pdf-bits.ts`.

use super::types::PdfBits;

/// Bit positions in the spec are 1-indexed, so bit 3 is `1 << 2`.
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

/// Decode a raw permission word.
///
/// `None` is the unprobed column and yields all-false, which is the
/// restrictive reading: an unprobed document grants nothing through
/// `pdf.permissions` rather than everything.
///
/// The parameter is `i64` where TypeScript takes `number`, because the value
/// arrives from PDFium as a 32-bit word that both a JSON number and a
/// signed C int can carry. `FPDF_GetDocPermissions` returns `0xFFFFFFFF` for
/// an unencrypted document, and a host that routed that through a signed
/// integer hands over `-1`. Truncating to `u32` makes those the same word,
/// which is what TypeScript's `&` on a double already does.
pub fn decode_pdf_bits(raw: Option<i64>) -> PdfBits {
    let Some(raw) = raw else {
        return PdfBits::default();
    };
    let raw = raw as u32;
    PdfBits {
        bit3: raw & mask::PRINT != 0,
        bit4: raw & mask::MODIFY != 0,
        bit5: raw & mask::COPY != 0,
        bit6: raw & mask::ANNOTATE_FILL != 0,
        bit9: raw & mask::FILL_FORMS != 0,
        bit10: raw & mask::ACCESSIBILITY != 0,
        bit11: raw & mask::ASSEMBLE != 0,
        bit12: raw & mask::PRINT_HIGH != 0,
    }
}
