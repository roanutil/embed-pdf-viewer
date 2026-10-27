use std::ffi::{CString, c_void};
use std::marker::PhantomData;
use std::ops::Deref;
use std::rc::Rc;

use crate::{Bitmap, Document, PdfiumError, RectF, TextPage};

/// Emit RGBA byte order rather than PDFium's native BGRA.
const FPDF_REVERSE_BYTE_ORDER: i32 = 0x10;
/// `FPDF_ANNOT`: the annotations, form fields' widgets excepted.
const FPDF_RENDER_ANNOT: i32 = 0x01;
/// `EPDF_RENDER_WIDGETS`: the form fields' widgets.
const EPDF_RENDER_WIDGETS: i32 = 0x8000;
/// `FPDF_PRINTING`: the print rules. Layers take their print state, and
/// annotations and widgets draw by their `print` flag (no-view ones too).
const FPDF_PRINTING: i32 = 0x800;
/// A sliced render paused, to be continued.
const FPDF_RENDER_TOBECONTINUED: i32 = 1;
/// A sliced render finished.
const FPDF_RENDER_DONE: i32 = 2;

/// What a page render draws beside the page content, and by which rules
/// (`PageRenderReader`'s flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderFlags {
    /// The annotations, form fields' widgets excepted (`FPDF_ANNOT`).
    pub annotations: bool,
    /// The form fields' widgets (`EPDF_RENDER_WIDGETS`).
    pub widgets: bool,
    /// The print rules rather than the screen's (`FPDF_PRINTING`).
    pub printing: bool,
}

impl RenderFlags {
    /// The flags PDFium takes, RGBA byte order always.
    fn bits(self) -> i32 {
        let mut flags = FPDF_REVERSE_BYTE_ORDER;
        if self.annotations {
            flags |= FPDF_RENDER_ANNOT;
        }
        if self.widgets {
            flags |= EPDF_RENDER_WIDGETS;
        }
        if self.printing {
            flags |= FPDF_PRINTING;
        }
        flags
    }
}

/// Why a sliced render gave no pixels: PDFium's refusal, or the caller's own
/// error from between two slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlicedRender<E> {
    Pdfium(PdfiumError),
    Stopped(E),
}

/// A filled bitmap over a pixel buffer this layer owns, with the matrix and
/// clip a page render draws through. Destroys the bitmap when dropped; the
/// pixels outlive it ([`RenderTarget::into_bitmap`]).
struct RenderTarget {
    bitmap: pdfium_sys::FPDF_BITMAP,
    matrix: pdfium_sys::FS_MATRIX,
    clip: pdfium_sys::FS_RECTF,
    width: u32,
    height: u32,
    stride: u32,
    pixels: Vec<u8>,
}

impl RenderTarget {
    fn new(
        width: u32,
        height: u32,
        matrix: [f64; 6],
        opaque_background: bool,
    ) -> Result<Self, PdfiumError> {
        let (native_width, native_height, native_stride, len) = bitmap_layout(width, height)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).map_err(|_| PdfiumError::OutOfMemory)?;
        pixels.resize(len, 0u8);

        // SAFETY: the buffer is exactly stride * height bytes, proved by
        // bitmap_layout, and it outlives the bitmap handle, which `Drop`
        // destroys before the buffer is freed or moved out.
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
        // SAFETY: the bitmap was created above.
        unsafe {
            pdfium_sys::FPDFBitmap_FillRect(
                bitmap,
                0,
                0,
                native_width,
                native_height,
                if opaque_background { 0xFFFF_FFFF } else { 0x0000_0000 },
            );
        }
        Ok(Self {
            bitmap,
            matrix: pdfium_sys::FS_MATRIX {
                a: matrix[0] as f32,
                b: matrix[1] as f32,
                c: matrix[2] as f32,
                d: matrix[3] as f32,
                e: matrix[4] as f32,
                f: matrix[5] as f32,
            },
            // The clip is the whole bitmap. A narrower one belongs in the
            // matrix, where it also scales; clipping here would crop instead.
            clip: pdfium_sys::FS_RECTF {
                left: 0.0,
                top: 0.0,
                right: width as f32,
                bottom: height as f32,
            },
            width,
            height,
            stride: native_stride as u32,
            pixels,
        })
    }

    /// The pixels, the bitmap destroyed first.
    fn into_bitmap(mut self) -> Bitmap {
        let bgra = std::mem::take(&mut self.pixels);
        let (width, height, stride) = (self.width, self.height, self.stride);
        // Drops here, destroying the bitmap; `pixels` is already out, and
        // its heap buffer did not move.
        drop(self);
        Bitmap { width, height, stride, bgra }
    }
}

impl Drop for RenderTarget {
    fn drop(&mut self) {
        // SAFETY: created in `new` and destroyed exactly once, here.
        unsafe { pdfium_sys::FPDFBitmap_Destroy(self.bitmap) };
    }
}

/// The turn an appearance render takes back out.
///
/// The arguments `EPDF_RenderAnnotBitmapUnrotated` takes since runtime
/// 5e9241945: `degrees` counterclockwise (the PDF convention), about the
/// middle of `unrotated_box`, in page space before turning.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppearanceTurn {
    pub degrees: f32,
    pub unrotated_box: RectF,
}

/// A page this layer may read but does not own.
///
/// The two callers are different by design. `Page` derefs to it and closes on
/// drop. Stage A of the ownership migration
/// (`multiplatform-plan.md` Section 6) builds one from a pointer
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

    /// The page's display box in PDF user space (`FPDF_GetPageBoundingBox`):
    /// the crop box cut to the media box, as PDFium's display matrix maps it
    /// to the page's pixels. Corners as PDFium reports them; `None` when the
    /// call fails.
    pub fn bounding_box(&self) -> Option<RectF> {
        let mut rect = pdfium_sys::FS_RECTF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
        // SAFETY: the handle is live and `rect` is an initialized local.
        let ok = unsafe { pdfium_sys::FPDF_GetPageBoundingBox(self.handle, &mut rect) };
        (ok != 0).then_some(RectF {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        })
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
    /// Render a region through a caller-supplied affine, into RGBA, in one
    /// call (`FPDF_RenderPageBitmapWithMatrix`).
    ///
    /// [`PageView::render_bgra`] is the whole page at a size; this is what a
    /// viewport, a rotation and a sub-rect all become.
    /// `epdf_engine::features::render` renders through
    /// [`PageView::render_rgba_with_matrix_in_slices`], whose pixels are
    /// these, however it is sliced.
    ///
    /// RGBA rather than BGRA, through `FPDF_REVERSE_BYTE_ORDER`. The
    /// TypeScript does the same and its callers receive `rgba8`; converting in
    /// PDFium costs nothing and converting afterwards costs a pass over every
    /// pixel.
    ///
    /// `matrix` is the six affine cells in `FS_MATRIX` order, and it maps
    /// display space, after PDFium's own `GetDisplayMatrix` has flipped the y
    /// axis, onto the bitmap. `epdf_engine`'s
    /// `features::render::geometry::render_matrix` produces it.
    pub fn render_rgba_with_matrix(
        &self,
        width: u32,
        height: u32,
        matrix: [f64; 6],
        opaque_background: bool,
        flags: RenderFlags,
    ) -> Result<Bitmap, PdfiumError> {
        let target = RenderTarget::new(width, height, matrix, opaque_background)?;
        // SAFETY: the bitmap is live until `target` drops, after this call;
        // the matrix and clip are fields of `target`.
        unsafe {
            pdfium_sys::FPDF_RenderPageBitmapWithMatrix(
                target.bitmap,
                self.handle,
                &target.matrix,
                &target.clip,
                flags.bits(),
            );
        }
        Ok(target.into_bitmap())
    }

    /// [`PageView::render_rgba_with_matrix`] in slices
    /// (`EPDF_RenderPageBitmapWithMatrix_Start`, `EPDF_RenderPage_Continue`),
    /// as `PageRenderReader.render` draws: PDFium works for `budget_ms` at a
    /// time (0 pauses at every chance), and `between` runs between two
    /// slices. An error from `between` stops the render there, and is
    /// answered as [`SlicedRender::Stopped`]. However the render is sliced,
    /// the pixels are those of the one-call render.
    ///
    /// The render is closed (`FPDF_RenderPage_Close`) whatever happened, and
    /// before the bitmap it draws into is freed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_rgba_with_matrix_in_slices<E>(
        &self,
        width: u32,
        height: u32,
        matrix: [f64; 6],
        opaque_background: bool,
        flags: RenderFlags,
        budget_ms: u32,
        mut between: impl FnMut() -> Result<(), E>,
    ) -> Result<Bitmap, SlicedRender<E>> {
        let target = RenderTarget::new(width, height, matrix, opaque_background)
            .map_err(SlicedRender::Pdfium)?;
        let budget = i32::try_from(budget_ms).unwrap_or(i32::MAX);
        // SAFETY: the bitmap is live until `target` drops, which is after
        // the render is closed below; the matrix and clip are fields of
        // `target`, and the page handle is live for `self`'s borrow.
        let mut status = unsafe {
            pdfium_sys::EPDF_RenderPageBitmapWithMatrix_Start(
                target.bitmap,
                self.handle,
                &target.matrix,
                &target.clip,
                flags.bits(),
                budget,
            )
        };
        let mut stopped = None;
        while status == FPDF_RENDER_TOBECONTINUED {
            if let Err(e) = between() {
                stopped = Some(e);
                break;
            }
            // SAFETY: a render of this page is in progress, started above.
            status = unsafe { pdfium_sys::EPDF_RenderPage_Continue(self.handle, budget) };
        }
        // SAFETY: called once after every start, whatever it returned, and
        // before `target` frees the bitmap the render draws into.
        unsafe { pdfium_sys::FPDF_RenderPage_Close(self.handle) };
        if let Some(e) = stopped {
            return Err(SlicedRender::Stopped(e));
        }
        if status != FPDF_RENDER_DONE {
            return Err(SlicedRender::Pdfium(PdfiumError::Render));
        }
        Ok(target.into_bitmap())
    }

    /// Renders ONE annotation's appearance stream into a transparent bitmap.
    ///
    /// The page is still required. An appearance stream is a form XObject in
    /// the page's resource space, so PDFium needs the page to resolve what it
    /// references, even though nothing of the page's own content is drawn.
    ///
    /// `state` is `None` to draw the state `/AS` selects, or the name of one
    /// state of the mode's appearance subdictionary, drawn whatever `/AS`
    /// says. A mode whose entry is a single stream, or a state it lacks,
    /// draws nothing.
    ///
    /// `turn` picks between two entry points rather than changing the
    /// matrix. With one, `EPDF_RenderAnnotBitmapUnrotated` draws the
    /// appearance as the page shows it and then turned back by
    /// `turn.degrees` about the middle of `turn.unrotated_box`, which is what
    /// lets a consumer re-apply the angle as a view transform and never
    /// re-render on rotate. The fork no longer finds the turn itself
    /// (runtime 5e9241945): the caller reads it and passes it in.
    #[allow(clippy::too_many_arguments)]
    pub fn render_annotation_rgba_with_matrix(
        &self,
        annotation: &crate::Annotation<'_>,
        mode: crate::AppearanceMode,
        state: Option<&str>,
        width: u32,
        height: u32,
        matrix: [f64; 6],
        turn: Option<AppearanceTurn>,
    ) -> Result<Bitmap, PdfiumError> {
        let state = state
            .map(CString::new)
            .transpose()
            .map_err(|_| PdfiumError::InvalidArgument("appearance state"))?;
        let state_ptr = state.as_ref().map_or(std::ptr::null(), |state| state.as_ptr());
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

        // SAFETY: the bitmap, page and annotation handles are all live, and
        // the matrix outlives the call. `state_ptr` is null or points into
        // `state`, a NUL-terminated CString that lives to the end of this
        // function. `&unrotated_box` is a local that lives across the one
        // call it is passed to.
        let ok = unsafe {
            // Transparent, not white: an appearance composites over page
            // content that this bitmap does not contain.
            pdfium_sys::FPDFBitmap_FillRect(bitmap, 0, 0, native_width, native_height, 0x0000_0000);
            let drawn = if let Some(turn) = turn {
                let unrotated_box = pdfium_sys::FS_RECTF {
                    left: turn.unrotated_box.left,
                    top: turn.unrotated_box.top,
                    right: turn.unrotated_box.right,
                    bottom: turn.unrotated_box.bottom,
                };
                pdfium_sys::EPDF_RenderAnnotBitmapUnrotated(
                    bitmap,
                    self.handle,
                    annotation.as_raw(),
                    mode as i32,
                    state_ptr,
                    turn.degrees,
                    &unrotated_box,
                    &fs_matrix,
                    FPDF_REVERSE_BYTE_ORDER,
                )
            } else {
                pdfium_sys::EPDF_RenderAnnotBitmap(
                    bitmap,
                    self.handle,
                    annotation.as_raw(),
                    mode as i32,
                    state_ptr,
                    &fs_matrix,
                    FPDF_REVERSE_BYTE_ORDER,
                )
            };
            pdfium_sys::FPDFBitmap_Destroy(bitmap);
            drawn
        };
        if ok == 0 {
            return Err(PdfiumError::Bitmap);
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

    /// `EPDFPage_GetParsedSize`: what the page's parsed content costs in
    /// memory, its estimate on the safe side. `None` when the fork reports
    /// nothing.
    pub fn parsed_size(&self) -> Option<ParsedSize> {
        let mut size = pdfium_sys::EPDF_PAGE_PARSED_SIZE_::default();
        // SAFETY: the handle is live for this borrow and `size` is a valid
        // out-parameter of the struct's own type.
        let ok = unsafe { pdfium_sys::EPDFPage_GetParsedSize(self.handle, &raw mut size) };
        (ok != 0).then_some(ParsedSize {
            objects: size.objects,
            path_points: size.path_points,
            text_chars: size.text_chars,
            estimated_bytes: size.estimated_bytes,
            complete: size.complete != 0,
        })
    }

    /// `EPDFPage_IsContentCurrent`: whether the parsed objects still match
    /// the document (its boxes, rotation and content streams), which a
    /// write in a layer transaction, or its abort, can change.
    pub fn is_content_current(&self) -> bool {
        // SAFETY: the handle is live for this borrow.
        unsafe { pdfium_sys::EPDFPage_IsContentCurrent(self.handle) != 0 }
    }

    /// `EPDFPage_ResetRenderCache`: empty the page's image cache, so its
    /// next render decodes images as a newly loaded page does. `false`, and
    /// nothing changed, while a progressive render of it is in progress.
    pub fn reset_render_cache(&self) -> bool {
        // SAFETY: the handle is live for this borrow.
        unsafe { pdfium_sys::EPDFPage_ResetRenderCache(self.handle) != 0 }
    }
}

/// What a page's parsed content costs (`EPDF_PAGE_PARSED_SIZE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedSize {
    pub objects: u64,
    pub path_points: u64,
    pub text_chars: u64,
    /// The bytes the objects hold, estimated on the safe side.
    pub estimated_bytes: u64,
    /// False while the page is still parsing.
    pub complete: bool,
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

    /// Wrap a page handle PDFium just created.
    ///
    /// # Safety
    /// `handle` must be a live `FPDF_PAGE` this wrapper may close, and it
    /// must belong to a document that outlives `'a`.
    pub(crate) unsafe fn from_raw_owned(handle: pdfium_sys::FPDF_PAGE) -> Option<Self> {
        // SAFETY: the caller promises the handle is live for `'a`.
        unsafe { PageView::from_raw(handle) }.map(|view| Self { view })
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
