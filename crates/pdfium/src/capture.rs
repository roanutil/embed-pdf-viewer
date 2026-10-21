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

/// The bytes of a buffer an export returned, which is then released; `None`
/// for a null buffer.
fn take_owned(buffer: *mut std::ffi::c_void, size: std::os::raw::c_ulong) -> Option<Vec<u8>> {
    if buffer.is_null() {
        return None;
    }
    let length = usize::try_from(size).unwrap_or(0);
    // SAFETY: the fork reported `size` readable bytes at `buffer`.
    let bytes = unsafe { std::slice::from_raw_parts(buffer.cast::<u8>(), length) }.to_vec();
    // SAFETY: the buffer came from an export and is freed once.
    unsafe { pdfium_sys::EPDF_FreeBuffer(buffer) };
    Some(bytes)
}

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

    /// `EPDFPage_ExportRawToOwnedBuffer`: the page `page_object_number` as a
    /// delete removes it, with every object the layer holds a version of
    /// that it reaches, the `/Names /Pages` keys that name it, the page it
    /// followed, and what it takes from the page tree above it. `None` when
    /// it is not a page in the page tree, or the document is not a layer.
    pub fn export_page_raw(&self, page_object_number: u32) -> Option<Vec<u8>> {
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and `size` is a valid out-parameter.
        let buffer = unsafe {
            pdfium_sys::EPDFPage_ExportRawToOwnedBuffer(
                self.as_raw(),
                page_object_number,
                &raw mut size,
            )
        };
        take_owned(buffer, size)
    }

    /// `EPDFPage_ImportRaw`: put the page of `capture` back into the page
    /// tree, right after the page it followed while that is there (else
    /// first), with its names. `false` outside a layer transaction, for a
    /// malformed capture, or when the page is in the page tree already.
    pub fn import_page_raw(&self, capture: &[u8]) -> bool {
        let Ok(size) = std::os::raw::c_ulong::try_from(capture.len()) else {
            return false;
        };
        // SAFETY: the handle is live and `capture` outlives the call.
        unsafe { pdfium_sys::EPDFPage_ImportRaw(self.as_raw(), capture.as_ptr().cast(), size) != 0 }
    }

    /// `EPDFForm_ExportFieldRawToOwnedBuffer`: the terminal field
    /// `field_object_number` as `EPDFForm_DeleteField` removes it, with its
    /// widgets, their pages and positions, the ancestors the delete prunes,
    /// and every object the layer holds a version of that they reach.
    /// `None` for a field that isn't terminal, or a document that is not a
    /// layer.
    pub fn export_field_raw(&self, field_object_number: u32) -> Option<Vec<u8>> {
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and `size` is a valid out-parameter.
        let buffer = unsafe {
            pdfium_sys::EPDFForm_ExportFieldRawToOwnedBuffer(
                self.as_raw(),
                field_object_number,
                &raw mut size,
            )
        };
        take_owned(buffer, size)
    }

    /// `EPDFForm_ImportFieldRaw`: put the field of `capture` back, its
    /// ancestors and each widget at the positions they had. `false` outside
    /// a layer transaction, for a malformed capture, or when a container or
    /// a widget's page is gone.
    pub fn import_field_raw(&self, capture: &[u8]) -> bool {
        let Ok(size) = std::os::raw::c_ulong::try_from(capture.len()) else {
            return false;
        };
        // SAFETY: the handle is live and `capture` outlives the call.
        unsafe {
            pdfium_sys::EPDFForm_ImportFieldRaw(self.as_raw(), capture.as_ptr().cast(), size) != 0
        }
    }

    /// `EPDFDoc_ExportDictCanonicalToOwnedBuffer`: the dictionary
    /// `object_number` as bytes a guard compares, without the keys in
    /// `skip_keys` and with every object the layer holds under `deep_keys`,
    /// each stream as it reads. Keys are separated by spaces. `None` when
    /// it is not a dictionary.
    pub fn export_dict_canonical(
        &self,
        object_number: u32,
        deep_keys: &str,
        skip_keys: &str,
    ) -> Option<Vec<u8>> {
        let deep = std::ffi::CString::new(deep_keys).ok()?;
        let skip = std::ffi::CString::new(skip_keys).ok()?;
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live, both strings outlive the call and
        // `size` is a valid out-parameter.
        let buffer = unsafe {
            pdfium_sys::EPDFDoc_ExportDictCanonicalToOwnedBuffer(
                self.as_raw(),
                object_number,
                deep.as_ptr(),
                skip.as_ptr(),
                &raw mut size,
            )
        };
        take_owned(buffer, size)
    }

    /// `EPDFDoc_ExportDictRawToOwnedBuffer`: the dictionary `object_number`
    /// as it reads now, every key with its value, and under the keys in
    /// `deep_keys` (separated by spaces, such as `"AP MK"`) every object the
    /// layer holds a version of that they reach. `None` when it is not a
    /// dictionary.
    pub fn export_dict_raw(&self, object_number: u32, deep_keys: &str) -> Option<Vec<u8>> {
        let deep = std::ffi::CString::new(deep_keys).ok()?;
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live, the string outlives the call and
        // `size` is a valid out-parameter.
        let buffer = unsafe {
            pdfium_sys::EPDFDoc_ExportDictRawToOwnedBuffer(
                self.as_raw(),
                object_number,
                deep.as_ptr(),
                &raw mut size,
            )
        };
        take_owned(buffer, size)
    }

    /// `EPDFDoc_ImportDictRaw`: put the dictionary of `capture` back as it
    /// was, the objects under its deep keys where the layer holds no version
    /// and then exactly the keys it held. `false` outside a layer
    /// transaction, for a malformed capture, or when the object is something
    /// other than a dictionary now.
    pub fn import_dict_raw(&self, capture: &[u8]) -> bool {
        let Ok(size) = std::os::raw::c_ulong::try_from(capture.len()) else {
            return false;
        };
        // SAFETY: the handle is live and `capture` outlives the call.
        unsafe {
            pdfium_sys::EPDFDoc_ImportDictRaw(self.as_raw(), capture.as_ptr().cast(), size) != 0
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
