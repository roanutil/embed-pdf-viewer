use std::ffi::{c_void, CString};
use std::marker::PhantomData;

use crate::{Library, Page, PdfiumError};

#[derive(Debug)]
pub struct Document {
    handle: pdfium_sys::FPDF_DOCUMENT,
    /// FPDF_LoadMemDocument does NOT copy: PDFium reads from this buffer for the
    /// document's whole life, so the Document owns it. Field order matters,
    /// because fields drop after the Drop impl body runs, which is what puts
    /// FPDF_CloseDocument ahead of freeing these bytes. Never read directly;
    /// it exists to keep the allocation PDFium points into alive.
    #[allow(dead_code)]
    bytes: Vec<u8>,
    _not_send: PhantomData<*const ()>,
}

impl Document {
    pub fn from_bytes(_lib: &Library, bytes: &[u8], password: Option<&str>) -> Result<Self, PdfiumError> {
        // One copy, owned by the Document. Moving the Document moves the Vec's
        // three words, never the heap allocation, so the pointer PDFium holds
        // stays valid. A borrowed &[u8] would need a lifetime on Document,
        // which makes it unusable from the worker-thread slab in epdf-ops.
        let bytes = bytes.to_vec();

        let c_password = password.map(|p| CString::new(p).map_err(|_| PdfiumError::Password)).transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        let handle = unsafe {
            pdfium_sys::FPDF_LoadMemDocument(
                bytes.as_ptr() as *const c_void,
                bytes.len() as i32,
                password_ptr,
            )
        };

        if handle.is_null() {
            // FPDF_GetLastError returns c_ulong, which is 64-bit on macOS and
            // Linux but 32-bit on Windows; FPDF_ERR_PASSWORD is a generated
            // u32 constant, so `as _` is required to compile on every target.
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == pdfium_sys::FPDF_ERR_PASSWORD as _ { PdfiumError::Password } else { PdfiumError::Load });
        }

        Ok(Self { handle, bytes, _not_send: PhantomData })
    }

    pub fn page_count(&self) -> u32 {
        let count = unsafe { pdfium_sys::FPDF_GetPageCount(self.handle) };
        count.max(0) as u32
    }

    pub fn page(&self, index: u32) -> Result<Page<'_>, PdfiumError> {
        if index >= self.page_count() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Page::load(self.handle, index)
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // Closes first; `self.bytes` is freed afterwards, when the fields drop.
        unsafe { pdfium_sys::FPDF_CloseDocument(self.handle) };
    }
}
