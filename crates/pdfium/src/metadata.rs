//! The document Info dictionary.
//!
//! Two string ABIs meet here and they are not the same. Values come back as
//! UTF-16 through `FPDF_GetMetaText`, and key names come back as UTF-8 through
//! `EPDF_GetMetaKeyName`. Both report a required byte length first and fill a
//! buffer second, and both count the terminator in that length.

use std::ffi::CString;

use crate::{Document, PdfiumError};

/// `/Trapped`, as PDFium reports it.
///
/// PDFium has four values and the wire has three: `NotSet` folds into
/// `Unknown`, because a document that never said is in the same position as
/// one that said it did not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trapped {
    True,
    False,
    Unknown,
}

impl Document {
    /// One Info-dictionary value.
    ///
    /// `None` when the key is absent, `Some("")` when it is present and empty.
    /// The distinction is real: a document that set `/Title ()` said something
    /// different from one that set no title, and a patch has to be able to
    /// tell them apart.
    pub fn meta_text(&self, key: &str) -> Result<Option<String>, PdfiumError> {
        let c_key = CString::new(key)
            .map_err(|_| PdfiumError::InvalidArgument("metadata key contains NUL"))?;

        // SAFETY: the handle is live and the key outlives the call.
        let present = unsafe { pdfium_sys::EPDF_HasMetaText(self.handle, c_key.as_ptr()) };
        if present == 0 {
            return Ok(None);
        }

        // SAFETY: a null buffer with zero length is the documented measure
        // call; it returns the required byte length including the terminator.
        let bytes = unsafe {
            pdfium_sys::FPDF_GetMetaText(self.handle, c_key.as_ptr(), std::ptr::null_mut(), 0)
        };
        if bytes <= 2 {
            // Two bytes is the terminator alone, which is the present-but-
            // empty case rather than an error.
            return Ok(Some(String::new()));
        }

        let mut buffer: Vec<u16> = Vec::new();
        let units = (bytes as usize).div_ceil(2);
        buffer.try_reserve_exact(units).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(units, 0u16);

        // SAFETY: the buffer holds `bytes` bytes, which is what the length
        // argument claims.
        let written = unsafe {
            pdfium_sys::FPDF_GetMetaText(
                self.handle,
                c_key.as_ptr(),
                buffer.as_mut_ptr().cast(),
                bytes,
            )
        };
        if written == 0 {
            return Ok(Some(String::new()));
        }
        // Drop the terminator. The count is bytes, the buffer is u16.
        let produced = ((written as usize) / 2).saturating_sub(1).min(buffer.len());
        Ok(Some(String::from_utf16_lossy(&buffer[..produced])))
    }

    /// `/Trapped`.
    pub fn meta_trapped(&self) -> Trapped {
        // SAFETY: the handle is live.
        match unsafe { pdfium_sys::EPDF_GetMetaTrapped(self.handle) } {
            1 => Trapped::True,
            2 => Trapped::False,
            // 0 is NotSet and 3 is Unknown; both are "nobody said".
            _ => Trapped::Unknown,
        }
    }

    /// Every custom key in the Info dictionary, in document order.
    ///
    /// Custom means not one of the reserved names. A reader that returned
    /// those too would report `/Title` twice, once as a field and once as
    /// custom data.
    pub fn custom_meta_keys(&self) -> Result<Vec<String>, PdfiumError> {
        // SAFETY: the handle is live. The `true` asks for custom keys only.
        let count = unsafe { pdfium_sys::EPDF_GetMetaKeyCount(self.handle, 1) };
        // EPDF_GetMetaKeyCount reports a negative count for a document with
        // no Info dictionary, so the floor is not decorative.
        let count = count.max(0);

        let mut keys = Vec::new();
        for index in 0..count {
            if let Some(key) = self.custom_meta_key_name(index)? {
                keys.push(key);
            }
        }
        Ok(keys)
    }

    fn custom_meta_key_name(&self, index: i32) -> Result<Option<String>, PdfiumError> {
        // SAFETY: the measure call, as above.
        let bytes = unsafe {
            pdfium_sys::EPDF_GetMetaKeyName(self.handle, index, 1, std::ptr::null_mut(), 0)
        };
        if bytes <= 1 {
            return Ok(None);
        }

        let mut buffer: Vec<u8> = Vec::new();
        buffer.try_reserve_exact(bytes as usize).map_err(|_| PdfiumError::OutOfMemory)?;
        buffer.resize(bytes as usize, 0u8);

        // SAFETY: the buffer holds `bytes` bytes.
        let written = unsafe {
            pdfium_sys::EPDF_GetMetaKeyName(
                self.handle,
                index,
                1,
                buffer.as_mut_ptr().cast(),
                bytes,
            )
        };
        if written == 0 {
            return Ok(None);
        }
        // UTF-8 here, unlike the values. Drop the terminator.
        let produced = (written as usize).saturating_sub(1).min(buffer.len());
        Ok(Some(String::from_utf8_lossy(&buffer[..produced]).into_owned()))
    }
}
