//! What a registered font's program is, and the last-resort font.
//!
//! The reads `FontRegistrar.readProgram` and `FontRegistrar.programFor` make
//! (`engine/services/src/features/fonts/FontRegistrar.ts`), and the
//! last-resort registration its constructor makes. Facts only, as the fork
//! reports them: rounding the metrics to thousandths and naming the format
//! are the engine's.
//!
//! Every call here reads PDFium's thread-local font registry, so a
//! [`FontId`] means something only on the thread that registered it.

use crate::{FontId, PdfiumError};

/// `EPDF_FONT_FORMAT_*`: what a font program is.
///
/// `EPDF_FONT_FORMAT_NONE` (the PDF only names the font) is not a variant:
/// the readers answer `None` for it, as the TypeScript maps it to `null`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontProgramFormat {
    /// TrueType outlines (sfnt).
    TrueType,
    /// OpenType with CFF outlines (sfnt).
    OpenTypeCff,
    /// Bare CFF (`/FontFile3 /Type1C` and the like).
    Cff,
    /// Type 1 (`/FontFile`).
    Type1,
    /// Glyphs drawn by content streams.
    Type3,
}

impl FontProgramFormat {
    /// The format an `EPDF_FONT_FORMAT_*` code names; `None` for
    /// `EPDF_FONT_FORMAT_NONE`, -1 and any code the fork does not define.
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            1 => Some(Self::TrueType),
            2 => Some(Self::OpenTypeCff),
            3 => Some(Self::Cff),
            4 => Some(Self::Type1),
            5 => Some(Self::Type3),
            _ => None,
        }
    }
}

/// A run of code points a font maps: `start` and the `count` that follow.
///
/// `CodePointRange` in `engine/core`, and the `(first, count)` pairs of
/// `uint32_t` the fork writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodePointRange {
    pub start: u32,
    pub count: u32,
}

/// A program's line metrics as the layout reads them, in em: the hhea
/// ascender and descender over units per em, descent positive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontLineMetrics {
    pub ascent: f32,
    pub descent: f32,
}

/// Make a registered font the last resort, or clear it with `None`.
///
/// New text draws a character no font or fallback has with the last resort,
/// so it shows as that font's box and the text keeps it. False for an id the
/// fork does not know.
pub fn set_last_resort_font(id: Option<FontId>) -> bool {
    let raw = id.map_or(0, FontId::raw);
    // SAFETY: takes an id only; an unknown one answers false.
    unsafe { pdfium_sys::EPDFFont_SetLastResortFont(raw) != 0 }
}

/// The bytes the writer embeds for a registered font (the static instance
/// for a variable font). `None` for an id the fork does not know.
pub fn font_program_data(id: FontId) -> Result<Option<Vec<u8>>, PdfiumError> {
    // SAFETY: a null buffer with length zero asks for the length only.
    let length = unsafe { pdfium_sys::EPDFFont_GetProgramData(id.raw(), std::ptr::null_mut(), 0) };
    if length == 0 {
        return Ok(None);
    }
    let mut buffer = zeroed_bytes(length)?;
    // SAFETY: `buffer` holds `length` bytes and lives for the call.
    let written = unsafe {
        pdfium_sys::EPDFFont_GetProgramData(id.raw(), buffer.as_mut_ptr().cast(), length)
    };
    // The fork copies nothing when the buffer is short, and the length it
    // reported cannot have grown in between on this thread.
    if written != length {
        return Ok(None);
    }
    Ok(Some(buffer))
}

/// What a registered font's program is. `None` for an id the fork does not
/// know.
pub fn font_program_format(id: FontId) -> Option<FontProgramFormat> {
    // SAFETY: takes an id only; an unknown one answers -1.
    FontProgramFormat::from_code(unsafe { pdfium_sys::EPDFFont_GetProgramFormat(id.raw()) })
}

/// The SHA-256 of [`font_program_data`]'s bytes. `None` for an id the fork
/// does not know.
pub fn font_program_hash(id: FontId) -> Option<[u8; 32]> {
    let mut hash = [0u8; 32];
    // SAFETY: `hash` is the 32 bytes the call writes.
    let ok = unsafe { pdfium_sys::EPDFFont_GetProgramHash(id.raw(), hash.as_mut_ptr()) };
    (ok != 0).then_some(hash)
}

/// The characters a registered font maps. Empty for an id the fork does not
/// know.
pub fn font_coverage(id: FontId) -> Result<Vec<CodePointRange>, PdfiumError> {
    read_ranges(|out, capacity| {
        // SAFETY: `out` holds `capacity` pairs, or is null with zero.
        unsafe { pdfium_sys::EPDFFont_GetCoverage(id.raw(), out, capacity) }
    })
}

/// A registered font's line metrics. `None` for an id the fork does not know.
pub fn font_metrics(id: FontId) -> Option<FontLineMetrics> {
    let (mut ascent, mut descent) = (0f32, 0f32);
    // SAFETY: both pointers name live floats for the call.
    let ok = unsafe { pdfium_sys::EPDFFont_GetMetrics(id.raw(), &mut ascent, &mut descent) };
    (ok != 0).then_some(FontLineMetrics { ascent, descent })
}

/// A registered font's PostScript name, as its embedded `/BaseFont` spells
/// it. `None` for an id the fork does not know.
pub fn font_postscript_name(id: FontId) -> Result<Option<String>, PdfiumError> {
    read_utf8(|buffer, length| {
        // SAFETY: `buffer` holds `length` bytes, or is null with zero.
        unsafe { pdfium_sys::EPDFFont_GetPostScriptName(id.raw(), buffer, length) }
    })
}

/// A zeroed buffer of a length the fork reported, allocated fallibly.
pub(crate) fn zeroed_bytes(length: std::os::raw::c_ulong) -> Result<Vec<u8>, PdfiumError> {
    let length = usize::try_from(length).map_err(|_| PdfiumError::OutOfMemory)?;
    let mut buffer: Vec<u8> = Vec::new();
    buffer.try_reserve_exact(length).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(length, 0);
    Ok(buffer)
}

/// Measure then fill, for a getter of `(first, count)` pairs that returns how
/// many there are. `readRanges` in `engine/services`.
pub(crate) fn read_ranges<F>(mut call: F) -> Result<Vec<CodePointRange>, PdfiumError>
where
    F: FnMut(*mut u32, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let count = call(std::ptr::null_mut(), 0);
    if count == 0 {
        return Ok(Vec::new());
    }
    let pairs = usize::try_from(count).map_err(|_| PdfiumError::OutOfMemory)?;
    let words = pairs.checked_mul(2).ok_or(PdfiumError::OutOfMemory)?;
    let mut buffer: Vec<u32> = Vec::new();
    buffer.try_reserve_exact(words).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(words, 0);
    call(buffer.as_mut_ptr(), count);
    Ok(buffer
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[start, count]| CodePointRange { start, count })
        .collect())
}

/// Measure then fill, for a UTF-8 accessor whose length counts the NUL.
/// `Some("")` for a lone NUL, as the TypeScript's `readUtf8String` answers;
/// its callers fold that to `null` themselves.
pub(crate) fn read_utf8<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut std::os::raw::c_char, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let length = call(std::ptr::null_mut(), 0);
    if length == 0 {
        return Ok(None);
    }
    let mut buffer = zeroed_bytes(length)?;
    let written = call(buffer.as_mut_ptr().cast(), length);
    if written == 0 {
        return Ok(None);
    }
    let produced = usize::try_from(written).unwrap_or(0).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf8_lossy(&buffer[..produced]).into_owned()))
}

/// Measure then fill, for a UTF-16LE accessor whose byte length counts the
/// NUL. `Some("")` for an empty string (a length of 2), as the TypeScript's
/// `readUtf16String` answers.
pub(crate) fn read_utf16<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes == 0 {
        return Ok(None);
    }
    let units = usize::try_from(bytes).map_err(|_| PdfiumError::OutOfMemory)?.div_ceil(2);
    let mut buffer: Vec<u16> = Vec::new();
    buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(units, 0);
    let written = call(buffer.as_mut_ptr(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = (usize::try_from(written).unwrap_or(0) / 2).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
}
