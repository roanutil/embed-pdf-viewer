//! Writing the document information dictionary.
//!
//! G14. The write half of [`crate::metadata`].
//!
//! # A null value removes the key
//!
//! `EPDF_SetMetaText` deletes an entry when it is handed a null value,
//! which is how a clear is spelled. An EMPTY string is not a clear: it is a
//! key that is present and holds nothing, and the reader reports it as one.
//! Collapsing the two would make a patch unable to say "the title is
//! deliberately blank".

use std::ffi::CString;

use crate::{Document, PdfiumError, Trapped};

impl Document {
    /// Set or remove one `/Info` entry.
    ///
    /// `None` removes it.
    pub fn set_meta_text(&self, key: &str, value: Option<&str>) -> Result<bool, PdfiumError> {
        let key = CString::new(key).map_err(|_| PdfiumError::InvalidArgument("metadata key"))?;
        let encoded: Option<Vec<u16>> =
            value.map(|value| value.encode_utf16().chain(std::iter::once(0)).collect());
        let pointer = encoded.as_ref().map_or(std::ptr::null(), |value| value.as_ptr());
        // SAFETY: the handle is live and both strings outlive the call.
        Ok(unsafe { pdfium_sys::EPDF_SetMetaText(self.as_raw(), key.as_ptr(), pointer) != 0 })
    }

    /// `/Trapped`.
    ///
    /// No clear form: the wire vocabulary is three states and none of them
    /// is absence, so a patch that wants the entry left alone omits it.
    pub fn set_meta_trapped(&self, status: Trapped) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDF_SetMetaTrapped(self.as_raw(), status.code() as u32) != 0 }
    }
}
