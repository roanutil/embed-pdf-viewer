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
            // Document::page already checked index < page_count(), so a null
            // handle here is not an out-of-range index; it's PDFium refusing
            // to load a page object that exists but is corrupt.
            return Err(PdfiumError::PageLoad(index));
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
    /// Task 2 and pdfium-binding-benchmarks/crates/ops/src/lib.rs. Changing the order or
    /// the fill colour changes the pixels and breaks the golden comparison in
    /// every host.
    pub fn render_bgra(&self, width: u32, height: u32) -> Result<Bitmap, PdfiumError> {
        let (native_width, native_height, native_stride, len) = bitmap_layout(width, height)?;
        let mut bgra = Vec::new();
        bgra.try_reserve_exact(len).map_err(|_| PdfiumError::Bitmap)?;
        bgra.resize(len, 0u8);

        let bitmap = unsafe {
            pdfium_sys::FPDFBitmap_CreateEx(
                native_width,
                native_height,
                pdfium_sys::FPDFBitmap_BGRA as i32,
                bgra.as_mut_ptr() as *mut c_void,
                native_stride,
            )
        };
        if bitmap.is_null() {
            return Err(PdfiumError::Bitmap);
        }

        unsafe {
            // White ground, fully opaque, so a page with no background matches
            // what a viewer shows.
            pdfium_sys::FPDFBitmap_FillRect(bitmap, 0, 0, native_width, native_height, 0xFFFF_FFFF);
            pdfium_sys::FPDF_RenderPageBitmap(
                bitmap,
                self.handle,
                0,
                0,
                native_width,
                native_height,
                0,
                0,
            );
            pdfium_sys::FPDFBitmap_Destroy(bitmap);
        }

        Ok(Bitmap { width, height, stride: native_stride as u32, bgra })
    }
}

fn bitmap_layout(width: u32, height: u32) -> Result<(i32, i32, i32, usize), PdfiumError> {
    let invalid = || PdfiumError::Bitmap;
    if width == 0 || height == 0 {
        return Err(invalid());
    }
    let native_width = i32::try_from(width).map_err(|_| invalid())?;
    let native_height = i32::try_from(height).map_err(|_| invalid())?;
    let stride = width.checked_mul(4).ok_or_else(invalid)?;
    let native_stride = i32::try_from(stride).map_err(|_| invalid())?;
    // PDFium itself uses a checked u32 for pitch * height. Reject invalid
    // layouts before allocating, including on 32-bit Rust targets.
    let len = stride.checked_mul(height).ok_or_else(invalid)?;
    let len = usize::try_from(len).map_err(|_| invalid())?;
    if len > isize::MAX as usize {
        return Err(invalid());
    }
    Ok((native_width, native_height, native_stride, len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unrepresentable_bitmap_layouts_before_allocation() {
        for (width, height) in [
            (0, 1), (1, 0), (u32::MAX, 1), (1 << 30, 1),
            (1 << 29, 1), (1, 1 << 31), (1 << 28, 4),
        ] {
            assert_eq!(bitmap_layout(width, height), Err(PdfiumError::Bitmap));
        }
        assert_eq!(bitmap_layout(612, 792), Ok((612, 792, 2448, 1_938_816)));
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::FPDF_ClosePage(self.handle) };
    }
}
