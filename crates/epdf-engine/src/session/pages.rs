//! The pages of one open session, kept by the host's page residency.
//!
//! G3. Replaces `services/src/document-session/pages/{PagePtrPool,PageRecord}.ts`.
//!
//! [`PagePtrPool::acquire`] answers the page with that object number, loaded
//! with the normalized loader when it isn't kept; overlapping acquires of one
//! page share it, and the [`PooledPage`] a caller holds hands it back to the
//! residency when it drops, which keeps it parsed while there is room
//! ([`super::residency::PageResidency`]). A pool made without a residency
//! gets one of its own that keeps nothing: its pages close when released.
//!
//! # Where this differs from the TypeScript, and why
//!
//! `PagePtrPool.closeAll` calls `FPDF_ClosePage` on every pooled pointer
//! whether or not somebody is holding it. In JavaScript that leaves the
//! holder with a number that no longer addresses a page, and the next call
//! through it is undefined behaviour inside PDFium.
//!
//! Here a page is a counted handle, so a close cannot invalidate a live
//! holder: the page closes when the last holder drops, which may be after
//! [`PagePtrPool::close_all`] returns. G3's gate asks that close "waits for
//! outstanding leases", and this is that, enforced by the borrow checker
//! rather than by a convention every caller has to keep.
//!
//! The cost is that a caller who parks a page handle keeps a PDFium page
//! alive past session close, where the TypeScript would have freed the memory
//! and left them a dangling pointer. That is the better failure: a leak is
//! visible, findable and bounded by the holder's own lifetime.
//!
//! # Working sets
//!
//! What the views show (`pages.workingSet`) decides which kept pages close
//! first: pages in no working set, then `near` pages, then `visible` pages
//! from the fewest device pixels up (`PageResidency.setWorkingSet`).

use std::cell::RefCell;
use std::rc::Rc;

use pdfium::SharedPage;
use serde::Deserialize;

use super::residency::{KEEP_NOTHING, Owner, PageResidency};
use crate::access::OwnedDocument;
use crate::error::EngineError;
use crate::identity::PageObjectNumber;

/// The residency a host's sessions share.
pub type SharedResidency = Rc<RefCell<PageResidency<SharedPage>>>;

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

/// The pages of one open document (`PagePtrPool`), kept by a residency.
///
/// Carries no lifetime: the pages it hands out hold the document open
/// themselves through [`pdfium::SharedPage`], so a session can own the pool and
/// the document side by side.
#[derive(Debug)]
pub struct PagePtrPool {
    residency: SharedResidency,
    owner: Owner,
}

impl Default for PagePtrPool {
    fn default() -> Self {
        Self::new()
    }
}

/// A page a job holds (`acquire` until `release`): it reads as the page, and
/// hands it back to the residency when it drops.
pub struct PooledPage {
    page: Rc<SharedPage>,
    residency: SharedResidency,
    owner: Owner,
    page_object_number: PageObjectNumber,
}

impl std::fmt::Debug for PooledPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledPage").field("page", &self.page_object_number).finish()
    }
}

impl PooledPage {
    /// The page's shared handle, for a caller that keeps it past this hold.
    pub fn shared(&self) -> &Rc<SharedPage> {
        &self.page
    }
}

impl std::ops::Deref for PooledPage {
    type Target = SharedPage;

    fn deref(&self) -> &SharedPage {
        &self.page
    }
}

impl crate::access::PageAccess for PooledPage {
    fn view(&self) -> &pdfium::PageView<'_> {
        self.page.view()
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        // The page this hold has stays alive until this struct's fields
        // drop, after the residency may have let go of its own claim.
        match self.residency.try_borrow_mut() {
            Ok(mut residency) => residency.release(self.owner, self.page_object_number),
            // Only a page dropped inside a residency call could meet it
            // borrowed, and no residency call holds a page.
            Err(_) => debug_assert!(false, "a page was released while the residency was in use"),
        }
    }
}

/// Where a page is in a view (`scheduling/place.ts`): on screen, placed
/// just off it, or in no view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Place {
    Elsewhere,
    Near,
    Visible,
}

/// A page's place and its device pixels on screen there (`Placed`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub place: Place,
    pub pixels: f64,
}

impl Placed {
    /// A page no view shows: it closes first.
    pub const ELSEWHERE: Placed = Placed { place: Place::Elsewhere, pixels: 0.0 };

    /// `placedBefore`: whether `self` is the better place, on screen before
    /// near before elsewhere, then more pixels.
    pub fn before(&self, other: &Placed) -> bool {
        if self.place != other.place {
            return self.place > other.place;
        }
        self.pixels > other.pixels
    }
}

/// One page of a working set as the wire sends it (`WorkingSetPage`):
/// `visible` (the part on screen) is read by the scheduler upstream, not by
/// the pool, and is ignored here.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingSetPage {
    #[serde(with = "crate::identity::page_ref")]
    pub page: PageObjectNumber,
    pub role: WorkingSetRole,
    pub pixels: f64,
}

/// `WorkingSetPage.role`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkingSetRole {
    Visible,
    Near,
}

impl From<WorkingSetRole> for Place {
    fn from(role: WorkingSetRole) -> Self {
        match role {
            WorkingSetRole::Visible => Self::Visible,
            WorkingSetRole::Near => Self::Near,
        }
    }
}

impl PagePtrPool {
    /// A pool with a residency of its own that keeps nothing: its pages
    /// close when released.
    pub fn new() -> Self {
        Self::with_residency(Rc::new(RefCell::new(PageResidency::new(KEEP_NOTHING))))
    }

    /// A pool whose pages `residency` keeps, beside every other pool's.
    pub fn with_residency(residency: SharedResidency) -> Self {
        let owner = residency.borrow_mut().register_owner();
        Self { residency, owner }
    }

    /// The page, loaded when it isn't kept, and held until the answer drops.
    ///
    /// Loads through the normalized loader, because
    /// [`crate::access::OwnedDocument`] does; see
    /// `tests/rotation_normalization.rs` for why that matters.
    pub fn acquire(
        &self,
        document: &OwnedDocument,
        page_object_number: PageObjectNumber,
    ) -> Result<PooledPage, EngineError> {
        // The index lookup first: a page the document doesn't have is
        // `NotFound` whether or not a page of that number is kept.
        document.page_index(page_object_number)?;
        let page = self
            .residency
            .borrow_mut()
            .acquire(self.owner, page_object_number, || document.shared_page(page_object_number))?;
        Ok(PooledPage {
            page,
            residency: Rc::clone(&self.residency),
            owner: self.owner,
            page_object_number,
        })
    }

    /// True while at least one holder has the page open.
    ///
    /// Page-structure mutations assert on this. Thread confinement means no
    /// other job can be mid-flight, so a held page during a structural
    /// mutation is a leaked acquire, and the mutator fails loudly rather than
    /// mutating under a live handle.
    pub fn is_held(&self, page_object_number: PageObjectNumber) -> bool {
        self.residency.borrow().is_held(self.owner, page_object_number)
    }

    /// True while the residency keeps the page, held or not: a job for it
    /// parses nothing new.
    pub fn is_kept(&self, page_object_number: PageObjectNumber) -> bool {
        self.residency.borrow().is_kept(self.owner, page_object_number)
    }

    /// How many pages currently have a holder. For the balanced-acquire gate.
    pub fn held_count(&self) -> usize {
        self.residency.borrow().held_count(self.owner)
    }

    /// What `view` shows of the document, replacing its last set; an empty
    /// set withdraws it (`PageResidency.setWorkingSet`). A page listed twice
    /// keeps its better place. Bookkeeping only: it touches no PDFium state.
    pub fn set_working_set(&self, view: &str, pages: &[WorkingSetPage]) {
        self.residency.borrow_mut().set_working_set(
            self.owner,
            view,
            pages
                .iter()
                .map(|page| (page.page, Placed { place: page.role.into(), pixels: page.pixels })),
        );
    }

    /// Where the working sets have the page: the best place any view gives
    /// it (`PageResidency.placeOf`).
    pub fn place_of(&self, page_object_number: PageObjectNumber) -> Placed {
        self.residency.borrow().place_of(self.owner, page_object_number)
    }

    /// Closes the pages no job holds: a write changed the document's content
    /// in place (`closeIdle`).
    pub fn close_idle(&self) {
        self.residency.borrow_mut().close_idle_of(self.owner);
    }

    /// Close every page of the document (`closeAll`).
    ///
    /// Does not close a page somebody is still holding; see the module
    /// documentation.
    pub fn close_all(&self) {
        self.residency.borrow_mut().close_owner(self.owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::residency::PageResidencyPolicy;
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

    fn keeping() -> SharedResidency {
        Rc::new(RefCell::new(PageResidency::new(PageResidencyPolicy {
            budget_bytes: 64 * 1024 * 1024,
            idle_ms: 0,
        })))
    }

    #[test]
    fn overlapping_acquires_share_one_load() {
        let document = document();
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let second = pool.acquire(&document, number).expect("second");
        assert!(Rc::ptr_eq(first.shared(), second.shared()), "the second acquire reloaded");
        assert!(pool.is_held(number));
    }

    #[test]
    fn without_a_residency_a_page_closes_when_its_last_holder_drops() {
        // `a session without a residency keeps nothing`.
        let document = document();
        let residency: SharedResidency = Rc::new(RefCell::new(PageResidency::new(KEEP_NOTHING)));
        let pool = PagePtrPool::with_residency(Rc::clone(&residency));
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let second = pool.acquire(&document, number).expect("second");
        drop(first);
        assert!(pool.is_held(number), "one holder left");
        drop(second);
        assert!(!pool.is_held(number), "no holders left");
        assert!(!pool.is_kept(number), "nothing keeps it");
        assert_eq!(pool.held_count(), 0);

        drop(pool.acquire(&document, number).expect("again"));
        assert_eq!(residency.borrow().counts().loads, 2, "a released page was kept");
        assert_eq!(residency.borrow().counts().closes, 2);
    }

    #[test]
    fn a_residency_keeps_a_released_page_and_hands_it_back() {
        let document = document();
        let residency = keeping();
        let pool = PagePtrPool::with_residency(Rc::clone(&residency));
        let number = first_page(&document);

        let first = pool.acquire(&document, number).expect("first");
        let address = Rc::as_ptr(first.shared());
        drop(first);
        assert!(!pool.is_held(number));
        assert!(pool.is_kept(number), "the residency let it go");
        let again = pool.acquire(&document, number).expect("again");
        assert_eq!(Rc::as_ptr(again.shared()), address, "the kept page was loaded again");
        assert_eq!(residency.borrow().counts().loads, 1);
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

    fn pon(n: u32) -> PageObjectNumber {
        PageObjectNumber::new(n).expect("a number")
    }

    fn shown(page: u32, role: WorkingSetRole, pixels: f64) -> WorkingSetPage {
        WorkingSetPage { page: pon(page), role, pixels }
    }

    #[test]
    fn a_page_keeps_its_best_place_and_a_set_replaces_the_views_last_one() {
        // "a page keeps its best place; a set replaces the view's last one",
        // and an empty set withdraws the view.
        use WorkingSetRole::{Near, Visible};
        let pool = PagePtrPool::new();
        pool.set_working_set(
            "stage",
            &[shown(1, Visible, 1_000_000.0), shown(3, Visible, 200_000.0)],
        );
        pool.set_working_set("rail", &[shown(2, Visible, 30_000.0), shown(3, Near, 0.0)]);
        assert_eq!(pool.place_of(pon(3)).place, Place::Visible);
        assert_eq!(pool.place_of(pon(4)), Placed::ELSEWHERE);

        pool.set_working_set("stage", &[shown(1, Visible, 1_000_000.0)]);
        assert_eq!(pool.place_of(pon(3)).place, Place::Near);
        pool.set_working_set("rail", &[]);
        assert_eq!(pool.place_of(pon(2)), Placed::ELSEWHERE);
        assert_eq!(pool.place_of(pon(1)).place, Place::Visible);

        // Listed twice in one set, a page keeps the better of the two.
        pool.set_working_set("rail", &[shown(5, Near, 0.0), shown(5, Visible, 10.0)]);
        assert_eq!(pool.place_of(pon(5)), Placed { place: Place::Visible, pixels: 10.0 });
        pool.close_all();
        assert_eq!(pool.place_of(pon(1)), Placed::ELSEWHERE, "a closed pool forgets its sets");
    }

    #[test]
    fn close_all_does_not_invalidate_a_live_holder() {
        // The divergence from the TypeScript, asserted rather than described.
        // `closeAll` there frees the pointer a holder is still using; here the
        // page outlives the pool's knowledge of it.
        let document = document();
        let pool = PagePtrPool::with_residency(keeping());
        let number = first_page(&document);

        let held = pool.acquire(&document, number).expect("held");
        pool.close_all();

        assert!(!pool.is_held(number), "the pool forgot the page");
        // And the holder can still read it, which is the whole point.
        assert!(held.view().width() > 0.0);
    }
}
