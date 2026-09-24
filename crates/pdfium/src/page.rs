use std::ffi::c_void;
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use crate::{Bitmap, Document, PdfiumError, TextPage};

/// Emit RGBA byte order rather than PDFium's native BGRA.
const FPDF_REVERSE_BYTE_ORDER: i32 = 0x10;
/// Draw annotation appearances along with the page content.
const FPDF_RENDER_ANNOT: i32 = 0x01;

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
    /// Render a region through a caller-supplied affine, into RGBA.
    ///
    /// The general form. [`PageView::render_bgra`] is the whole page at a
    /// size; this is what a viewport, a rotation and a sub-rect all become,
    /// and it is the only entry point `epdf_engine::features::render` uses.
    ///
    /// RGBA rather than BGRA, through `FPDF_REVERSE_BYTE_ORDER`. The
    /// TypeScript does the same and its callers receive `rgba8`; converting in
    /// PDFium costs nothing and converting afterwards costs a pass over every
    /// pixel.
    ///
    /// `matrix` is the six affine cells in `FS_MATRIX` order, and it maps
    /// display space, after PDFium's own `GetDisplayMatrix` has flipped the y
    /// axis, onto the bitmap. `epdf_engine`'s
    /// `features::render::geometry::display_rect_to_device_matrix` produces it.
    pub fn render_rgba_with_matrix(
        &self,
        width: u32,
        height: u32,
        matrix: [f64; 6],
        opaque_background: bool,
        include_annotations: bool,
    ) -> Result<Bitmap, PdfiumError> {
        let (native_width, native_height, native_stride, len) = bitmap_layout(width, height)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| PdfiumError::OutOfMemory)?;
        pixels.resize(len, 0u8);

        // SAFETY: the buffer is exactly stride * height bytes, proved by
        // bitmap_layout, and it outlives the bitmap handle destroyed below.
        let bitmap = unsafe {
            pdfium_sys::FPDFBitmap_CreateEx(
                native_width,
                native_height,
                pdfium_sys::FPDFBitmap_BGRA as i32,
                pixels.as_mut_ptr() as *mut c_void,
                native_stride,
            )
        };
        if bitmap.is_null() {
            return Err(PdfiumError::Bitmap);
        }

        let fs_matrix = pdfium_sys::FS_MATRIX {
            a: matrix[0] as f32,
            b: matrix[1] as f32,
            c: matrix[2] as f32,
            d: matrix[3] as f32,
            e: matrix[4] as f32,
            f: matrix[5] as f32,
        };
        // The clip is the whole bitmap. A narrower one belongs in the matrix,
        // where it also scales; clipping here would crop instead.
        let clip = pdfium_sys::FS_RECTF {
            left: 0.0,
            top: 0.0,
            right: width as f32,
            bottom: height as f32,
        };

        let mut flags = FPDF_REVERSE_BYTE_ORDER;
        if include_annotations {
            flags |= FPDF_RENDER_ANNOT;
        }

        unsafe {
            pdfium_sys::FPDFBitmap_FillRect(
                bitmap,
                0,
                0,
                native_width,
                native_height,
                if opaque_background { 0xFFFF_FFFF } else { 0x0000_0000 },
            );
            pdfium_sys::FPDF_RenderPageBitmapWithMatrix(
                bitmap,
                self.handle,
                &fs_matrix,
                &clip,
                flags,
            );
            pdfium_sys::FPDFBitmap_Destroy(bitmap);
        }

        Ok(Bitmap { width, height, stride: native_stride as u32, bgra: pixels })
    }

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

    /// How many annotations the page carries.
    ///
    /// `FPDFPage_GetAnnotCount` returns a negative value on failure, which is
    /// reported as zero rather than propagated: the count's only caller so far
    /// is a test asserting that a write landed, and "the page has no
    /// annotations" is the right reading of a page PDFium will not enumerate.
    /// A real G7 reader will want the distinction and should not reuse this.
    pub fn annotation_count(&self) -> u32 {
        unsafe { pdfium_sys::FPDFPage_GetAnnotCount(self.handle) }.max(0) as u32
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
    pub(crate) fn load(doc: pdfium_sys::FPDF_DOCUMENT, index: u32) -> Result<Self, PdfiumError> {
        let handle = unsafe { pdfium_sys::FPDF_LoadPage(doc, index as i32) };
        // Document::page already checked index < page_count(), so a null
        // handle here is not an out-of-range index; it is PDFium refusing to
        // load a page object that exists but is corrupt.
        let view = unsafe { PageView::from_raw(handle) }.ok_or(PdfiumError::PageLoad(index))?;
        Ok(Self { view })
    }

    /// Loads a page by PDF object number with its rotation normalized to zero.
    ///
    /// The loader every read path should use. `FPDF_LoadPage` reports a
    /// `/Rotate 90` page as landscape and places its text accordingly;
    /// `EPDFDoc_LoadPageByObjectNumberNormalized` forces rotation to zero so
    /// render, text, geometry and annotation coordinates share one space. The
    /// TypeScript `PagePtrPool.acquire` uses it for exactly that reason, and
    /// `multiplatform-plan.md` says in as many words that calling
    /// `FPDF_LoadPage` here "would bypass normalized rotation".
    pub(crate) fn load_normalized(
        doc: pdfium_sys::FPDF_DOCUMENT,
        object_number: u32,
    ) -> Result<Self, PdfiumError> {
        // SAFETY: `doc` is a live document handle owned by the caller.
        let handle =
            unsafe { pdfium_sys::EPDFDoc_LoadPageByObjectNumberNormalized(doc, object_number) };
        let view = unsafe { PageView::from_raw(handle) }
            .ok_or(PdfiumError::PageLoadByObjectNumber(object_number))?;
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
        for (width, height) in
            [(0, 1), (1, 0), (u32::MAX, 1), (1 << 30, 1), (1 << 29, 1), (1, 1 << 31), (1 << 28, 4)]
        {
            assert!(bitmap_layout(width, height).is_err(), "{width}x{height} was accepted");
        }
        assert_eq!(bitmap_layout(612, 792), Ok((612, 792, 2448, 1_938_816)));
    }
}

/// A page that keeps its document alive.
///
/// [`Page`] borrows its `Document`, which is right for a page used and dropped
/// inside one call and wrong for a page a session pools across calls: a
/// borrowing page cannot be stored beside the document it borrows from.
/// `SharedPage` holds an `Rc<Document>` instead, so the invariant "the document
/// outlives the page" is upheld by the refcount rather than by a lifetime.
///
/// Not `Send`, like everything else here: PDFium handles never cross threads.
#[derive(Debug)]
pub struct SharedPage {
    handle: pdfium_sys::FPDF_PAGE,
    /// `'static` is a placeholder, not a claim. The real bound is `document`
    /// below, and [`SharedPage::view`] hands out a borrow no longer than
    /// `&self`; `PageView` is covariant in its lifetime, so that shortening is
    /// the compiler's own coercion rather than a transmute.
    view: PageView<'static>,
    /// Declared last so it drops last: the page is closed in `Drop` before the
    /// document it was loaded from can be released.
    document: Rc<Document>,
}

impl SharedPage {
    /// Loads a page by object number, rotation normalized, keeping `document`
    /// alive for as long as the page.
    pub fn load(document: Rc<Document>, object_number: u32) -> Result<Self, PdfiumError> {
        // SAFETY: `document` is live for the duration of the call and, through
        // the `Rc` this struct keeps, for the life of the returned page.
        let handle = unsafe {
            pdfium_sys::EPDFDoc_LoadPageByObjectNumberNormalized(document.as_raw(), object_number)
        };
        // SAFETY: the handle is non-null when `from_raw` returns `Some`, it is
        // closed exactly once in this type's `Drop`, and the `Rc` field keeps
        // its document open for all of the chosen lifetime.
        let view = unsafe { PageView::from_raw(handle) }
            .ok_or(PdfiumError::PageLoadByObjectNumber(object_number))?;
        Ok(Self { handle, view, document })
    }

    /// The page, for no longer than this borrow.
    pub fn view(&self) -> &PageView<'_> {
        &self.view
    }

    /// The document this page was loaded from.
    pub fn document(&self) -> &Rc<Document> {
        &self.document
    }
}

impl Deref for SharedPage {
    type Target = PageView<'static>;

    fn deref(&self) -> &Self::Target {
        &self.view
    }
}

impl Drop for SharedPage {
    fn drop(&mut self) {
        // SAFETY: loaded above and closed exactly once, here, before the
        // document field drops.
        unsafe { pdfium_sys::FPDF_ClosePage(self.handle) };
    }
}
