//! The fork's annotation captures and the layer transactions an import runs
//! in (`public/epdf_capture.h`, `public/epdf_layer.h`), and the raw block
//! move a reorder writes (`EPDFPage_MoveAnnotsRaw`).
//!
//! A capture holds the layer's own versions of what a delete is about to
//! remove, as bytes, and its import puts them back. Which objects to capture,
//! and when an import is allowed, is the engine's decision; these only move
//! the bytes. Every export works on a layer document only, and every import
//! runs inside a layer transaction.

use crate::Document;

impl Document {
    /// `EPDFPage_ExportAnnotsRawToOwnedBuffer`: the annotations at `indexes`
    /// of page `page_index`'s `/Annots`, with their positions, and every
    /// object the layer holds a version of that they reach. `None` for a
    /// document that is not a layer, or a member that is not an indirect
    /// object (promote the page first).
    pub fn export_annotations_raw(&self, page_index: u32, indexes: &[u32]) -> Option<Vec<u8>> {
        let page_index = i32::try_from(page_index).ok()?;
        let indexes: Vec<i32> =
            indexes.iter().map(|index| i32::try_from(*index).ok()).collect::<Option<_>>()?;
        let count = i32::try_from(indexes.len()).ok()?;
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live, `indexes` outlives the call and
        // `size` is a valid out-parameter.
        let buffer = unsafe {
            pdfium_sys::EPDFPage_ExportAnnotsRawToOwnedBuffer(
                self.as_raw(),
                page_index,
                indexes.as_ptr(),
                count,
                &raw mut size,
            )
        };
        if buffer.is_null() {
            return None;
        }
        let length = usize::try_from(size).unwrap_or(0);
        // SAFETY: the fork reported `size` readable bytes at `buffer`.
        let bytes = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), length) }.to_vec();
        // SAFETY: the buffer came from the export above and is freed once.
        unsafe { pdfium_sys::EPDF_FreeBuffer(buffer) };
        Some(bytes)
    }

    /// `EPDFPage_ImportAnnotsRaw`: put the annotations of `capture` back on
    /// page `page_index`, each at the position it had. `false` outside a
    /// layer transaction, for a malformed capture, or when the page is not
    /// the one it was taken on.
    pub fn import_annotations_raw(&self, page_index: u32, capture: &[u8]) -> bool {
        let (Ok(page_index), Ok(size)) =
            (i32::try_from(page_index), std::os::raw::c_ulong::try_from(capture.len()))
        else {
            return false;
        };
        // SAFETY: the handle is live and `capture` outlives the call.
        unsafe {
            pdfium_sys::EPDFPage_ImportAnnotsRaw(
                self.as_raw(),
                page_index,
                capture.as_ptr().cast(),
                size,
            ) != 0
        }
    }

    /// `EPDFLayer_BeginTransaction`: `false` for a document that is not a
    /// layer, or when one is already open.
    pub fn layer_begin_transaction(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFLayer_BeginTransaction(self.as_raw()) != 0 }
    }

    /// `EPDFLayer_CommitTransaction`: `false` when none is open.
    pub fn layer_commit_transaction(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFLayer_CommitTransaction(self.as_raw()) != 0 }
    }

    /// `EPDFLayer_AbortTransaction`: `false` when none is open.
    pub fn layer_abort_transaction(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFLayer_AbortTransaction(self.as_raw()) != 0 }
    }

    /// `EPDFLayer_IsInTransaction`.
    pub fn layer_in_transaction(&self) -> bool {
        // SAFETY: the handle is live.
        unsafe { pdfium_sys::EPDFLayer_IsInTransaction(self.as_raw()) != 0 }
    }

    /// `EPDFPage_MoveAnnotsRaw`: move the entries at `from_indices` of page
    /// `page_index`'s `/Annots`, together and in the order given, to
    /// `to_index` of the list left once they are out, without loading the
    /// page. A request the fork refuses changes nothing and answers `false`.
    pub fn move_annotations_raw(
        &self,
        page_index: u32,
        from_indices: &[u32],
        to_index: u32,
    ) -> bool {
        let Some(from): Option<Vec<i32>> =
            from_indices.iter().map(|index| i32::try_from(*index).ok()).collect()
        else {
            return false;
        };
        let (Ok(page_index), Ok(count), Ok(to_index)) =
            (i32::try_from(page_index), i32::try_from(from.len()), i32::try_from(to_index))
        else {
            return false;
        };
        // SAFETY: the handle is live and `from` outlives the call.
        unsafe {
            pdfium_sys::EPDFPage_MoveAnnotsRaw(
                self.as_raw(),
                page_index,
                from.as_ptr(),
                count,
                to_index,
            ) != 0
        }
    }
}
