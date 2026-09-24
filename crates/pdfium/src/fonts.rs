//! The thread-local font registry.
//!
//! PDFium's font registry is per-thread: a font registered here is visible
//! only to this thread's rendering and annotation authoring, and the numeric
//! id it returns is a per-thread counter. Both facts shape everything above
//! this module.
//!
//! The id is volatile, so it must never cross a boundary. What callers hold is
//! a stable key of their own choosing, and
//! `epdf_engine::features::fonts::FontRegistry` owns the key-to-id map.

use std::ffi::CString;

use crate::{FileAccess, PdfiumError};

/// A native font id, valid on the thread that registered it and nowhere else.
///
/// A newtype rather than a bare integer so it cannot be serialized by
/// accident: an id that reached another thread would name a different font or
/// no font at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FontId(u32);

impl FontId {
    /// PDFium reports failure as zero rather than as an error.
    fn from_raw(raw: u32) -> Option<Self> {
        (raw != 0).then_some(Self(raw))
    }

    /// The raw id, for the `EPDFFont_*` calls that take one.
    pub fn raw(self) -> u32 {
        self.0
    }
}

/// How a family name, weight and slant are described to PDFium.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontDescription {
    /// Empty means "take it from the file".
    pub family_name: String,
    /// Zero means "take it from the file".
    pub weight: i32,
    /// `None` infers from the file, which is what the TypeScript spells `-1`.
    pub italic: Option<bool>,
}

impl FontDescription {
    /// Everything inferred from the font file itself.
    pub fn inferred() -> Self {
        Self { family_name: String::new(), weight: 0, italic: None }
    }

    fn italic_code(&self) -> i32 {
        match self.italic {
            None => -1,
            Some(true) => 1,
            Some(false) => 0,
        }
    }
}

/// Register a font from bytes.
///
/// PDFium copies the bytes into its own registry, so the caller's buffer is
/// free immediately afterwards. That is the difference from
/// [`register_font_file`], where the file is read on demand and its handle has
/// to stay alive.
pub fn register_memory_font(
    description: &FontDescription,
    bytes: &[u8],
) -> Result<FontId, PdfiumError> {
    if bytes.is_empty() {
        return Err(PdfiumError::InvalidArgument("font data is empty"));
    }
    let family = CString::new(description.family_name.as_str())
        .map_err(|_| PdfiumError::InvalidArgument("font family contains NUL"))?;

    // SAFETY: the pointer and length describe `bytes`, live for the call, and
    // PDFium copies before returning.
    let raw = unsafe {
        pdfium_sys::EPDFFont_RegisterMemFont64(
            family.as_ptr(),
            description.weight,
            description.italic_code(),
            bytes.as_ptr() as *const std::ffi::c_void,
            bytes.len(),
        )
    };
    FontId::from_raw(raw).ok_or(PdfiumError::FontRegistration)
}

/// Register a font PDFium reads from a file on demand.
///
/// The access must outlive the registration: PDFium range-reads the file every
/// time it builds a face, not once at registration. Keeping it alive is the
/// caller's job, and `epdf_engine`'s registry does it by holding the handle
/// until the registry is cleared.
pub fn register_font_file(
    description: &FontDescription,
    access: &FileAccess,
) -> Result<FontId, PdfiumError> {
    let family = CString::new(description.family_name.as_str())
        .map_err(|_| PdfiumError::InvalidArgument("font family contains NUL"))?;

    // SAFETY: `access` is borrowed for the call and kept alive past it by the
    // caller, which is the contract this function's documentation states.
    let raw = unsafe {
        pdfium_sys::EPDFFont_RegisterFont(
            family.as_ptr(),
            description.weight,
            description.italic_code(),
            access.as_ptr(),
        )
    };
    FontId::from_raw(raw).ok_or(PdfiumError::FontRegistration)
}

/// Append a registered font to the ordered glyph-fallback chain.
pub fn add_fallback_font(id: FontId) -> Result<(), PdfiumError> {
    // SAFETY: `id` is non-zero by construction and names a font registered on
    // this thread.
    let ok = unsafe { pdfium_sys::EPDFFont_AddFallbackFont(id.raw()) };
    if ok == 0 { Err(PdfiumError::FontRegistration) } else { Ok(()) }
}

/// Empty the fallback chain, leaving the fonts registered.
pub fn clear_fallback_fonts() {
    unsafe { pdfium_sys::EPDFFont_ClearFallbackFonts() };
}

/// Unregister every font on this thread and reset the fallback chain.
pub fn clear_registered_fonts() {
    unsafe { pdfium_sys::EPDFFont_ClearRegisteredFonts() };
}
