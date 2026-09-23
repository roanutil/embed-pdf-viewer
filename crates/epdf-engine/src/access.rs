//! `DocumentAccess` and `PageAccess`: what every reader is written against.
//!
//! This is the generalization of the `FormModelSource { Borrowed | Owned }`
//! pattern in `multiplatform-port-hazards.md` 3.4, hoisted so it covers every
//! read rather than just the form model.
//!
//! The ownership migration runs a reader in two modes
//! (`multiplatform-plan-claude-remix.md` Section 6):
//!
//! - Stage A, on web. TypeScript owns the session. It lends a validated page
//!   pointer for the duration of one call through
//!   `document-session/pages/PagePtrPool.acquire` and releases it in a
//!   `finally`. Rust retains nothing, closes nothing, and must not call
//!   `FPDF_LoadPage` itself: doing so would bypass the pool's normalized
//!   rotation, its shared refcounts and its structural-mutation leak checks.
//!   [`BorrowedPage`] is that mode, and it cannot close a page because
//!   `pdfium::PageView` has no `Drop`.
//! - Stage B and native. Rust owns the session and the page.
//!   [`OwnedPage`] is that mode.
//!
//! Without this trait the read groups get written once against a raw pointer
//! and again at G10 against a session, and that second pass is in nobody's
//! estimate. The cost, stated in the port order: the trait is designed before
//! the Stage B session exists, so expect to widen it once.

use std::collections::BTreeMap;

use pdfium::{Document, PageView};

use crate::error::{EngineError, EngineErrorCode};
use crate::identity::PageObjectNumber;

/// One page, however it is owned.
pub trait PageAccess {
    /// The page, for the duration of this borrow only.
    fn view(&self) -> &PageView<'_>;

    /// The page's durable identity.
    ///
    /// Not cached: a `0` here means the file is malformed, and reporting that
    /// at the point of use keeps the check next to the read that depends on
    /// it.
    fn object_number(&self) -> Result<PageObjectNumber, EngineError> {
        PageObjectNumber::new(self.view().object_number())
    }
}

/// A page lent by an owner outside Rust, for one call.
#[derive(Debug)]
pub struct BorrowedPage<'a> {
    view: PageView<'a>,
}

impl<'a> BorrowedPage<'a> {
    /// Wraps a page pointer the caller owns.
    ///
    /// # Safety
    /// `handle` must be a live `FPDF_PAGE` acquired by the owner, on this
    /// thread, and it must stay open for all of `'a`. The caller releases it;
    /// this type never does.
    pub unsafe fn from_raw(handle: RawPageHandle) -> Result<Self, EngineError> {
        // A plain pointer cast. The safety contract above is what makes it
        // sound; the type is `c_void` so host adapters never name a
        // `pdfium-sys` type to call this.
        let view = unsafe { PageView::from_raw(handle.cast()) }.ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "the borrowed page pointer was null",
            )
        })?;
        Ok(Self { view })
    }
}

/// An `FPDF_PAGE` as it reaches a host adapter: an opaque address, decoded
/// from the worker protocol or handed over by `napi-rs`.
///
/// Deliberately `c_void` rather than the `pdfium-sys` type, so nothing above
/// this crate has to depend on the bindings.
pub type RawPageHandle = *mut std::ffi::c_void;

impl PageAccess for BorrowedPage<'_> {
    fn view(&self) -> &PageView<'_> {
        &self.view
    }
}

/// A page this process loaded and closes.
#[derive(Debug)]
pub struct OwnedPage<'a> {
    page: pdfium::Page<'a>,
}

impl<'a> OwnedPage<'a> {
    pub fn new(page: pdfium::Page<'a>) -> Self {
        Self { page }
    }
}

impl PageAccess for OwnedPage<'_> {
    fn view(&self) -> &PageView<'_> {
        &self.page
    }
}

/// One document, however it is owned, addressed by durable page identity.
pub trait DocumentAccess {
    type Page<'a>: PageAccess
    where
        Self: 'a;

    fn page(&self, page: PageObjectNumber) -> Result<Self::Page<'_>, EngineError>;
}

/// A document this process opened, with an object-number index built eagerly.
///
/// The eager walk is a placeholder for the session's page registry (G3), which
/// keeps `recordsByIndex` and `recordsByObjectNumber` alongside a
/// `fullyEnumerated` flag so a lazy per-page lookup and a full enumeration
/// agree on identity (`DocumentSession.ts:41-44`). That agreement is a G6 gate
/// and this type does not attempt it: it enumerates once, up front, and says
/// so.
#[derive(Debug)]
pub struct OwnedDocument {
    document: Document,
    index_by_object_number: BTreeMap<PageObjectNumber, u32>,
}

impl OwnedDocument {
    /// Opens the index by walking every page once.
    ///
    /// A page whose object number is `0` is a direct object, which makes the
    /// file malformed for identity purposes. It is skipped rather than
    /// rejected, so a document with one broken page still serves its others,
    /// and asking for that page reports `NotFound`.
    pub fn new(document: Document) -> Result<Self, EngineError> {
        let mut index_by_object_number = BTreeMap::new();
        for index in 0..document.page_count() {
            let page = document.page(index).map_err(|e| {
                EngineError::with_message(EngineErrorCode::DocOpenFailed, e.to_string())
            })?;
            if let Ok(number) = PageObjectNumber::new(page.object_number()) {
                index_by_object_number.insert(number, index);
            }
        }
        Ok(Self { document, index_by_object_number })
    }

    pub fn page_count(&self) -> u32 {
        self.document.page_count()
    }

    pub fn document(&self) -> &Document {
        &self.document
    }
}

impl DocumentAccess for OwnedDocument {
    type Page<'a>
        = OwnedPage<'a>
    where
        Self: 'a;

    fn page(&self, page: PageObjectNumber) -> Result<Self::Page<'_>, EngineError> {
        // The index lookup is the NotFound check, so a page this document does
        // not have is reported before PDFium is asked.
        if !self.index_by_object_number.contains_key(&page) {
            return Err(EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no page with object number {page}"),
            ));
        }
        // By object number and normalized, never by index. `FPDF_LoadPage`
        // reports a `/Rotate 90` page as landscape and places its text to
        // match, which puts every coordinate this engine produces in a
        // different space from the one `PagePtrPool.acquire` produces on web.
        // `tooling/engine-conformance/fixtures/rotated_pages.pdf` is four
        // identical pages at 0, 90, 180 and 270 degrees, and
        // `tests/rotation_normalization.rs` fails if this reverts.
        let loaded = self.document.page_by_object_number(page.get()).map_err(|e| {
            EngineError::with_message(EngineErrorCode::RuntimeUnavailable, e.to_string())
        })?;
        Ok(OwnedPage::new(loaded))
    }
}
