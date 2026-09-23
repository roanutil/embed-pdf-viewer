//! Page handle lifetime for one open session.
//!
//! G3. Replaces `services/src/document-session/pages/{PagePtrPool,PageRecord}.ts`.
//!
//! While at least one holder has a page, the handle is shared rather than
//! reloaded. Once the last holder drops it the page is closed; a later
//! [`PagePtrPool::acquire`] reloads it. There is no retention across the
//! zero-holder boundary, which matches the TypeScript and leaves room for an
//! LRU later without changing any caller.
//!
//! # Where this differs from the TypeScript, and why
//!
//! `PagePtrPool` counts references by hand and `closeAll` calls
//! `FPDF_ClosePage` on every pooled pointer whether or not somebody is holding
//! it. In JavaScript that leaves the holder with a number that no longer
//! addresses a page, and the next call through it is undefined behaviour
//! inside PDFium.
//!
//! Here the pool holds [`Weak`] references and hands out [`Rc`]s, so the count
//! is the language's and a close cannot invalidate a live holder: the page
//! closes when the last holder drops, which may be after
//! [`PagePtrPool::close_all`] returns. G3's gate asks that close "waits for
//! outstanding leases", and this is that, enforced by the borrow checker
//! rather than by a convention every caller has to keep.
//!
//! The cost is that a caller who parks a page handle keeps a PDFium page alive
//! past session close, where the TypeScript would have freed the memory and
//! left them a dangling pointer. That is the better failure: a leak is
//! visible, findable and bounded by the holder's own lifetime.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use pdfium::SharedPage;

use crate::access::OwnedDocument;
use crate::error::EngineError;
use crate::identity::PageObjectNumber;

/// Record of one page within a session. Metadata only.
///
/// Carries no handle: lifetime belongs to [`PagePtrPool`]. Built lazily, and
/// fully populated only by a path that needs index-to-object-number for the
/// whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageRecord {
    pub page_object_number: PageObjectNumber,
    pub page_index: u32,
}

/// Shared page handles for one open document.
///
/// Carries no lifetime: the pages it hands out hold the document open
/// themselves through [`pdfium::SharedPage`], so a session can own the pool and
/// the document side by side. A pool that borrowed its document could not be
/// stored next to it.
#[derive(Debug, Default)]
pub struct PagePtrPool {
    /// `Weak` so the pool never keeps a page alive on its own. An entry whose
    /// page has been dropped is a tombstone that [`PagePtrPool::acquire`]
    /// replaces and [`PagePtrPool::is_held`] reports as not held.
    entries: RefCell<BTreeMap<PageObjectNumber, Weak<SharedPage>>>,
}

impl PagePtrPool {
    pub fn new() -> Self {
        Self::default()
    }

    /// The page, loaded once and shared until the last holder drops it.
    ///
    /// Loads through the normalized loader, because
    /// [`crate::access::OwnedDocument`] does; see
    /// `tests/rotation_normalization.rs` for why that matters.
    pub fn acquire(
        &self,
        document: &OwnedDocument,
        page_object_number: PageObjectNumber,
    ) -> Result<Rc<SharedPage>, EngineError> {
        if let Some(live) = self.entries.borrow().get(&page_object_number).and_then(Weak::upgrade) {
            return Ok(live);
        }
        // Loaded outside the borrow: `shared_page` calls into PDFium and a
        // panic there with the map borrowed would poison every later acquire.
        let page = Rc::new(document.shared_page(page_object_number)?);
        self.entries.borrow_mut().insert(page_object_number, Rc::downgrade(&page));
        Ok(page)
    }

    /// True while at least one holder has the page open.
    ///
    /// Page-structure mutations assert on this. Thread confinement means no
    /// other job can be mid-flight, so a held page during a structural
    /// mutation is a leaked acquire, and the mutator fails loudly rather than
    /// mutating under a live handle.
    pub fn is_held(&self, page_object_number: PageObjectNumber) -> bool {
        self.entries.borrow().get(&page_object_number).is_some_and(|weak| weak.strong_count() > 0)
    }

    /// How many pages currently have a holder. For the balanced-acquire gate.
    pub fn held_count(&self) -> usize {
        self.entries.borrow().values().filter(|weak| weak.strong_count() > 0).count()
    }

    /// Drop the pool's knowledge of every page.
    ///
    /// Does not close a page somebody is still holding; see the module
    /// documentation. Pages with no holder are already closed, so this is
    /// forgetting tombstones rather than freeing memory.
    pub fn close_all(&self) {
        self.entries.borrow_mut().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdfium::{Document, Library};

    fn document() -> OwnedDocument {
        OwnedDocument::new(
            Document::from_bytes(
                Library::acquire(),
                &test_support::fixture(test_support::REPORT),
                None,
            )
            .expect("open"),
        )
        .expect("index")
    }

    fn first_page(document: &OwnedDocument) -> PageObjectNumber {
        PageObjectNumber::new(document.document().page(0).expect("page").object_number())
            .expect("number")
    }

    #[test]
    fn overlapping_acquires_share_one_load() {
        let document = document();
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let second = pool.acquire(&document, number).expect("second");
        assert!(Rc::ptr_eq(&first, &second), "the second acquire reloaded the page");
        assert_eq!(Rc::strong_count(&first), 2);
    }

    #[test]
    fn a_page_closes_when_its_last_holder_drops() {
        let document = document();
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let second = pool.acquire(&document, number).expect("second");
        assert!(pool.is_held(number));

        drop(first);
        assert!(pool.is_held(number), "one holder left");
        drop(second);
        assert!(!pool.is_held(number), "no holders left, so the page is closed");
        assert_eq!(pool.held_count(), 0);
    }

    #[test]
    fn a_reacquire_after_zero_holders_reloads() {
        // No retention across the zero-holder boundary, which is what the
        // TypeScript documents and what leaves room for an LRU later.
        let document = document();
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let address = Rc::as_ptr(&first);
        drop(first);

        let again = pool.acquire(&document, number).expect("again");
        assert_ne!(Rc::as_ptr(&again), address, "the tombstone was reused");
        assert!(pool.is_held(number));
    }

    #[test]
    fn different_pages_are_counted_separately() {
        let document = document();
        let pool = PagePtrPool::new();
        let first = first_page(&document);
        let second =
            PageObjectNumber::new(document.document().page(1).expect("page").object_number())
                .expect("number");
        assert_ne!(first, second);

        let a = pool.acquire(&document, first).expect("a");
        let b = pool.acquire(&document, second).expect("b");
        assert_eq!(pool.held_count(), 2);
        drop(a);
        assert_eq!(pool.held_count(), 1);
        assert!(pool.is_held(second));
        drop(b);
        assert_eq!(pool.held_count(), 0);
    }

    #[test]
    fn a_page_the_document_does_not_have_is_not_found() {
        let document = document();
        let pool = PagePtrPool::new();
        let missing = PageObjectNumber::new(99_999).expect("number");
        assert_eq!(
            pool.acquire(&document, missing).expect_err("no such page").code,
            crate::error::EngineErrorCode::NotFound,
        );
        assert!(!pool.is_held(missing), "a failed acquire leaves no entry");
    }

    #[test]
    fn close_all_does_not_invalidate_a_live_holder() {
        // The divergence from the TypeScript, asserted rather than described.
        // `closeAll` there frees the pointer a holder is still using; here the
        // page outlives the pool's knowledge of it.
        let document = document();
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let held = pool.acquire(&document, number).expect("held");
        pool.close_all();

        assert!(!pool.is_held(number), "the pool forgot the page");
        // And the holder can still read it, which is the whole point.
        assert!(held.view().width() > 0.0);
    }
}
