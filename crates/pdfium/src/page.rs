use std::ffi::c_void;
use std::marker::PhantomData;
use std::ops::Deref;

use crate::{Bitmap, PdfiumError, TextPage};

/// A page this layer may read but does not own.
///
/// The two callers are different by design. `Page` derefs to it and closes on
/// drop. Stage A of the ownership migration
/// (`multiplatform-plan-claude-remix.md` Section 6) builds one from a pointer
/// the TypeScript `PagePtrPool` lends for the duration of a single call: Rust
/// must not call `FPDF_LoadPage` itself there, because doing so would bypass
/// normalized rotation, the pool's shared refcounts and its structural-mutation
/// leak checks. A `PageView` closes nothing, so it cannot.
#[derive(Debug)]
pub struct PageView<'a> {
    handle: pdfium_sys::FPDF_PAGE,
    _owner: PhantomData<&'a ()>,
    _not_send: PhantomData<*const ()>,
}

impl<'a> PageView<'a> {
    /// Wraps a page pointer owned by someone else. Returns `None` for null.
    ///
    /// # Safety
    /// `handle` must be a live `FPDF_PAGE` that stays open, on this thread,
    /// for all of `'a`, and `'a` must not outlive whatever owns it. The
    /// lifetime is unbounded at this call, so the caller chooses it: bind it
    /// to the borrow the pointer came from rather than letting it be inferred.
    pub unsafe fn from_raw(handle: pdfium_sys::FPDF_PAGE) -> Option<Self> {
        if handle.is_null() {
            return None;
        }
        Some(Self { handle, _owner: PhantomData, _not_send: PhantomData })
    }

    pub fn width(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageWidthF(self.handle) }
    }

    pub fn height(&self) -> f32 {
        unsafe { pdfium_sys::FPDF_GetPageHeightF(self.handle) }
    }

    /// The page's PDF indirect object number, which is the engine's durable
    /// page identity. Zero means the file put a direct (non-indirect) page
    /// dictionary in the `/Pages` tree, which the engine reports as a
    /// malformed PDF rather than working around.
    pub fn object_number(&self) -> u32 {
        unsafe { pdfium_sys::EPDFPage_GetObjectNumber(self.handle) }
    }

    pub fn text(&self) -> Result<TextPage<'_>, PdfiumError> {
        TextPage::load(self.handle)
    }

    /// Renders the whole page into a fresh BGRA buffer.
    ///
    /// The five calls, in this order and with this fill colour, are what the
    /// frozen vectors were generated from. Changing either changes the pixels
    /// and breaks the golden comparison on every host.
    pub fn render_bgra(&self, width: u32, height: u32) -> Result<Bitmap, PdfiumError> {
        let (native_width, native_height, native_stride, len) = bitmap_layout(width, height)?;
        let mut bgra = Vec::new();
        bgra.try_reserve_exact(len).map_err(|_| PdfiumError::OutOfMemory)?;
        bgra.resize(len, 0u8);

        // SAFETY: the buffer is `len` bytes, which bitmap_layout proved is
        // exactly stride * height, and it outlives the bitmap handle below.
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
            pdfium_sys::FPDFBitmap_FillRect(
                bitmap,
                0,
                0,
                native_width,
                native_height,
                0xFFFF_FFFF,
            );
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

    /// # Safety
    /// The caller must not close the handle or outlive this view.
    pub unsafe fn as_raw(&self) -> pdfium_sys::FPDF_PAGE {
        self.handle
    }
}

/// A page this layer loaded and closes on drop.
#[derive(Debug)]
pub struct Page<'a> {
    view: PageView<'a>,
}

impl<'a> Page<'a> {
    pub(crate) fn load(
        doc: pdfium_sys::FPDF_DOCUMENT,
        index: u32,
    ) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDF_LoadPage(doc, index as i32) };
        // Document::page already checked index < page_count(), so a null
        // handle here is not an out-of-range index; it is PDFium refusing to
        // load a page object that exists but is corrupt.
        let view = unsafe { PageView::from_raw(handle) }.ok_or(PdfiumError::PageLoad(index))?;
        Ok(Self { view })
    }
}

impl<'a> Deref for Page<'a> {
    type Target = PageView<'a>;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        // SAFETY: loaded by FPDF_LoadPage above and closed exactly once.
        unsafe { pdfium_sys::FPDF_ClosePage(self.view.handle) };
    }
}

/// Validates a bitmap layout before anything is allocated: the buffer
/// contract's "lengths and multiplications validated before allocation, no
/// freeing by guessed capacity".
fn bitmap_layout(width: u32, height: u32) -> Result<(i32, i32, i32, usize), PdfiumError> {
    let invalid = || PdfiumError::InvalidArgument("bitmap width/height");
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
            (0, 1),
            (1, 0),
            (u32::MAX, 1),
            (1 << 30, 1),
            (1 << 29, 1),
            (1, 1 << 31),
            (1 << 28, 4),
        ] {
            assert!(bitmap_layout(width, height).is_err(), "{width}x{height} was accepted");
        }
        assert_eq!(bitmap_layout(612, 792), Ok((612, 792, 2448, 1_938_816)));
    }
}
