//! Copying pages into a new document, and serializing it.
//!
//! A read over the source: `FPDF_ImportPagesByIndex` copies into a scratch
//! document, so nothing about the original changes. The scratch is closed
//! before anything returns, success or failure.
//!
//! `EPDF_SaveDocumentToOwnedBuffer` lands here rather than in a save module
//! because this is its first caller; G10's standalone saves reuse it.

use crate::{Document, PdfiumError};

// `FPDF_SAVE_FLAGS`: write a full file rather than an incremental update.
//
// An incremental save appends a revision to the original bytes, which for a
// document built from nothing would produce a file with a history it never
// had. Extraction wants a clean document.
use crate::save::FPDF_NO_INCREMENTAL;

/// A buffer PDFium allocated, freed when this drops.
///
/// Not a `Vec`: the bytes are in PDFium's allocator and freeing them with
/// Rust's would be a cross-allocator free. The copy out happens in
/// [`OwnedBuffer::to_vec`], which is explicit about paying for it.
struct OwnedBuffer {
    ptr: *mut std::ffi::c_void,
    len: usize,
}

impl OwnedBuffer {
    fn to_vec(&self) -> Result<Vec<u8>, PdfiumError> {
        let mut out = Vec::new();
        out.try_reserve_exact(self.len).map_err(|_| PdfiumError::OutOfMemory)?;
        // SAFETY: PDFium reported `len` bytes at `ptr`, and the buffer is
        // alive for as long as this value is.
        out.extend_from_slice(unsafe {
            std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len)
        });
        Ok(out)
    }
}

impl Drop for OwnedBuffer {
    fn drop(&mut self) {
        // SAFETY: allocated by EPDF_SaveDocumentToOwnedBuffer and freed
        // exactly once, here, with PDFium's own deallocator.
        unsafe { pdfium_sys::EPDF_FreeBuffer(self.ptr) };
    }
}

/// How [`Document::extract_pages`] copies its pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractCopy {
    /// As they are (`FPDF_ImportPagesByIndex`).
    Plain,
    /// As they draw for `usage` (`EPDFDoc_ExportPagesFlattened`): the
    /// annotations and the form fields painted in when chosen, nothing
    /// interactive left.
    Flattened { usage: crate::FlattenUsage, annotations: bool, form_fields: bool },
}

// `EPDF_EXPORT_FLATTENED_*` (`public/epdf_flatten.h`).
const EXPORT_FLATTENED_ANNOTATIONS: i32 = 0x1;
const EXPORT_FLATTENED_FORM_FIELDS: i32 = 0x2;

impl Document {
    /// Copy pages, in the order given, into a new document and serialize it
    /// (`PagesExtractor.extract`), with this document's `/Title` when it has
    /// one.
    ///
    /// Indices address this document's current page order. The caller's order
    /// is preserved in the output, so extracting `[3, 1]` produces a two-page
    /// document in that order rather than in document order.
    pub fn extract_pages(
        &self,
        indices: &[u32],
        copy: ExtractCopy,
    ) -> Result<Vec<u8>, PdfiumError> {
        if indices.is_empty() {
            return Err(PdfiumError::InvalidArgument("extract needs at least one page"));
        }
        let signed: Vec<i32> = indices
            .iter()
            .map(|i| i32::try_from(*i).map_err(|_| PdfiumError::InvalidArgument("page index")))
            .collect::<Result<_, _>>()?;

        // A guard so every path below closes the scratch document, including
        // the error ones. Without it an early return leaks a whole document.
        struct Scratch(pdfium_sys::FPDF_DOCUMENT);
        impl Drop for Scratch {
            fn drop(&mut self) {
                // SAFETY: created below and closed exactly once.
                unsafe { pdfium_sys::FPDF_CloseDocument(self.0) };
            }
        }
        let scratch = match copy {
            ExtractCopy::Plain => {
                // SAFETY: no arguments, and a null return is the documented
                // failure.
                let dest = unsafe { pdfium_sys::FPDF_CreateNewDocument() };
                if dest.is_null() {
                    return Err(PdfiumError::Load);
                }
                let scratch = Scratch(dest);
                // SAFETY: both handles are live, and the slice is
                // `signed.len()` elements as the length argument claims.
                let ok = unsafe {
                    pdfium_sys::FPDF_ImportPagesByIndex(
                        scratch.0,
                        self.handle,
                        signed.as_ptr(),
                        signed.len() as std::os::raw::c_ulong,
                        0,
                    )
                };
                if ok == 0 {
                    return Err(PdfiumError::InvalidArgument("PDFium refused the page selection"));
                }
                scratch
            }
            ExtractCopy::Flattened { usage, annotations, form_fields } => {
                let flags = if annotations { EXPORT_FLATTENED_ANNOTATIONS } else { 0 }
                    | if form_fields { EXPORT_FLATTENED_FORM_FIELDS } else { 0 };
                // SAFETY: the handle is live, and the slice is `signed.len()`
                // elements as the length argument claims. A non-null result
                // is a new document the caller closes.
                let dest = unsafe {
                    pdfium_sys::EPDFDoc_ExportPagesFlattened(
                        self.handle,
                        signed.as_ptr(),
                        signed.len() as std::os::raw::c_ulong,
                        usage as i32,
                        flags,
                    )
                };
                if dest.is_null() {
                    return Err(PdfiumError::InvalidArgument("PDFium refused the flattened copy"));
                }
                Scratch(dest)
            }
        };

        // Either copy keeps the document's title.
        if let Some(title) = self.meta_text("Title")? {
            let key = std::ffi::CString::new("Title").map_err(|_| PdfiumError::Load)?;
            let value: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: the scratch handle is live and both strings outlive
            // the call.
            unsafe { pdfium_sys::EPDF_SetMetaText(scratch.0, key.as_ptr(), value.as_ptr()) };
        }

        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and `size` is an initialized local.
        let ptr = unsafe {
            pdfium_sys::EPDF_SaveDocumentToOwnedBuffer(scratch.0, FPDF_NO_INCREMENTAL, &mut size)
        };
        if ptr.is_null() || size == 0 {
            if !ptr.is_null() {
                // SAFETY: allocated by the call above.
                unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
            }
            return Err(PdfiumError::Load);
        }
        let buffer = OwnedBuffer { ptr, len: size as usize };
        buffer.to_vec()
    }
}
