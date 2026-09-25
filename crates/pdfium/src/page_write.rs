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
