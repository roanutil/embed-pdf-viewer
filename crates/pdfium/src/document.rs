use std::ffi::{CString, c_void};
use std::marker::PhantomData;
use std::rc::Rc;

use crate::{FileAccess, Library, Page, PdfiumError};

/// An open `FPDF_DOCUMENT` and the bytes PDFium is reading from.
#[derive(Debug)]
pub struct Document {
    pub(crate) handle: pdfium_sys::FPDF_DOCUMENT,
    /// `FPDF_LoadMemDocument64` does NOT copy: PDFium reads from this buffer
    /// for the document's whole life, so the Document owns it. Field order
    /// matters, because fields drop after the `Drop` impl body runs, which is
    /// what puts `FPDF_CloseDocument` ahead of freeing these bytes. Never read
    /// directly; it exists to keep the allocation PDFium points into alive.
    #[allow(dead_code)]
    bytes: Vec<u8>,
    /// The handle is only valid while the library that created it stays
    /// initialized. Holding a clone of the caller's `Rc<Library>` means
    /// `FPDF_DestroyLibrary` runs only once the last Document has gone, not
    /// when whichever guard happens to drop first. Like `bytes`, this runs no
    /// code itself; it exists to be dropped after the close call below.
    #[allow(dead_code)]
    library: Rc<Library>,
    /// The base a layer reads through, for a layer opened over one. Dropped
    /// after the close in `Drop`, like `bytes`, so every holder of the
    /// document (a pooled page's `Rc<Document>` included) keeps the base
    /// alive for as long as PDFium can read through it.
    #[allow(dead_code)]
    base: Option<Rc<crate::BaseDocument>>,
    _not_send: PhantomData<*const ()>,
}

impl Document {
    /// Opens a document from bytes, copying them into an allocation the
    /// Document owns.
    ///
    /// One copy is deliberate. Moving the Document moves the `Vec`'s three
    /// words, never the heap allocation, so the pointer PDFium holds stays
    /// valid; a borrowed `&[u8]` would put a lifetime on `Document` and make
    /// it unusable from an owner thread's document map.
    ///
    /// The copy is reserved fallibly: a document larger than the process can
    /// hold is an error, not an abort. That is the admission check the
    /// lifecycle contract asks for on input bytes.
    pub fn from_bytes(
        lib: Rc<Library>,
        bytes: &[u8],
        password: Option<&str>,
    ) -> Result<Self, PdfiumError> {
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len()).map_err(|_| PdfiumError::OutOfMemory)?;
        owned.extend_from_slice(bytes);

        // A NUL in the password would truncate silently, so reject it rather
        // than opening the document with a shorter password than was asked
        // for and reporting the failure as a wrong password.
        let c_password = password
            .map(|p| {
                CString::new(p).map_err(|_| PdfiumError::InvalidArgument("password contains NUL"))
            })
            .transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        // SAFETY: the pointer and length describe `owned`, which this
        // Document keeps alive for as long as the handle exists, and the
        // password pointer is either null or a live NUL-terminated CString
        // that outlives the call.
        let handle = unsafe {
            pdfium_sys::FPDF_LoadMemDocument64(
                owned.as_ptr() as *const c_void,
                owned.len(),
                password_ptr,
            )
        };

        if handle.is_null() {
            // FPDF_GetLastError returns c_ulong, which is 64-bit on macOS and
            // Linux but 32-bit on Windows; FPDF_ERR_PASSWORD is a generated
            // u32 constant, so `as _` is required to compile on every target.
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == pdfium_sys::FPDF_ERR_PASSWORD as _ {
                PdfiumError::Password
            } else if err == pdfium_sys::FPDF_ERR_SECURITY as _ {
                // Collapsing this into Load would tell a caller the bytes are
                // not a PDF, when they are a PDF nobody here can decrypt.
                PdfiumError::Security
            } else {
                PdfiumError::Load
            });
        }

        Ok(Self { handle, bytes: owned, library: lib, base: None, _not_send: PhantomData })
    }

    /// Open a document read through a [`FileAccess`] rather than from a copy.
    ///
    /// The access has to outlive the document: PDFium keeps the pointer and
    /// reads through it for the life of the handle, so this borrows rather
    /// than taking ownership and the caller keeps the `FileAccess` alive.
    pub fn from_file_access<'a>(
        lib: Rc<Library>,
        access: &'a FileAccess,
        password: Option<&str>,
    ) -> Result<FileBackedDocument<'a>, PdfiumError> {
        let c_password = password
            .map(|p| {
                CString::new(p).map_err(|_| PdfiumError::InvalidArgument("password contains NUL"))
            })
            .transpose()?;
        let password_ptr = c_password.as_ref().map_or(std::ptr::null(), |c| c.as_ptr());

        // SAFETY: `access` is borrowed for `'a`, which bounds the returned
        // document, so the pointer stays valid for every read PDFium makes.
        let handle = unsafe { pdfium_sys::FPDF_LoadCustomDocument(access.as_ptr(), password_ptr) };
        if handle.is_null() {
            let err = unsafe { pdfium_sys::FPDF_GetLastError() };
            return Err(if err == pdfium_sys::FPDF_ERR_PASSWORD as _ {
                PdfiumError::Password
            } else if err == pdfium_sys::FPDF_ERR_SECURITY as _ {
                PdfiumError::Security
            } else {
                PdfiumError::Load
            });
        }
        Ok(FileBackedDocument {
            // SAFETY: non-null, closed exactly once when the wrapper drops,
            // and the access it reads through outlives it by construction.
            document: unsafe { Self::from_raw_layer(handle, lib) },
            _access: PhantomData,
        })
    }

    /// Wraps a document handle produced by something other than a load.
    ///
    /// # Safety
    /// `handle` must be a non-null `FPDF_DOCUMENT` that this `Document` may
    /// close exactly once with `FPDF_CloseDocument`, and whatever it points
    /// into must outlive it. A stamp drawing the fork exports or makes
    /// canonical is such a handle: it points into nothing (`crate::stamp`).
    /// A layer over a [`crate::BaseDocument`] is opened with
    /// [`Document::from_raw_layer_over`], which keeps its base.
    pub(crate) unsafe fn from_raw_layer(
        handle: pdfium_sys::FPDF_DOCUMENT,
        library: Rc<Library>,
    ) -> Self {
        // No bytes of its own: a layer's storage belongs to its base.
        Self { handle, bytes: Vec::new(), library, base: None, _not_send: PhantomData }
    }

    /// [`Document::from_raw_layer`] for a layer `EPDFLayer_OpenLayer` opened
    /// over `base`, which the document holds until after its handle closes.
    ///
    /// # Safety
    /// As [`Document::from_raw_layer`], with `base` the document the handle
    /// points into.
    pub(crate) unsafe fn from_raw_layer_over(
        handle: pdfium_sys::FPDF_DOCUMENT,
        library: Rc<Library>,
        base: Rc<crate::BaseDocument>,
    ) -> Self {
        Self { handle, bytes: Vec::new(), library, base: Some(base), _not_send: PhantomData }
    }

    pub fn page_count(&self) -> u32 {
        // Negative means PDFium could not read the page tree; report zero
        // pages rather than a huge unsigned count.
        unsafe { pdfium_sys::FPDF_GetPageCount(self.handle) }.max(0) as u32
    }

    /// The permission word in force for this session.
    ///
    /// `FPDF_GetDocPermissions`, whose header says: "If the document is not
    /// protected **or was unlocked by the owner**, 0xffffffff will be
    /// returned." So this is the effective word, and an owner unlock elevates
    /// it. Pair with [`Document::user_permissions`], which never does.
    ///
    /// Raw. Bits 1-2 are reserved as zero by the standard security handler and
    /// PDFium masks even owner permissions through that convention, which is
    /// why "all permissions" is `0xFFFFFFFC` rather than `0xFFFFFFFF`.
    /// Interpreting that is `epdf_engine::features::security`.
    pub fn permissions(&self) -> u32 {
        unsafe { pdfium_sys::FPDF_GetDocPermissions(self.handle) as u32 }
    }

    /// Loads a page by PDF object number, with rotation normalized to zero.
    ///
    /// The loader read paths should use. See [`Page::load_normalized`]: a
    /// `/Rotate 90` page loaded by index reports landscape dimensions and
    /// places its text accordingly, which puts every coordinate the engine
    /// produces in a different space from the one the web engine uses.
    pub fn page_by_object_number(&self, object_number: u32) -> Result<Page<'_>, PdfiumError> {
        Page::load_normalized(self.handle, object_number)
    }

    /// Loads a page by zero-based index, owning it until the returned `Page`
    /// drops.
    ///
    /// Does NOT normalize rotation. Prefer
    /// [`Document::page_by_object_number`] for anything whose coordinates
    /// leave this crate; this one is for walking the page tree to discover
    /// what is there.
    pub fn page(&self, index: u32) -> Result<Page<'_>, PdfiumError> {
        if index >= self.page_count() {
            return Err(PdfiumError::PageOutOfRange(index));
        }
        Page::load(self.handle, index)
    }

    /// The permission word the document declares for a user.
    ///
    /// `FPDF_GetDocUserPermissions`, whose header says: "**Always returns user
    /// permissions, even if the document was unlocked by the owner.**"
    ///
    /// The two calls are easy to get the wrong way round, and the shipping
    /// TypeScript does: `SecurityReader.readLiveSecurityInfo` calls this one
    /// and describes it as "the permission word in force right now (e.g. after
    /// an owner unlock)", which is the opposite of what it returns. The
    /// consequence is that an owner-unlocked session reports itself as
    /// restricted. The generated fixtures show it directly:
    /// `encrypted-r2-rc4-40.pdf` opened with the owner password reports
    /// `permissions() == 0xFFFFFFFC` and `user_permissions() == 0xFFFFF7E8`.
    pub fn user_permissions(&self) -> u32 {
        unsafe { pdfium_sys::FPDF_GetDocUserPermissions(self.handle) as u32 }
    }

    /// Whether the document carries an encryption dictionary at all.
    pub fn is_encrypted(&self) -> bool {
        unsafe { pdfium_sys::EPDF_IsEncrypted(self.handle) != 0 }
    }

    /// Whether this session unlocked with the owner password.
    pub fn is_owner_unlocked(&self) -> bool {
        unsafe { pdfium_sys::EPDF_IsOwnerUnlocked(self.handle) != 0 }
    }

    /// The standard security handler revision, 2 through 6.
    ///
    /// PDFium returns -1 for a document with no encryption dictionary, which
    /// callers drop rather than report; see
    /// `epdf_engine::features::security::state::build_security_info`.
    pub fn security_handler_revision(&self) -> i32 {
        unsafe { pdfium_sys::FPDF_GetSecurityHandlerRevision(self.handle) }
    }

    /// Make PDFium's internals behave as though the owner password was given.
    ///
    /// Proves nothing and authorizes nobody. It exists because high-level
    /// PDFium subsystems, forms especially, consult the permission bits
    /// themselves and would otherwise refuse work the application has already
    /// authorized. The header is explicit: "Callers must enforce JWT/PDF
    /// permission policy outside PDFium."
    ///
    /// So this is only safe to call in an engine that enforces permissions
    /// itself. `epdf_engine::session::DocumentSession` does, through
    /// `epdf_engine::auth::scope`, and turning this on without that would
    /// hand every caller an owner-unlocked document.
    pub fn set_runtime_owner_permissions(&self, enabled: bool) -> bool {
        unsafe { pdfium_sys::EPDF_SetRuntimeOwnerPermissions(self.handle, i32::from(enabled)) != 0 }
    }

    /// Check a password against an already-open document and report what it
    /// would unlock.
    ///
    /// Does not change the session. This is the "would this password help?"
    /// question a prompt asks, answered without reopening the document, which
    /// is why it exists alongside the password argument to
    /// [`Document::from_bytes`].
    ///
    /// `None` when the password is rejected.
    pub fn check_password_permissions(&self, password: &str) -> Option<PasswordPermissions> {
        let Ok(c_password) = CString::new(password) else {
            // A NUL would truncate silently, which would report a wrong
            // password as correct for its prefix.
            return None;
        };
        let mut kind: i32 = 0;
        let mut user: u32 = 0;
        let mut effective: u32 = 0;
        let mut revision: i32 = 0;
        // SAFETY: the handle is live, the password outlives the call, and the
        // four out-parameters are distinct initialized locals.
        let ok = unsafe {
            pdfium_sys::EPDF_CheckPasswordPermissions(
                self.handle,
                c_password.as_ptr(),
                &mut kind,
                &mut user,
                &mut effective,
                &mut revision,
            )
        };
        (ok != 0).then_some(PasswordPermissions {
            kind,
            user_permissions: user,
            effective_permissions: effective,
            security_handler_revision: revision,
        })
    }

    /// The library keeping PDFium initialized on this thread.
    ///
    /// Cloning the `Rc` rather than borrowing it, because the point is to
    /// open a SECOND document that outlives the borrow of the first.
    pub(crate) fn library(&self) -> Rc<Library> {
        self.library.clone()
    }

    /// The raw handle, for the operations this layer has not wrapped yet.
    ///
    /// # Safety
    /// The caller must not close the handle or outlive this `Document`.
    pub unsafe fn as_raw(&self) -> pdfium_sys::FPDF_DOCUMENT {
        self.handle
    }
}

/// A document PDFium reads through a borrowed [`FileAccess`].
///
/// Separate from [`Document`] because the lifetime is load-bearing: the access
/// must outlive the handle, and a plain `Document` has nowhere to record that.
#[derive(Debug)]
pub struct FileBackedDocument<'a> {
    document: Document,
    _access: PhantomData<&'a FileAccess>,
}

impl std::ops::Deref for FileBackedDocument<'_> {
    type Target = Document;

    fn deref(&self) -> &Self::Target {
        &self.document
    }
}

/// What a password would unlock, without unlocking it.
///
/// `kind` is `EPDF_PASSWORD_PERMISSION_*` from `public/fpdfview.h`: invalid =
/// 0, none = 1, user = 2, owner = 3. Left as the raw code here because naming
/// it is the engine's job, in
/// `epdf_engine::features::security::state::OpenedAs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPermissions {
    pub kind: i32,
    pub user_permissions: u32,
    pub effective_permissions: u32,
    pub security_handler_revision: i32,
}

impl Drop for Document {
    fn drop(&mut self) {
        // Closes first; `self.bytes` and a layer's base are freed afterwards,
        // when the fields drop.
        // SAFETY: the handle was returned non-null by FPDF_LoadMemDocument64
        // and is closed exactly once, here.
        unsafe { pdfium_sys::FPDF_CloseDocument(self.handle) };
    }
}
