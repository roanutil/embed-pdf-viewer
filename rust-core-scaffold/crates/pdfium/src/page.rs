use std::ffi::c_void;
use std::marker::PhantomData;

use crate::{Bitmap, PdfiumError, TextPage};

pub struct Page<'a> {
    handle: pdfium_sys::FPDF_PAGE,
    _doc: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

impl<'a> Page<'a> {
    pub(crate) fn load(doc: pdfium_sys::FPDF_DOCUMENT, index: u32) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDF_LoadPage(doc, index as i32) };
        if handle.is_null() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Ok(Self { handle, _doc: PhantomData, _not_send: PhantomData })
    }

    pub fn width(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageWidthF(self.handle) }
    }

    pub fn height(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageHeightF(self.handle) }
    }

    pub fn text(&self) -> Result<TextPage<'_>, PdfiumError> {
        TextPage::load(self.handle)
    }

    /// The same five calls, in the same order, as build/generate-vectors.mjs in
    /// Task 2 and pdfium-rust-spike/crates/ops/src/lib.rs. Changing the order or
    /// the fill colour changes the pixels and breaks the golden comparison in
    /// every host.
    pub fn render_bgra(&self, width: u32, height: u32) -> Result<Bitmap, PdfiumError> {
        let stride = width * 4;
        let mut bgra = vec![0u8; (stride as usize) * (height as usize)];

        let bitmap = unsafe {
            pdfium_sys::FPDFBitmap_CreateEx(
                width as i32,
                height as i32,
                pdfium_sys::FPDFBitmap_BGRA as i32,
                bgra.as_mut_ptr() as *mut c_void,
                stride as i32,
            )
        };
        if bitmap.is_null() {
            return Err(PdfiumError::Bitmap);
        }

        unsafe {
            // White ground, fully opaque, so a page with no background matches
            // what a viewer shows.
            pdfium_sys::FPDFBitmap_FillRect(bitmap, 0, 0, width as i32, height as i32, 0xFFFF_FFFF);
            pdfium_sys::FPDF_RenderPageBitmap(
                bitmap,
                self.handle,
                0,
                0,
                width as i32,
                height as i32,
                0,
                0,
            );
            pdfium_sys::FPDFBitmap_Destroy(bitmap);
        }

        Ok(Bitmap { width, height, stride, bgra })
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDF_ClosePage(self.handle) };
    }
}
