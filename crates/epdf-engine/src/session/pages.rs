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
//!
//! # Working sets
//!
//! What the views show (`pages.workingSet`) decides which kept pages close
//! first: pages in no working set, then `near` pages, then `visible` pages
//! from the fewest device pixels up (`PageResidency.setWorkingSet`). The pool
//! records the sets and ranks pages by them. It keeps no page past its last
//! holder, as upstream's pool keeps nothing without a residency (`budgetBytes:
//! 0`), so the order has no page to apply to until a residency is ported.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use pdfium::SharedPage;
use serde::Deserialize;

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
    /// Per view, where the pages it shows are (`PageResidency.workingSets`).
    working_sets: RefCell<BTreeMap<String, BTreeMap<PageObjectNumber, Placed>>>,
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

    /// What `view` shows of the document, replacing its last set; an empty
    /// set withdraws it (`PageResidency.setWorkingSet`). A page listed twice
    /// keeps its better place. Bookkeeping only: it touches no PDFium state.
    pub fn set_working_set(&self, view: &str, pages: &[WorkingSetPage]) {
        let mut shown: BTreeMap<PageObjectNumber, Placed> = BTreeMap::new();
        for page in pages {
            let placed = Placed { place: page.role.into(), pixels: page.pixels };
            match shown.get(&page.page) {
                Some(listed) if !placed.before(listed) => {}
                _ => {
                    shown.insert(page.page, placed);
                }
            }
        }
        let mut sets = self.working_sets.borrow_mut();
        if shown.is_empty() {
            sets.remove(view);
        } else {
            sets.insert(view.to_owned(), shown);
        }
    }

    /// Where the working sets have the page: the best place any view gives
    /// it (`PageResidency.placeOf`).
    pub fn place_of(&self, page_object_number: PageObjectNumber) -> Placed {
        let mut best = Placed::ELSEWHERE;
        for shown in self.working_sets.borrow().values() {
            if let Some(placed) = shown.get(&page_object_number)
                && placed.before(&best)
            {
                best = *placed;
            }
        }
        best
    }

    /// `pages` in the order they close (`PageResidency.firstToClose`): the
    /// worst placed first, and among equals in the order given, which a
    /// caller passes least recently used first.
    pub fn close_order(&self, pages: &[PageObjectNumber]) -> Vec<PageObjectNumber> {
        let mut ranked: Vec<(PageObjectNumber, Placed)> =
            pages.iter().map(|&page| (page, self.place_of(page))).collect();
        // Stable, so equals keep the caller's order.
        ranked.sort_by(|(_, a), (_, b)| {
            if b.before(a) {
                std::cmp::Ordering::Less
            } else if a.before(b) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        ranked.into_iter().map(|(page, _)| page).collect()
    }

    /// Drop the pool's knowledge of every page.
    ///
    /// Does not close a page somebody is still holding; see the module
    /// documentation. Pages with no holder are already closed, so this is
    /// forgetting tombstones rather than freeing memory.
    pub fn close_all(&self) {
        self.entries.borrow_mut().clear();
        self.working_sets.borrow_mut().clear();
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

    fn pon(n: u32) -> PageObjectNumber {
        PageObjectNumber::new(n).expect("a number")
    }

    fn shown(page: u32, role: WorkingSetRole, pixels: f64) -> WorkingSetPage {
        WorkingSetPage { page: pon(page), role, pixels }
    }

    #[test]
    fn pages_no_view_shows_close_first_then_near_then_visible_from_the_fewest_pixels() {
        // `page-residency.test.ts`: "closes pages no view shows first, then
        // near ones, then visible ones from the fewest pixels up". Equals
        // close in the order given, least recently used first.
        use WorkingSetRole::{Near, Visible};
        let pool = PagePtrPool::new();
        pool.set_working_set(
            "stage",
            &[
                shown(1, Visible, 1_000_000.0),
                shown(2, Visible, 30_000.0),
                shown(7, Visible, 500_000.0),
                shown(3, Near, 0.0),
            ],
        );
        let order = pool.close_order(&[pon(1), pon(2), pon(4), pon(3), pon(7), pon(5)]);
        assert_eq!(order, [4, 5, 3, 2, 7, 1].map(pon));
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
        let pool = PagePtrPool::new();
        let number = first_page(&document);

        let held = pool.acquire(&document, number).expect("held");
        pool.close_all();

        assert!(!pool.is_held(number), "the pool forgot the page");
        // And the holder can still read it, which is the whole point.
        assert!(held.view().width() > 0.0);
    }
}
