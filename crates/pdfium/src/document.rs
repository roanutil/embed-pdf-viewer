use std::ffi::{CString, c_void};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::{Library, Page, PdfiumError};

/// An open `FPDF_DOCUMENT` and the bytes PDFium is reading from.
#[derive(Debug)]
pub struct Document {
    handle: pdfium_sys::FPDF_DOCUMENT,
    /// `FPDF_LoadMemDocument64` does NOT copy: PDFium reads from this buffer
    /// for the document's whole life, so the Document owns it. Field order
    /// matters, because fields drop after the `Drop` impl body runs, which is
    /// what puts `FPDF_CloseDocument` ahead of freeing these bytes. Never read
    /// directly; it exists to keep the allocation PDFium points into alive.
    #[allow(dead_code)]
    bytes: Vec<u8>,
    /// The handle is only valid while the library that created it stays
    /// initialized. Holding a clone of the caller's `Rc<Library>` means
    /// `FPDF_DestroyLibrary` runs only once the last Document has gone, not
    /// when whichever guard happens to drop first. Like `bytes`, this runs no
    /// code itself; it exists to be dropped after the close call below.
    #[allow(dead_code)]
    library: Rc<Library>,
    _not_send: PhantomData<*const ()>,
}

impl Document {
    /// Opens a document from bytes, copying them into an allocation the
    /// Document owns.
    ///
    /// One copy is deliberate. Moving the Document moves the `Vec`'s three
    /// words, never the heap allocation, so the pointer PDFium holds stays
    /// valid; a borrowed `&[u8]` would put a lifetime on `Document` and make
    /// it unusable from an owner thread's document map.
    ///
    /// The copy is reserved fallibly: a document larger than the process can
    /// hold is an error, not an abort. That is the admission check the
    /// lifecycle contract asks for on input bytes.
    pub fn from_bytes(
        lib: Rc<Library>,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<Self, PdfiumError> {
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| PdfiumError::OutOfMemory)?;
        owned.extend_from_slice(bytes);

        // A NUL in the password would truncate silently, so reject it rather
        // than opening the document with a shorter password than was asked
        // for and reporting the failure as a wrong password.
        let c_password = password
            .map(|p| CString::new(p).map_err(|_| PdfiumError::InvalidArgument("password contains NUL")))
            .transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        // SAFETY: the pointer and length describe `owned`, which this
        // Document keeps alive for as long as the handle exists, and the
        // password pointer is either null or a live NUL-terminated CString
        // that outlives the call.
        let handle = unsafe {
            pdfium_sys::FPDF_LoadMemDocument64(
                owned.as_ptr() as *const c_void,
                owned.len(),
                password_ptr,
            )
        };

        if handle.is_null() {
            // FPDF_GetLastError returns c_ulong, which is 64-bit on macOS and
            // Linux but 32-bit on Windows; FPDF_ERR_PASSWORD is a generated
            // u32 constant, so `as _` is required to compile on every target.
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == pdfium_sys::FPDF_ERR_PASSWORD as _ {
                PdfiumError::Password
            } else {
                PdfiumError::Load
            });
        }

        Ok(Self { handle, bytes: owned, library: lib, _not_send: PhantomData })
    }

    pub fn page_count(&self) -> u32 {
        // Negative means PDFium could not read the page tree; report zero
        // pages rather than a huge unsigned count.
        unsafe { pdfium_sys::FPDF_GetPageCount(self.handle) }.max(0) as u32
    }

    /// The document's standard security permission bits, as PDFium reports
    /// them.
    ///
    /// Raw. Bits 1-2 are reserved as zero by the standard security handler and
    /// PDFium masks even owner permissions through that convention, which is
    /// why "all permissions" is `0xFFFFFFFC` rather than `0xFFFFFFFF`.
    /// Interpreting that is `epdf_engine::session::security`.
    pub fn permissions(&self) -> u32 {
        unsafe { pdfium_sys::FPDF_GetDocPermissions(self.handle) as u32 }
    }

    /// Loads a page by zero-based index, owning it until the returned `Page`
    /// drops.
    pub fn page(&self, index: u32) -> Result<Page<'_>, PdfiumError> {
        if index >= self.page_count() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Page::load(self.handle, index)
    }

    /// The raw handle, for the operations this layer has not wrapped yet.
    ///
    /// # Safety
    /// The caller must not close the handle or outlive this `Document`.
    pub unsafe fn as_raw(&self) -> pdfium_sys::FPDF_DOCUMENT {
        self.handle
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // Closes first; `self.bytes` is freed afterwards, when the fields drop.
        // SAFETY: the handle was returned non-null by FPDF_LoadMemDocument64
        // and is closed exactly once, here.
        unsafe { pdfium_sys::FPDF_CloseDocument(self.handle) };
    }
}
