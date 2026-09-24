//! `/PieceInfo`: private, application-scoped data on a page or the catalog.
//!
//! G8. Wraps the fork's `EPDFDoc_*PieceInfo*` and `EPDFDoc_*PagePieceInfo*`
//! families.
//!
//! The two families are the same twenty-six calls with a page object number
//! inserted, so this layer folds them into one shape behind [`PieceInfoHolder`]
//! and every caller writes the operation once. That is not cosmetic: the
//! TypeScript branches on `pon === undefined` at each of thirteen call sites,
//! and a branch written the wrong way round at one of them would read the
//! catalog's data while claiming a page's.

use std::ffi::CString;

use crate::{Document, PdfiumError};

/// Whose `/PieceInfo` a call is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceInfoHolder {
    /// The document catalog.
    Catalog,
    /// One page, by its indirect object number.
    Page(u32),
}

/// `FPDF_OBJECT_*` codes, as the value-type accessors report them.
pub mod object_type {
    pub const BOOLEAN: i32 = 1;
    pub const NUMBER: i32 = 2;
    pub const STRING: i32 = 3;
    pub const NAME: i32 = 4;
    pub const ARRAY: i32 = 5;
}

/// Turns a Rust string into a NUL-terminated byte string.
///
/// An application name or a `/PieceInfo` key containing a NUL is refused
/// rather than truncated: a truncated key would address a DIFFERENT entry,
/// silently.
fn c_str(what: &'static str, value: &str) -> Result<CString, PdfiumError> {
    CString::new(value).map_err(|_| PdfiumError::InvalidArgument(what))
}

impl Document {
    /// Whether the holder has an entry for this application.
    pub fn has_piece_info(
        &self,
        holder: PieceInfoHolder,
        application: &str,
    ) -> Result<bool, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        // SAFETY: the handle is live and the name outlives the call.
        let has = unsafe {
            match holder {
                PieceInfoHolder::Catalog => {
                    pdfium_sys::EPDFDoc_HasPieceInfoEntry(self.as_raw(), application.as_ptr())
                }
                PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_HasPagePieceInfoEntry(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                ),
            }
        };
        Ok(has != 0)
    }

    /// How many applications the holder carries data for.
    pub fn piece_info_application_count(&self, holder: PieceInfoHolder) -> u32 {
        // SAFETY: the handle is live.
        let count = unsafe {
            match holder {
                PieceInfoHolder::Catalog => {
                    pdfium_sys::EPDFDoc_GetPieceInfoEntryCount(self.as_raw())
                }
                PieceInfoHolder::Page(page) => {
                    pdfium_sys::EPDFDoc_GetPagePieceInfoEntryCount(self.as_raw(), page)
                }
            }
        };
        u32::try_from(count).unwrap_or(0)
    }

    /// One application name, by position.
    pub fn piece_info_application_at(
        &self,
        holder: PieceInfoHolder,
        index: u32,
    ) -> Result<Option<String>, PdfiumError> {
        let Ok(index) = i32::try_from(index) else {
            return Ok(None);
        };
        read_utf8(|buffer, len| {
            // SAFETY: the handle is live and `len` describes `buffer`.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => {
                        pdfium_sys::EPDFDoc_GetPieceInfoEntryAt(self.as_raw(), index, buffer, len)
                    }
                    PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoEntryAt(
                        self.as_raw(),
                        page,
                        index,
                        buffer,
                        len,
                    ),
                }
            }
        })
    }

    /// How many keys one application's `/Private` dictionary holds.
    pub fn piece_info_key_count(
        &self,
        holder: PieceInfoHolder,
        application: &str,
    ) -> Result<u32, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        // SAFETY: the handle is live and the name outlives the call.
        let count = unsafe {
            match holder {
                PieceInfoHolder::Catalog => {
                    pdfium_sys::EPDFDoc_GetPieceInfoKeyCount(self.as_raw(), application.as_ptr())
                }
                PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoKeyCount(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                ),
            }
        };
        Ok(u32::try_from(count).unwrap_or(0))
    }

    /// One key, by position.
    pub fn piece_info_key_at(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        index: u32,
    ) -> Result<Option<String>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let Ok(index) = i32::try_from(index) else {
            return Ok(None);
        };
        read_utf8(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoKeyAt(
                        self.as_raw(),
                        application.as_ptr(),
                        index,
                        buffer,
                        len,
                    ),
                    PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoKeyAt(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        index,
                        buffer,
                        len,
                    ),
                }
            }
        })
    }

    /// The `FPDF_OBJECT_*` code of one value.
    pub fn piece_info_value_type(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<i32, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        // SAFETY: as above.
        Ok(unsafe {
            match holder {
                PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoValueType(
                    self.as_raw(),
                    application.as_ptr(),
                    key.as_ptr(),
                ),
                PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoValueType(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                    key.as_ptr(),
                ),
            }
        })
    }

    /// A text-string value.
    pub fn piece_info_string(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<Option<String>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoString(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        buffer,
                        len,
                    ),
                    PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoString(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                        buffer,
                        len,
                    ),
                }
            }
        })
    }

    /// A NAME value, which is a byte string and not a text one.
    pub fn piece_info_name(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<Option<String>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        read_utf8(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoName(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        buffer,
                        len,
                    ),
                    PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoName(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                        buffer,
                        len,
                    ),
                }
            }
        })
    }

    /// A number value.
    ///
    /// `f32`, because that is what the accessor reports, and the engine
    /// widens rather than rounding: a PDF real is single precision here
    /// whatever the file wrote.
    pub fn piece_info_number(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<Option<f32>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        let mut value = 0f32;
        // SAFETY: as above, and `value` is a valid out-parameter.
        let ok = unsafe {
            match holder {
                PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoNumber(
                    self.as_raw(),
                    application.as_ptr(),
                    key.as_ptr(),
                    &raw mut value,
                ),
                PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoNumber(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                    key.as_ptr(),
                    &raw mut value,
                ),
            }
        };
        Ok((ok != 0).then_some(value))
    }

    /// A boolean value.
    pub fn piece_info_boolean(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<Option<bool>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        let mut value = 0i32;
        // SAFETY: as above.
        let ok = unsafe {
            match holder {
                PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoBoolean(
                    self.as_raw(),
                    application.as_ptr(),
                    key.as_ptr(),
                    &raw mut value,
                ),
                PieceInfoHolder::Page(page) => pdfium_sys::EPDFDoc_GetPagePieceInfoBoolean(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                    key.as_ptr(),
                    &raw mut value,
                ),
            }
        };
        Ok((ok != 0).then_some(value != 0))
    }

    /// How many strings an ARRAY value holds.
    ///
    /// Negative means the array is not a pure list of text strings, which is
    /// a value this vocabulary does not model. The engine turns that into
    /// `unknown`; it is reported here rather than clamped, because zero
    /// would mean an empty array and that is a different thing.
    pub fn piece_info_string_array_count(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
    ) -> Result<i32, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        // SAFETY: as above.
        Ok(unsafe {
            match holder {
                PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoStringArrayCount(
                    self.as_raw(),
                    application.as_ptr(),
                    key.as_ptr(),
                ),
                PieceInfoHolder::Page(page) => {
                    pdfium_sys::EPDFDoc_GetPagePieceInfoStringArrayCount(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                    )
                }
            }
        })
    }

    /// One string of an ARRAY value.
    pub fn piece_info_string_array_at(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
        index: u32,
    ) -> Result<Option<String>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        let key = c_str("piece-info key contains NUL", key)?;
        let Ok(index) = i32::try_from(index) else {
            return Ok(None);
        };
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoStringArrayAt(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        index,
                        buffer,
                        len,
                    ),
                    PieceInfoHolder::Page(page) => {
                        pdfium_sys::EPDFDoc_GetPagePieceInfoStringArrayAt(
                            self.as_raw(),
                            page,
                            application.as_ptr(),
                            key.as_ptr(),
                            index,
                            buffer,
                            len,
                        )
                    }
                }
            }
        })
    }

    /// The application dictionary's `/LastModified`, as the PDF date string.
    pub fn piece_info_last_modified(
        &self,
        holder: PieceInfoHolder,
        application: &str,
    ) -> Result<Option<String>, PdfiumError> {
        let application = c_str("piece-info application contains NUL", application)?;
        read_utf16(|buffer, len| {
            // SAFETY: as above.
            unsafe {
                match holder {
                    PieceInfoHolder::Catalog => pdfium_sys::EPDFDoc_GetPieceInfoLastModified(
                        self.as_raw(),
                        application.as_ptr(),
                        buffer,
                        len,
                    ),
                    PieceInfoHolder::Page(page) => {
                        pdfium_sys::EPDFDoc_GetPagePieceInfoLastModified(
                            self.as_raw(),
                            page,
                            application.as_ptr(),
                            buffer,
                            len,
                        )
                    }
                }
            }
        })
    }
}

/// Measure then fill, for a UTF-16 accessor whose length is in bytes and
/// counts the terminator.
fn read_utf16<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut u16, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 2 {
        return Ok(None);
    }
    let units = (bytes as usize).div_ceil(2);
    let mut buffer: Vec<u16> = Vec::new();
    buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(units, 0u16);

    let written = call(buffer.as_mut_ptr(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
}

/// The same for a UTF-8 accessor.
fn read_utf8<F>(mut call: F) -> Result<Option<String>, PdfiumError>
where
    F: FnMut(*mut std::os::raw::c_char, std::os::raw::c_ulong) -> std::os::raw::c_ulong,
{
    let bytes = call(std::ptr::null_mut(), 0);
    if bytes <= 1 {
        return Ok(None);
    }
    let mut buffer: Vec<u8> = Vec::new();
    buffer.try_reserve_exact(bytes as usize).map_err(|_| PdfiumError::OutOfMemory)?;
    buffer.resize(bytes as usize, 0u8);

    let written = call(buffer.as_mut_ptr().cast(), bytes);
    if written == 0 {
        return Ok(None);
    }
    let produced = (written as usize).saturating_sub(1).min(buffer.len());
    Ok(Some(String::from_utf8_lossy(&buffer[..produced]).into_owned()))
}
