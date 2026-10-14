//! Serializing an open document.
//!
//! G10. Wraps `EPDF_SaveDocumentToOwnedBuffer` and the layer-artifact
//! writers.
//!
//! # Two outputs that are not the same file
//!
//! A STANDALONE save is a self-contained PDF: the base bytes plus whatever
//! has been layered on top, written as a document any reader opens. A LAYER
//! ARTIFACT is the storage format this project's server persistence uses, and
//! it is not a PDF at all. Confusing them produces a file that either fails
//! to open or silently drops every edit, so they are separate calls with
//! separate names rather than one call with a flag.
//!
//! A layer document can produce either. Only a layer document can produce the
//! artifact, which is the one precondition this layer does NOT enforce: it
//! has no way to tell a layer handle from a base one, so the engine checks.

use crate::{Document, PdfiumError};

/// How much of the file to rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfSaveMode {
    /// Append a revision to the original bytes, leaving them intact. Cheap,
    /// and it preserves anything this build did not understand, because the
    /// untouched objects are literally the original ones.
    Incremental,
    /// Write the whole file afresh. Larger to produce and smaller to keep:
    /// the history goes away, and so does anything only the history held.
    #[default]
    Full,
}

impl PdfSaveMode {
    /// The `FPDF_SAVE_FLAGS` word.
    /// `c_ulong`, not `u64`: PDFium takes `unsigned long`, which is 64 bits
    /// on the native targets and 32 on `wasm32-unknown-emscripten`. Naming
    /// the C type is the only spelling that is right on both.
    fn flags(self) -> std::os::raw::c_ulong {
        match self {
            // 1 << 0 and 1 << 1 in `fpdf_save.h`. They are not a bitfield to
            // combine: passing both is a contradiction PDFium resolves by
            // preferring one, silently.
            Self::Incremental => 1 << 0,
            Self::Full => 1 << 1,
        }
    }
}

/// Why a layer artifact could not be written.
///
/// `EPDFLayerSaveStatus`. Separate from [`PdfiumError`] because the first one
/// is a fact about how large the document has grown, which a caller can act
/// on, rather than a runtime failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerSaveError {
    /// The append-only offsets outgrew the artifact format's range. The
    /// document is fine; the delta has to be flattened into a new base.
    AppendOnlyOffsetTooLarge,
    /// PDFium declined to write.
    SaveFailed,
    /// A status this build does not know, kept rather than collapsed.
    Unknown(u32),
}

impl LayerSaveError {
    fn from_status(status: u32) -> Self {
        match status {
            1 => Self::AppendOnlyOffsetTooLarge,
            2 => Self::SaveFailed,
            other => Self::Unknown(other),
        }
    }
}

impl std::fmt::Display for LayerSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppendOnlyOffsetTooLarge => {
                write!(f, "the layer artifact's append-only offsets exceed the supported range")
            }
            Self::SaveFailed => write!(f, "writing the layer artifact failed"),
            Self::Unknown(status) => write!(f, "the layer save failed with status {status}"),
        }
    }
}

/// What a layer document can serialize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerOutput {
    /// Base plus delta, as one artifact. What server persistence stores.
    Artifact,
    /// The delta alone. The caller materializes the layer by concatenating
    /// its own base bytes with this.
    Delta,
}

impl Document {
    /// Serialize as a standalone PDF.
    ///
    /// For a layer document this is not the artifact: `Incremental` copies
    /// the base bytes through and appends the delta as an ordinary PDF
    /// revision, so the result is a file any reader opens.
    pub fn save_to_buffer(&self, mode: PdfSaveMode) -> Result<Vec<u8>, PdfiumError> {
        let mut size: std::os::raw::c_ulong = 0;
        // SAFETY: the handle is live and `size` is a valid out-parameter.
        let ptr = unsafe {
            pdfium_sys::EPDF_SaveDocumentToOwnedBuffer(self.as_raw(), mode.flags(), &raw mut size)
        };
        owned_buffer(ptr, size).ok_or(PdfiumError::Save)
    }

    /// `EPDF_SaveDocumentToOwnedBufferEx`: as [`Document::save_to_buffer`],
    /// but `Ok(None)` when the save found nothing changed since load (the
    /// loaded bytes are then the document) and wrote nothing.
    pub fn save_to_buffer_ex(&self, mode: PdfSaveMode) -> Result<Option<Vec<u8>>, PdfiumError> {
        let mut size: std::os::raw::c_ulong = 0;
        let mut status: pdfium_sys::EPDFSaveStatus =
            pdfium_sys::EPDFSaveStatus_EPDFSaveStatus_kFailed;
        // SAFETY: the handle is live and both out-parameters are valid.
        let ptr = unsafe {
            pdfium_sys::EPDF_SaveDocumentToOwnedBufferEx(
                self.as_raw(),
                mode.flags() as pdfium_sys::FPDF_DWORD,
                0,
                &raw mut size,
                &raw mut status,
            )
        };
        if status == pdfium_sys::EPDFSaveStatus_EPDFSaveStatus_kUnchangedSinceLoad {
            if !ptr.is_null() {
                // SAFETY: allocated by the save call, freed once.
                unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
            }
            return Ok(None);
        }
        let bytes = owned_buffer(ptr, size).ok_or(PdfiumError::Save)?;
        if status != pdfium_sys::EPDFSaveStatus_EPDFSaveStatus_kWritten {
            return Err(PdfiumError::Save);
        }
        Ok(Some(bytes))
    }

    /// `EPDFLayer_SaveDeltaToOwnedBufferEx`: a layer's cumulative delta and
    /// whether anything changed since load. `(None, false)` when nothing
    /// did; `(None, true)` when the layer changed back to its base and the
    /// delta is empty.
    pub fn save_layer_delta_ex(&self) -> Result<(Option<Vec<u8>>, bool), LayerSaveError> {
        let mut size: std::os::raw::c_ulong = 0;
        let mut status = u32::MAX;
        let mut changed: pdfium_sys::FPDF_BOOL = 0;
        // SAFETY: the handle is live and every out-parameter is valid.
        let ptr = unsafe {
            pdfium_sys::EPDFLayer_SaveDeltaToOwnedBufferEx(
                self.as_raw(),
                &raw mut size,
                &raw mut status,
                &raw mut changed,
            )
        };
        if status != pdfium_sys::EPDFLayerSaveStatus_EPDFLayerSaveStatus_kSuccess {
            if !ptr.is_null() {
                // SAFETY: allocated by the save call, freed once.
                unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
            }
            return Err(LayerSaveError::SaveFailed);
        }
        if size == 0 {
            if !ptr.is_null() {
                // SAFETY: as above.
                unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
            }
            return Ok((None, changed != 0));
        }
        Ok((owned_buffer(ptr, size), changed != 0))
    }

    /// Serialize a layer document's artifact or its delta.
    ///
    /// The caller is responsible for this being a layer. PDFium answers for a
    /// base document too, with a status rather than a crash, but the answer
    /// is not a thing anyone wants.
    pub fn save_layer_to_buffer(&self, output: LayerOutput) -> Result<Vec<u8>, LayerSaveError> {
        let mut size: std::os::raw::c_ulong = 0;
        // `u32::MAX` is a status the enum never issues, so an out-parameter
        // PDFium did not touch is distinguishable from every real status.
        let mut status = u32::MAX;
        // SAFETY: the handle is live and both out-parameters are valid.
        let ptr = unsafe {
            match output {
                LayerOutput::Artifact => pdfium_sys::EPDFLayer_SaveLayerArtifactToOwnedBuffer(
                    self.as_raw(),
                    &raw mut size,
                    &raw mut status,
                ),
                LayerOutput::Delta => pdfium_sys::EPDFLayer_SaveDeltaToOwnedBuffer(
                    self.as_raw(),
                    &raw mut size,
                    &raw mut status,
                ),
            }
        };

        // The status is checked BEFORE the pointer. A save that reports a
        // failure and hands back a buffer anyway has produced something, and
        // whatever it is, it is not the artifact that was asked for; writing
        // it into storage would be worse than writing nothing.
        //
        // Unexercised, and kept. Every failure seen so far also answers null,
        // so a version that checked the pointer first passes every test:
        // `a_layer_session_writes_an_artifact_that_opens_again` provokes the
        // one failure available, a delta of a layer with no edits, and that
        // one returns null too. The guard is for a partial write, which is
        // the case nobody has managed to produce and the one that would
        // corrupt a document silently.
        if status != 0 {
            drop(owned_buffer(ptr, size));
            return Err(LayerSaveError::from_status(status));
        }
        owned_buffer(ptr, size).ok_or(LayerSaveError::SaveFailed)
    }
}

/// Copy a PDFium-allocated buffer out and free it.
///
/// The copy is unavoidable: the bytes are in PDFium's allocator and handing a
/// `Vec` over them would free them with Rust's.
pub(crate) fn owned_buffer(
    ptr: *mut std::ffi::c_void,
    size: std::os::raw::c_ulong,
) -> Option<Vec<u8>> {
    if ptr.is_null() {
        return None;
    }
    let len = size as usize;
    let out = if len == 0 {
        // A zero-byte save is not a document, so this is a failure rather
        // than an empty success. The buffer is still freed.
        //
        // Also unexercised: no save in the corpus produces zero bytes with a
        // non-null buffer. It is here because the alternative is handing a
        // caller an empty `Vec` that it will write to a file and later fail
        // to open, with nothing in between to say why.
        None
    } else {
        // Reserved fallibly rather than with `vec![0; len]`: `len` comes
        // from PDFium, and an allocation this layer cannot make is reported
        // rather than aborting the process.
        let mut bytes: Vec<u8> = Vec::new();
        match bytes.try_reserve_exact(len) {
            Ok(()) => {
                // SAFETY: PDFium reported `len` readable bytes at `ptr`.
                bytes.extend_from_slice(unsafe {
                    std::slice::from_raw_parts(ptr.cast::<u8>(), len)
                });
                Some(bytes)
            }
            Err(_) => None,
        }
    };
    // SAFETY: allocated by the save call and freed exactly once, here.
    unsafe { pdfium_sys::EPDF_FreeBuffer(ptr) };
    out
}
