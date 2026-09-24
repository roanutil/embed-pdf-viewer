//! Authoring an annotation's appearance from an image or a page.
//!
//! G11's stamp writer. Everything else in this crate either reads a
//! dictionary or writes one entry; this builds the `/AP` stream itself, which
//! is why it is the only place that touches page objects.
//!
//! # Who owns the object
//!
//! `FPDFPageObj_NewImageObj` hands back an object THIS code owns until
//! `FPDFAnnot_AppendObject` succeeds, and the annotation owns it afterwards.
//! There is no shared ownership and no reference count: destroying it after a
//! successful append is a double free, and dropping it after a failed one
//! leaks. [`ImageObject`] exists to make that transfer a move rather than a
//! convention, so neither mistake is expressible.

use crate::{Annotation, Document, MatrixF, PageView, PdfiumError, RectF};

/// How the fork's appearance writer fits content into a box.
///
/// `EPDF_STAMP_FIT` from `public/fpdf_annot.h`, named after CSS `object-fit`
/// because the wire vocabulary is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StampFit {
    /// The whole content, letterboxed. Aspect preserved.
    #[default]
    Contain,
    /// The whole box, cropped. Aspect preserved.
    Cover,
    /// The whole box, distorted. Aspect discarded.
    Stretch,
}

impl StampFit {
    fn code(self) -> u32 {
        match self {
            Self::Contain => 0,
            Self::Cover => 1,
            Self::Stretch => 2,
        }
    }
}

/// An image object that has not been appended yet.
///
/// Destroys itself on drop, so every path that fails between creating one and
/// appending it frees it. [`Annotation::append_object`] consumes it, which is
/// what stops the drop from running after the annotation has taken it.
#[derive(Debug)]
pub struct ImageObject {
    handle: pdfium_sys::FPDF_PAGEOBJECT,
}

impl ImageObject {
    /// Decode PNG bytes into this object.
    ///
    /// `page` is the page the image will be drawn on. PDFium takes an ARRAY
    /// of pages, because one image object can be shared across several, and
    /// this passes exactly one: an annotation's appearance belongs to the
    /// page the annotation is on.
    pub fn set_png(&self, page: &PageView<'_>, bytes: &[u8]) -> bool {
        self.set_encoded(page, bytes, true)
    }

    /// The same for JPEG.
    pub fn set_jpeg(&self, page: &PageView<'_>, bytes: &[u8]) -> bool {
        self.set_encoded(page, bytes, false)
    }

    fn set_encoded(&self, page: &PageView<'_>, bytes: &[u8], png: bool) -> bool {
        // SAFETY: borrowed for the duration of this call only.
        let mut pages = [unsafe { page.as_raw() }];
        // SAFETY: the object and page handles are live, and both the page
        // array and the byte buffer outlive the call.
        unsafe {
            let call = if png {
                pdfium_sys::EPDFImageObj_SetPng
            } else {
                pdfium_sys::EPDFImageObj_SetJpeg
            };
            call(pages.as_mut_ptr(), 1, self.handle, bytes.as_ptr(), bytes.len()) != 0
        }
    }

    /// The object's own matrix, which for an image is its size in points.
    ///
    /// An image object's unit square is its whole image, so scaling by the
    /// width and height is how it gets a size at all: an object with the
    /// identity matrix draws one point across.
    pub fn set_matrix(&self, matrix: MatrixF) -> bool {
        let native = pdfium_sys::FS_MATRIX {
            a: matrix.a,
            b: matrix.b,
            c: matrix.c,
            d: matrix.d,
            e: matrix.e,
            f: matrix.f,
        };
        // SAFETY: the handle is live and `native` outlives the call.
        unsafe { pdfium_sys::FPDFPageObj_SetMatrix(self.handle, &raw const native) != 0 }
    }

    /// Post-multiply a transform onto the object.
    pub fn transform(&self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFPageObj_Transform(self.handle, a, b, c, d, e, f) };
    }
}

impl Drop for ImageObject {
    fn drop(&mut self) {
        // Only reached when the object was never appended: a successful
        // append consumes `self` and forgets it.
        // SAFETY: created by `Document::new_image_object` and freed once.
        unsafe { pdfium_sys::FPDFPageObj_Destroy(self.handle) };
    }
}

impl Document {
    /// Open another document on the same library as this one.
    ///
    /// For the stamp path, where a single-page PDF is opened purely to be
    /// cloned into an `/AP` and closed again. It takes the library from an
    /// already-open document because that is the proof it is initialized on
    /// this thread: there is no way to hold a `Document` otherwise.
    pub fn open_sibling(&self, bytes: &[u8]) -> Result<Self, PdfiumError> {
        Self::from_bytes(self.library(), bytes, None)
    }

    /// Allocate an image object against this document.
    pub fn new_image_object(&self) -> Option<ImageObject> {
        // SAFETY: the handle is live.
        let handle = unsafe { pdfium_sys::FPDFPageObj_NewImageObj(self.as_raw()) };
        (!handle.is_null()).then_some(ImageObject { handle })
    }
}

impl Annotation<'_> {
    /// How many objects the appearance holds.
    pub fn object_count(&self) -> i32 {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_GetObjectCount(self.as_raw()) }
    }

    /// Remove one, by index.
    pub fn remove_object(&self, index: i32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFAnnot_RemoveObject(self.as_raw(), index) != 0 }
    }

    /// Take ownership of an object into the appearance.
    ///
    /// Consumes the object on success and hands it back on failure, so the
    /// caller always holds exactly the objects it still owns.
    pub fn append_object(&self, object: ImageObject) -> Result<(), ImageObject> {
        // SAFETY: both handles are live.
        let ok = unsafe { pdfium_sys::FPDFAnnot_AppendObject(self.as_raw(), object.handle) != 0 };
        if ok {
            // The annotation owns it now, so the drop must not run.
            std::mem::forget(object);
            Ok(())
        } else {
            Err(object)
        }
    }

    /// The page-space box one appearance object occupies.
    ///
    /// The read side of what [`ImageObject::set_matrix`] writes, and the only
    /// way to see where a fit policy actually put the content: the DTO
    /// carries the annotation's `/Rect` and never the placement inside it.
    pub fn object_bounds(&self, index: i32) -> Option<RectF> {
        // SAFETY: the handle is live; a bad index answers null.
        let object = unsafe { pdfium_sys::FPDFAnnot_GetObject(self.as_raw(), index) };
        if object.is_null() {
            return None;
        }
        let (mut left, mut bottom, mut right, mut top) = (0f32, 0f32, 0f32, 0f32);
        // SAFETY: four initialized locals, as the signature asks for.
        let ok = unsafe {
            pdfium_sys::FPDFPageObj_GetBounds(
                object,
                &raw mut left,
                &raw mut bottom,
                &raw mut right,
                &raw mut top,
            )
        };
        (ok != 0).then_some(RectF { left, bottom, right, top })
    }

    /// Normalize the appearance into `/Rect` under a fit policy.
    ///
    /// Also records the fork's `EPDFOrigContentRect`, which is what lets a
    /// later geometry-only change re-fit without the content being scaled a
    /// second time, and applies any `/EMBD_Metadata` rotation as an `/AP`
    /// `/Matrix`.
    pub fn update_appearance_to_rect(&self, fit: StampFit) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_UpdateAppearanceToRect(self.as_raw(), fit.code()) != 0 }
    }

    /// Deep-clone a page of another document in as this annotation's `/AP`.
    ///
    /// The source document has to stay open for the call and not after it:
    /// the clone is a copy, not a reference.
    pub fn set_appearance_from_page(&self, source: &Document, page_index: i32) -> bool {
        // SAFETY: both handles are live for the call.
        unsafe {
            pdfium_sys::EPDFAnnot_SetAppearanceFromPage(self.as_raw(), source.as_raw(), page_index)
                != 0
        }
    }
}
