//! Writing `/PieceInfo`.
//!
//! G14. The write half of [`crate::piece_info`], and it folds the same two
//! call families into one shape for the same reason: the catalog accessors
//! and the page accessors differ only by a page object number, and a branch
//! written the wrong way round would write one while claiming the other.
//!
//! # Every setter carries the document's modification date
//!
//! `/PieceInfo` entries are private application data, and §14.5 says a
//! consumer may discard them when the document has changed since the
//! application last wrote. That check is a comparison between the
//! application dictionary's `/LastModified` and the document's, so every
//! setter takes the document's date and stamps it. One date per job,
//! however many keys a patch touches: a batch written across two seconds
//! would otherwise look like two separate edits.

use std::ffi::CString;

use crate::{Document, PdfiumError, PieceInfoHolder};

/// A key and a value, as a write.
///
/// The array variant is the one that carries a list; the rest are scalars.
/// A `Clear` is a separate variant rather than an optional value, because
/// removing a key and setting it to nothing are different edits here, unlike
/// in `/Info`.
#[derive(Debug, Clone, PartialEq)]
pub enum PieceInfoValue {
    String(String),
    /// A PDF name, which is not a string: it is written with the name
    /// syntax and a reader that took it for text would quote it.
    Name(String),
    Number(f32),
    Boolean(bool),
    StringArray(Vec<String>),
    /// Remove the key.
    Clear,
}

impl Document {
    /// Write one `/PieceInfo` entry.
    ///
    /// `last_modified` is the document's date, as a PDF date string.
    pub fn set_piece_info(
        &self,
        holder: PieceInfoHolder,
        application: &str,
        key: &str,
        value: &PieceInfoValue,
        last_modified: &str,
    ) -> Result<bool, PdfiumError> {
        let application = c_string(application, "piece-info application")?;
        let key = c_string(key, "piece-info key")?;
        let modified = wide(last_modified);
        let page = holder_page(holder);

        // SAFETY: the handle is live, and every string and array outlives
        // the call.
        Ok(unsafe {
            match value {
                PieceInfoValue::String(text) => {
                    let text = wide(text);
                    match page {
                        None => pdfium_sys::EPDFDoc_SetPieceInfoString(
                            self.as_raw(),
                            application.as_ptr(),
                            key.as_ptr(),
                            text.as_ptr(),
                            modified.as_ptr(),
                        ),
                        Some(page) => pdfium_sys::EPDFDoc_SetPagePieceInfoString(
                            self.as_raw(),
                            page,
                            application.as_ptr(),
                            key.as_ptr(),
                            text.as_ptr(),
                            modified.as_ptr(),
                        ),
                    }
                }
                PieceInfoValue::Name(name) => {
                    let name = c_string(name, "piece-info name")?;
                    match page {
                        None => pdfium_sys::EPDFDoc_SetPieceInfoName(
                            self.as_raw(),
                            application.as_ptr(),
                            key.as_ptr(),
                            name.as_ptr(),
                            modified.as_ptr(),
                        ),
                        Some(page) => pdfium_sys::EPDFDoc_SetPagePieceInfoName(
                            self.as_raw(),
                            page,
                            application.as_ptr(),
                            key.as_ptr(),
                            name.as_ptr(),
                            modified.as_ptr(),
                        ),
                    }
                }
                PieceInfoValue::Number(number) => match page {
                    None => pdfium_sys::EPDFDoc_SetPieceInfoNumber(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        *number,
                        modified.as_ptr(),
                    ),
                    Some(page) => pdfium_sys::EPDFDoc_SetPagePieceInfoNumber(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                        *number,
                        modified.as_ptr(),
                    ),
                },
                PieceInfoValue::Boolean(flag) => match page {
                    None => pdfium_sys::EPDFDoc_SetPieceInfoBoolean(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        i32::from(*flag),
                        modified.as_ptr(),
                    ),
                    Some(page) => pdfium_sys::EPDFDoc_SetPagePieceInfoBoolean(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                        i32::from(*flag),
                        modified.as_ptr(),
                    ),
                },
                PieceInfoValue::StringArray(values) => {
                    let encoded: Vec<Vec<u16>> = values.iter().map(|v| wide(v)).collect();
                    let pointers: Vec<*const u16> = encoded.iter().map(|v| v.as_ptr()).collect();
                    let count = pointers.len() as std::os::raw::c_ulong;
                    match page {
                        None => pdfium_sys::EPDFDoc_SetPieceInfoStringArray(
                            self.as_raw(),
                            application.as_ptr(),
                            key.as_ptr(),
                            pointers.as_ptr().cast(),
                            count,
                            modified.as_ptr(),
                        ),
                        Some(page) => pdfium_sys::EPDFDoc_SetPagePieceInfoStringArray(
                            self.as_raw(),
                            page,
                            application.as_ptr(),
                            key.as_ptr(),
                            pointers.as_ptr().cast(),
                            count,
                            modified.as_ptr(),
                        ),
                    }
                }
                PieceInfoValue::Clear => match page {
                    None => pdfium_sys::EPDFDoc_ClearPieceInfoKey(
                        self.as_raw(),
                        application.as_ptr(),
                        key.as_ptr(),
                        modified.as_ptr(),
                    ),
                    Some(page) => pdfium_sys::EPDFDoc_ClearPagePieceInfoKey(
                        self.as_raw(),
                        page,
                        application.as_ptr(),
                        key.as_ptr(),
                        modified.as_ptr(),
                    ),
                },
            }
        } != 0)
    }

    /// Remove a whole application's data.
    pub fn clear_piece_info(
        &self,
        holder: PieceInfoHolder,
        application: &str,
    ) -> Result<bool, PdfiumError> {
        let application = c_string(application, "piece-info application")?;
        // SAFETY: the handle is live and the string outlives the call.
        Ok(unsafe {
            match holder_page(holder) {
                None => {
                    pdfium_sys::EPDFDoc_ClearPieceInfoEntry(self.as_raw(), application.as_ptr())
                }
                Some(page) => pdfium_sys::EPDFDoc_ClearPagePieceInfoEntry(
                    self.as_raw(),
                    page,
                    application.as_ptr(),
                ),
            }
        } != 0)
    }
}

/// The page number a holder names, or `None` for the catalog.
fn holder_page(holder: PieceInfoHolder) -> Option<u32> {
    match holder {
        PieceInfoHolder::Catalog => None,
        PieceInfoHolder::Page(page) => Some(page),
    }
}

fn c_string(value: &str, what: &'static str) -> Result<CString, PdfiumError> {
    CString::new(value).map_err(|_| PdfiumError::InvalidArgument(what))
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
