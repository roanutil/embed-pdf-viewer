//! Shareable base documents and the layer views opened on them.
//!
//! A base document is loaded once and frozen, then any number of layer
//! documents are opened on top of it. The layer is an ordinary
//! `FPDF_DOCUMENT`; the base is not, and none of the usual document calls
//! accept one. That is the point: the bytes of a large PDF are parsed once and
//! shared, while each session gets its own writable overlay.
//!
//! Lifetime is the thing to get right. The header says the memory buffer
//! "must remain valid until the returned base document is released", and a
//! layer document points into its base, so a base released while a layer is
//! open leaves that layer reading freed memory. Both invariants are held here
//! by ownership rather than by a rule: [`BaseDocument`] owns the bytes it was
//! loaded from, and [`LayerDocument`] holds an `Rc` to its base.

use std::ffi::CString;
use std::rc::Rc;

use crate::{Document, Library, PdfiumError};

/// Why a layer would not open.
///
/// `EPDFLayerOpenStatus` from `public/fpdfview.h`. Separate variants rather
/// than one failure because they send a caller somewhere different: a password
/// prompt can resolve the first and nothing can resolve the third.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerOpenError {
    /// The delta is encrypted and no password was given.
    PasswordRequired,
    /// The delta bytes are not a delta.
    MalformedDelta,
    /// The artifact was produced against a different base document.
    BaseMismatch,
    /// Anything else, including a null handle with a success status.
    OpenFailed,
}

impl LayerOpenError {
    fn from_status(status: u32) -> Self {
        match status {
            1 => Self::PasswordRequired,
            2 => Self::MalformedDelta,
            3 => Self::BaseMismatch,
            _ => Self::OpenFailed,
        }
    }
}

impl std::fmt::Display for LayerOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PasswordRequired => write!(f, "the layer delta requires a password"),
            Self::MalformedDelta => write!(f, "the layer delta is malformed"),
            Self::BaseMismatch => {
                write!(f, "the layer artifact does not match the supplied base document")
            }
            Self::OpenFailed => write!(f, "failed to open the layer document"),
        }
    }
}

impl std::error::Error for LayerOpenError {}

/// A parsed, frozen document that layers are opened on top of.
///
/// Not an `FPDF_DOCUMENT` and not usable with document calls.
#[derive(Debug)]
pub struct BaseDocument {
    handle: pdfium_sys::EPDF_BASE_DOCUMENT,
    /// The buffer PDFium keeps a pointer into. Must outlive `handle`, so it is
    /// declared after it and dropped after it.
    #[allow(dead_code)]
    bytes: Vec<u8>,
    #[allow(dead_code)]
    library: Rc<Library>,
    _not_send: std::marker::PhantomData<*const ()>,
}

impl BaseDocument {
    /// Load and freeze a base document from bytes.
    ///
    /// The copy is deliberate and reserved fallibly, as [`Document::from_bytes`]
    /// does: PDFium holds a pointer into this buffer until the base is
    /// released, so the base has to own it.
    pub fn from_bytes(
        library: Rc<Library>,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<Self, PdfiumError> {
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| PdfiumError::OutOfMemory)?;
        owned.extend_from_slice(bytes);

        let c_password = password
            .map(|p| {
                CString::new(p).map_err(|_| PdfiumError::InvalidArgument("password contains NUL"))
            })
            .transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        // SAFETY: the pointer and length describe `owned`, which this type
        // keeps alive until the handle is released in `Drop`.
        let handle = unsafe {
            pdfium_sys::EPDF_LoadMemBaseDocument64(
                owned.as_ptr() as *const std::ffi::c_void,
                owned.len(),
                password_ptr,
            )
        };
        if handle.is_null() {
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == pdfium_sys::FPDF_ERR_PASSWORD as _ {
                PdfiumError::Password
            } else {
                PdfiumError::Load
            });
        }
        Ok(Self { handle, bytes: owned, library, _not_send: std::marker::PhantomData })
    }

    /// Open a fresh, empty layer on this base.
    ///
    /// `self: &Rc<Self>` because the returned layer holds the base open. A
    /// layer whose base was released would read freed memory, and taking the
    /// `Rc` here makes that unrepresentable.
    pub fn open_layer(self: &Rc<Self>) -> Result<LayerDocument, LayerOpenError> {
        // Initialized to a value the enum does not define, so a PDFium build
        // that forgets to write the out-parameter is reported as a failure
        // rather than read as kSuccess.
        let mut status: pdfium_sys::EPDFLayerOpenStatus = u32::MAX;
        // SAFETY: the base handle is live for the duration of the call and,
        // through the `Rc` below, for the life of the returned layer. A null
        // file access opens a fresh layer, per the header.
        let handle = unsafe {
            pdfium_sys::EPDFLayer_OpenLayer(
                self.handle,
                std::ptr::null_mut(),
                std::ptr::null(),
                &mut status,
            )
        };
        if handle.is_null() || status != 0 {
            if !handle.is_null() {
                unsafe { pdfium_sys::FPDF_CloseDocument(handle) };
            }
            return Err(LayerOpenError::from_status(status));
        }
        // SAFETY: a non-null handle from EPDFLayer_OpenLayer is an ordinary
        // FPDF_DOCUMENT that must be closed with FPDF_CloseDocument, which is
        // what `Document::from_raw_layer` arranges.
        let document = unsafe { Document::from_raw_layer(handle, Rc::clone(&self.library)) };
        Ok(LayerDocument { document, base: Rc::clone(self) })
    }

    /// The number of objects currently in a layer's overlay.
    ///
    /// On the layer, not the base; here because it is the only way to observe
    /// that a fresh layer really is empty.
    pub fn promoted_object_count(layer: &Document) -> u64 {
        // SAFETY: `layer` is a live document handle owned by the caller.
        unsafe { pdfium_sys::EPDFLayer_GetPromotedObjectCount(layer.as_raw()) as u64 }
    }
}

impl Drop for BaseDocument {
    fn drop(&mut self) {
        // SAFETY: loaded above and released exactly once, here, before the
        // buffer it points into is freed.
        unsafe { pdfium_sys::EPDF_ReleaseBaseDocument(self.handle) };
    }
}

/// A writable layer over a shared base.
///
/// Derefs to an ordinary [`Document`], because that is what it is.
#[derive(Debug)]
pub struct LayerDocument {
    document: Document,
    /// Holds the base open. Declared after `document` so the layer closes
    /// first, which is the order the base's lifetime requires.
    #[allow(dead_code)]
    base: Rc<BaseDocument>,
}

impl LayerDocument {
    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn base(&self) -> &Rc<BaseDocument> {
        &self.base
    }

    /// Split the layer into its document and the base that backs it.
    ///
    /// For a caller that has to store the two separately, which a session
    /// does: the document goes where every other document goes and the base
    /// is parked with the session's retained resources.
    ///
    /// The `Rc` is not optional. A caller that drops it while still holding
    /// the `Document` leaves that document pointing into a released base, so
    /// the contract is that the `Rc` outlives the `Document`. Returning them
    /// as a tuple makes the safe thing the easy thing: tuple fields drop
    /// left to right, so `let (document, base) = ...` already drops the
    /// document first.
    pub fn into_parts(self) -> (Document, Rc<BaseDocument>) {
        (self.document, self.base)
    }
}

impl std::ops::Deref for LayerDocument {
    type Target = Document;

    fn deref(&self) -> &Self::Target {
        &self.document
    }
}
