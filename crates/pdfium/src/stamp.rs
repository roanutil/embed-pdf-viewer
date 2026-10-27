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

    /// `/CA` set and the appearance re-fit as [`Self::update_appearance_to_rect`]
    /// does, its opacity layer replaced for the new value rather than kept as
    /// drawing (`EPDFAnnot_SetStampOpacity`). On failure nothing changes.
    pub fn set_stamp_opacity(&self, fit: StampFit, opacity: f32) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetStampOpacity(self.as_raw(), fit.code(), opacity) != 0 }
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

/// Acrobat's page size limit: an image drawing larger than this is scaled
/// down to fit (`MAX_DRAWING_SIZE` in upstream's `stampDrawing.ts`).
const MAX_DRAWING_SIZE: f64 = 14_400.0;

impl Document {
    /// A one-page PDF's first page as a stamp drawing in a document of its
    /// own (`EPDFDoc_CanonicalDrawing`): what the stamp places, independent
    /// of any stamp's box or fit. `Err(Load)` when the bytes do not open.
    pub fn canonical_drawing_of_pdf(&self, bytes: &[u8]) -> Result<Document, PdfiumError> {
        let source = self.open_sibling(bytes)?;
        // SAFETY: the source handle is live for the call.
        let handle = unsafe { pdfium_sys::EPDFDoc_CanonicalDrawing(source.as_raw(), 0) };
        drop(source);
        self.own_scratch(handle).ok_or(PdfiumError::Load)
    }

    /// PNG or JPEG bytes as a canonical stamp drawing: the image at its own
    /// size on a page of its own, a pixel a point, scaled down to fit
    /// [`MAX_DRAWING_SIZE`] (`withImageDocument`). `Ok(None)` when PDFium
    /// refuses the image data.
    pub fn canonical_drawing_of_image(
        &self,
        bytes: &[u8],
        png: bool,
        width: u32,
        height: u32,
    ) -> Result<Option<Document>, PdfiumError> {
        let (width, height) = (f64::from(width), f64::from(height));
        let scale = (MAX_DRAWING_SIZE / width.max(height)).min(1.0);
        let (width, height) = (width * scale, height * scale);
        // SAFETY: no arguments; a null return is the documented failure.
        let scratch = self
            .own_scratch(unsafe { pdfium_sys::FPDF_CreateNewDocument() })
            .ok_or(PdfiumError::Load)?;
        {
            // SAFETY: the scratch handle is live.
            let page = unsafe { pdfium_sys::FPDFPage_New(scratch.as_raw(), 0, width, height) };
            if page.is_null() {
                return Err(PdfiumError::Load);
            }
            struct ClosePage(pdfium_sys::FPDF_PAGE);
            impl Drop for ClosePage {
                fn drop(&mut self) {
                    // SAFETY: created above and closed exactly once.
                    unsafe { pdfium_sys::FPDF_ClosePage(self.0) };
                }
            }
            let page = ClosePage(page);
            let object = scratch.new_image_object().ok_or(PdfiumError::Load)?;
            // SAFETY: the object handle is live and the bytes outlive the
            // call; no page array, as upstream passes none.
            let decoded = unsafe {
                let call = if png {
                    pdfium_sys::EPDFImageObj_SetPng
                } else {
                    pdfium_sys::EPDFImageObj_SetJpeg
                };
                call(std::ptr::null_mut(), 0, object.handle, bytes.as_ptr(), bytes.len()) != 0
            };
            if !decoded {
                return Ok(None);
            }
            if !object.set_matrix(MatrixF {
                a: width as f32,
                b: 0.0,
                c: 0.0,
                d: height as f32,
                e: 0.0,
                f: 0.0,
            }) {
                return Err(PdfiumError::Load);
            }
            // SAFETY: both handles are live; the page owns the object after.
            unsafe { pdfium_sys::FPDFPage_InsertObject(page.0, object.handle) };
            std::mem::forget(object);
            // SAFETY: the page handle is live.
            if unsafe { pdfium_sys::FPDFPage_GenerateContent(page.0) } == 0 {
                return Err(PdfiumError::Load);
            }
        }
        // SAFETY: the scratch handle is live for the call.
        let handle = unsafe { pdfium_sys::EPDFDoc_CanonicalDrawing(scratch.as_raw(), 0) };
        drop(scratch);
        Ok(Some(self.own_scratch(handle).ok_or(PdfiumError::Load)?))
    }

    /// Add a canonical drawing to this document, answering its object
    /// number (`EPDFDoc_ImportDrawing`); `None` when refused.
    pub fn import_drawing(&self, canonical: &Document) -> Option<u32> {
        // SAFETY: both handles are live.
        let drawing =
            unsafe { pdfium_sys::EPDFDoc_ImportDrawing(self.as_raw(), canonical.as_raw()) };
        (drawing != 0).then_some(drawing)
    }

    /// The drawings this document's stamps place (`EPDFDoc_GetStampDrawings`).
    pub fn stamp_drawings(&self) -> Vec<u32> {
        // SAFETY: a null buffer asks for the count.
        let count =
            unsafe { pdfium_sys::EPDFDoc_GetStampDrawings(self.as_raw(), std::ptr::null_mut(), 0) };
        if count == 0 {
            return Vec::new();
        }
        let mut drawings = vec![0u32; count as usize];
        // SAFETY: the buffer holds `count` entries, as the length says.
        let written = unsafe {
            pdfium_sys::EPDFDoc_GetStampDrawings(self.as_raw(), drawings.as_mut_ptr(), count)
        };
        drawings.truncate((written as usize).min(drawings.len()));
        drawings
    }

    /// One drawing of this document as a document of its own
    /// (`EPDFDoc_ExportDrawing`).
    pub fn export_drawing(&self, drawing: u32) -> Option<Document> {
        // SAFETY: the handle is live.
        let handle = unsafe { pdfium_sys::EPDFDoc_ExportDrawing(self.as_raw(), drawing) };
        self.own_scratch(handle)
    }

    /// A document the fork made for this one, owned and closed on drop.
    fn own_scratch(&self, handle: pdfium_sys::FPDF_DOCUMENT) -> Option<Document> {
        // SAFETY: a non-null handle the fork handed over to be closed once;
        // it points into nothing of this document's.
        (!handle.is_null()).then(|| unsafe { Document::from_raw_layer(handle, self.library()) })
    }
}

/// SHA-256 of `bytes` through the fork's digest (`EPDF_DigestBuffer`), or
/// `None` when it refuses.
pub fn sha256(bytes: &[u8]) -> Option<[u8; 32]> {
    let mut out = [0u8; 32];
    let mut len: std::os::raw::c_ulong = 32;
    // SAFETY: the input outlives the call, the output holds 32 bytes, and
    // `len` says so.
    let ok = unsafe {
        pdfium_sys::EPDF_DigestBuffer(
            bytes.as_ptr().cast(),
            bytes.len() as std::os::raw::c_ulong,
            pdfium_sys::EPDF_DIGEST_SHA256 as i32,
            out.as_mut_ptr(),
            &raw mut len,
        )
    };
    (ok != 0 && len == 32).then_some(out)
}

impl Annotation<'_> {
    /// The drawing this stamp places under our wrapper, by object number
    /// (`EPDFAnnot_GetStampDrawing`); `None` for a stamp without one, or an
    /// annotation that is not a stamp.
    pub fn stamp_drawing(&self) -> Option<u32> {
        // SAFETY: the handle is live.
        let drawing = unsafe { pdfium_sys::EPDFAnnot_GetStampDrawing(self.as_raw()) };
        (drawing != 0).then_some(drawing)
    }

    /// Place a drawing of the document in this stamp, under a wrapper of its
    /// own that fits it into the box (`EPDFAnnot_SetStampDrawing`), honouring
    /// any `/EMBD_Metadata` turn. The old appearance is replaced whole.
    pub fn set_stamp_drawing(&self, drawing: u32, fit: StampFit) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFAnnot_SetStampDrawing(self.as_raw(), drawing, fit.code()) != 0 }
    }
}
