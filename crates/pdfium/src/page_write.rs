//! Changing a document's page list.
//!
//! G13. The write half of [`crate::page_list`]: rotating, reordering and
//! removing pages.
//!
//! # Two addressing schemes, and only one of them is durable
//!
//! `FPDF_MovePages` speaks INDICES, because a reorder is defined over
//! positions and the same page has a different one before and after.
//! Everything else speaks OBJECT NUMBERS, because a page's identity has to
//! survive the reorder that changed its index. Translating between them is
//! the engine's, and it is why the move wrapper takes indices while the
//! rotate and delete wrappers do not.
//!
//! A deleted page's object number is RETIRED rather than freed: the fork
//! nulls the object instead of reusing the slot, so per-page state a caller
//! kept can never silently attach to an unrelated future page.

use crate::Document;

impl Document {
    /// Reorder pages.
    ///
    /// The pages at `from_indices` are detached and re-inserted as one
    /// contiguous block at `destination`, in the order given, and
    /// `destination` is a position in the POST-REMOVAL index space.
    ///
    /// Atomic: a false return means nothing moved. PDFium validates the
    /// whole request before it touches the tree, which is what lets this
    /// layer pass a refusal straight through rather than trying to
    /// reconstruct what happened.
    pub fn move_pages(&self, from_indices: &[i32], destination: i32) -> bool {
        // SAFETY: the handle is live and the slice outlives the call.
        unsafe {
            pdfium_sys::FPDF_MovePages(
                self.as_raw(),
                from_indices.as_ptr(),
                from_indices.len() as std::os::raw::c_ulong,
                destination,
            ) != 0
        }
    }

    /// `/Rotate`, in quarter turns.
    ///
    /// Quarter turns rather than degrees, which is what the fork's helper
    /// takes and what `/Rotate` divided by ninety is. The wire speaks
    /// degrees and the engine converts, so the two vocabularies meet in one
    /// place rather than at every call.
    pub fn set_page_rotation_by_object_number(
        &self,
        page_object_number: u32,
        quarter_turns: i32,
    ) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFDoc_SetPageRotationByObjectNumber(
                self.as_raw(),
                page_object_number,
                quarter_turns,
            ) != 0
        }
    }
}

impl crate::PageView<'_> {
    /// What a flatten did to one page.
    ///
    /// `EPDF_FLATTEN_*`. The middle value is the interesting one: a page
    /// with nothing to flatten is not a failure, and collapsing the two
    /// would make "this page had no annotations" indistinguishable from
    /// "this page could not be flattened".
    /// Apply every redaction marking on this page.
    ///
    /// Destructive: the content under each marking is removed and the
    /// marking with it. `None` is PDFium's false, which is OVERLOADED: it
    /// means either that the page had no markings or that applying one
    /// failed. The engine tells the two apart by looking first, because a
    /// sanitizer failure reported as an unchanged page is a document
    /// somebody believes was redacted.
    pub fn apply_redactions(&self) -> Option<u32> {
        let mut removed = 0u32;
        // SAFETY: the page handle is live and `removed` is a valid
        // out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFPage_ApplyRedactions(
                self.as_raw(),
                std::ptr::null(),
                0,
                &raw mut removed,
            )
        };
        (ok != 0).then_some(removed)
    }

    /// Apply one marking.
    ///
    /// The count is COLLATERAL: annotations other than the marking that
    /// went with it, which is popups cascading and widgets detaching.
    pub fn apply_redaction(&self, annotation: &crate::Annotation<'_>) -> Option<u32> {
        self.apply_redaction_marks(std::slice::from_ref(annotation))
    }

    /// Apply these markings together, in one pass.
    ///
    /// `EPDFPage_ApplyRedactions` with explicit marks: their regions are
    /// applied at once, so each image under them is read and written once.
    /// `None` is PDFium's false: a mark that is not a redaction of this page,
    /// nothing to apply, or a failure. An empty slice is refused here rather
    /// than passed through, because the native call reads an empty list as
    /// "every marking on the page".
    pub fn apply_redaction_marks(&self, marks: &[crate::Annotation<'_>]) -> Option<u32> {
        if marks.is_empty() {
            return None;
        }
        let handles: Vec<pdfium_sys::FPDF_ANNOTATION> =
            marks.iter().map(|annotation| annotation.as_raw()).collect();
        let count = i32::try_from(handles.len()).ok()?;
        let mut removed = 0u32;
        // SAFETY: the page handle and every annotation handle are live, the
        // handle array is `count` long and outlives the call, and `removed`
        // is a valid out-parameter.
        let ok = unsafe {
            pdfium_sys::EPDFPage_ApplyRedactions(
                self.as_raw(),
                handles.as_ptr(),
                count,
                &raw mut removed,
            )
        };
        (ok != 0).then_some(removed)
    }

    pub fn flatten(&self, usage: FlattenUsage) -> FlattenOutcome {
        // SAFETY: the page handle is live.
        let code = unsafe { pdfium_sys::EPDFPage_Flatten(self.as_raw(), usage as i32) };
        match code {
            0 => FlattenOutcome::Failed,
            1 => FlattenOutcome::Applied,
            2 => FlattenOutcome::NothingToDo,
            other => FlattenOutcome::Unknown(other),
        }
    }
}

/// Which appearance a flatten bakes.
///
/// An annotation may carry a different stream for each, and a hidden-on-
/// screen stamp that prints is the case that makes the distinction matter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenUsage {
    Display = 0,
    Print = 1,
}

/// What a flatten did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlattenOutcome {
    /// The page's annotations are now content.
    Applied,
    /// There was nothing to flatten, which is not a failure.
    NothingToDo,
    Failed,
    /// A code this build does not know, kept rather than collapsed.
    Unknown(i32),
}

impl Document {
    /// Create a blank page at an index.
    ///
    /// Returns a handle the caller must close. PDFium CLAMPS an index past
    /// the end to an append rather than refusing it, so a caller that wants
    /// the range enforced checks before calling: this layer passes the
    /// leniency through rather than inventing a contract PDFium does not
    /// have.
    pub fn new_page(&self, index: i32, width: f64, height: f64) -> Option<crate::Page<'_>> {
        // SAFETY: the handle is live for the borrow this returns.
        let handle = unsafe { pdfium_sys::FPDFPage_New(self.as_raw(), index, width, height) };
        // SAFETY: a non-null result is a page this document now owns and
        // this wrapper closes.
        unsafe { crate::Page::from_raw_owned(handle) }
    }

    /// Remove the page at an index.
    ///
    /// By INDEX, unlike [`Document::delete_page_by_object_number`], because
    /// its one caller is undoing pages it just created and knows where they
    /// are. No return value: PDFium has none.
    pub fn delete_page_at(&self, index: i32) {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::FPDFPage_Delete(self.as_raw(), index) };
    }

    /// Deep-copy pages out of another document.
    ///
    /// `indices` empty means every page, in order. The copy is not a full
    /// detach: imported streams still read through the SOURCE document, so
    /// it has to stay open for as long as the destination might be saved.
    /// That lifetime is the caller's to hold, which is why this takes a
    /// borrow rather than consuming the source.
    pub fn import_pages(&self, source: &Document, indices: &[i32], at: i32) -> bool {
        let (pointer, length) = if indices.is_empty() {
            (std::ptr::null(), 0)
        } else {
            (indices.as_ptr(), indices.len() as std::os::raw::c_ulong)
        };
        // SAFETY: both handles are live and the slice outlives the call.
        unsafe {
            pdfium_sys::FPDF_ImportPagesByIndex(self.as_raw(), source.as_raw(), pointer, length, at)
                != 0
        }
    }

    /// Remove a page.
    ///
    /// The object number is retired, not freed.
    pub fn delete_page_by_object_number(&self, page_object_number: u32) -> bool {
        // SAFETY: the handle is live.
        unsafe {
            pdfium_sys::EPDFDoc_DeletePageByObjectNumber(self.as_raw(), page_object_number) != 0
        }
    }
}

/// One annotation's outcome in [`crate::PageView::flatten_annotations`]:
/// `EPDF_FLATTEN_STATUS_*`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationFlattenStatus {
    /// Painted into the page content and removed from `/Annots`.
    Applied,
    /// Left in place: hidden for the usage, a Popup, or without a usable
    /// normal appearance.
    Skipped,
    /// Not an annotation of the page; the call failed whole.
    NotOnPage,
    /// A code this build does not know, kept rather than collapsed.
    Unknown(i32),
}

impl AnnotationFlattenStatus {
    fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Applied,
            1 => Self::Skipped,
            2 => Self::NotOnPage,
            other => Self::Unknown(other),
        }
    }
}

/// `FPDF_SAVE_FLAGS`: a whole file rather than an incremental update.
const FPDF_NO_INCREMENTAL: std::os::raw::c_ulong = 1 << 1;

impl crate::PageView<'_> {
    /// `EPDFPage_FlattenAnnotations`: flatten a chosen set of this page's
    /// annotations. All-or-nothing on validity: a handle that is not on
    /// this page fails the call and changes nothing.
    ///
    /// The handles must be dropped (closed) before they are used again: a
    /// flattened annotation is gone from the page.
    pub fn flatten_annotations(
        &self,
        annotations: &[crate::Annotation<'_>],
        usage: FlattenUsage,
    ) -> (FlattenOutcome, Vec<AnnotationFlattenStatus>) {
        let mut handles: Vec<pdfium_sys::FPDF_ANNOTATION> =
            annotations.iter().map(crate::Annotation::as_raw).collect();
        let mut statuses = vec![0i32; handles.len()];
        // SAFETY: the page and every annotation handle are live, and both
        // arrays hold `handles.len()` entries as the count claims.
        let code = unsafe {
            pdfium_sys::EPDFPage_FlattenAnnotations(
                self.as_raw(),
                handles.as_mut_ptr(),
                handles.len() as i32,
                usage as i32,
                statuses.as_mut_ptr(),
            )
        };
        let outcome = match code {
            0 => FlattenOutcome::Failed,
            1 => FlattenOutcome::Applied,
            2 => FlattenOutcome::NothingToDo,
            other => FlattenOutcome::Unknown(other),
        };
        (outcome, statuses.into_iter().map(AnnotationFlattenStatus::from_code).collect())
    }

    /// `EPDFPage_ExportAnnotationsAsDocument`, saved whole: the normal
    /// appearances of a chosen set of this page's annotations as a new
    /// single-page PDF. The source document is untouched.
    ///
    /// `Ok(None)` when the fork refuses (any entry hidden, a Popup, without
    /// a normal appearance, or not on this page), `Err(Save)` when the new
    /// document would not serialize.
    pub fn export_annotations_as_document(
        &self,
        annotations: &[crate::Annotation<'_>],
    ) -> Result<Option<Vec<u8>>, crate::PdfiumError> {
        let mut handles: Vec<pdfium_sys::FPDF_ANNOTATION> =
            annotations.iter().map(crate::Annotation::as_raw).collect();
        // SAFETY: the page and every annotation handle are live, and the
        // array holds `handles.len()` entries.
        let exported = unsafe {
            pdfium_sys::EPDFPage_ExportAnnotationsAsDocument(
                self.as_raw(),
                handles.as_mut_ptr(),
                handles.len() as i32,
            )
        };
        if exported.is_null() {
            return Ok(None);
        }
        struct Scratch(pdfium_sys::FPDF_DOCUMENT);
        impl Drop for Scratch {
            fn drop(&mut self) {
                // SAFETY: created above and closed exactly once.
                unsafe { pdfium_sys::FPDF_CloseDocument(self.0) };
            }
        }
        let scratch = Scratch(exported);
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and `size` is a valid out-parameter.
        let ptr = unsafe {
            pdfium_sys::EPDF_SaveDocumentToOwnedBuffer(
                scratch.0,
                FPDF_NO_INCREMENTAL,
                &raw mut size,
            )
        };
        crate::save::owned_buffer(ptr, size).map(Some).ok_or(crate::PdfiumError::Save)
    }
}
