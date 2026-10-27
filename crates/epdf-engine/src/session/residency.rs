//! The parsed pages of every document on one host, kept within a budget of
//! bytes (`services/src/document-session/pages/PageResidency.ts`).
//!
//! Parsing a page is the costly part of using it: seconds for a page of
//! millions of vector objects, where one tile of it renders in milliseconds.
//! So a page stays parsed after the job that used it, for the next one, as
//! long as memory allows:
//!
//! - A page costs what its parse made (`EPDFPage_GetParsedSize`) and a fixed
//!   overhead for the page itself ([`PAGE_OVERHEAD_BYTES`]).
//! - Room is made before a page loads, for its size when it was last open,
//!   and again once it has loaded. A page a job holds never closes, so one
//!   page may be over the budget on its own: the one a job needs.
//! - A page bigger than the whole budget stays parsed after its job too, or
//!   every job on it would parse it again: of such pages, the one that would
//!   close last is kept, and the budget holds every other page beside it. It
//!   closes when the pages jobs hold need the budget's room themselves.
//! - What the viewers show decides what closes first
//!   ([`PageResidency::set_working_set`]): pages in no working set, then
//!   `near` pages, then `visible` pages from the fewest device pixels on
//!   screen up. Least recently used first among equals.
//! - A page whose content changed since it was parsed closes when it is next
//!   taken (`EPDFPage_IsContentCurrent`). That sees writes made in layer
//!   transactions; the host closes a document's pages around the writes that
//!   change content in place.
//! - A page's image cache is emptied when its last job releases it, so its
//!   next job renders images as a newly loaded page does.
//! - Pages no job uses and no viewer shows close after a while
//!   ([`PageResidencyPolicy::idle_ms`]).
//!
//! # Where this differs from the TypeScript, and why
//!
//! This host answers one call at a time and has no event loop between them,
//! so two things upstream does are not here. A render does not load its page
//! in slices with an abort between them (`acquireInSlices`): nothing could
//! deliver the abort. And the idle timer does not fire on its own: the host
//! asks it at the start of each call ([`PageResidency::tick`]), which is
//! where a timer that fired between calls would have done its work.
//!
//! A page here is a counted handle, as the pool's always were
//! ([`super::pages`]): closing one a holder still has drops the residency's
//! claim on it, and the page itself closes with its last holder.

use std::collections::BTreeMap;
use std::rc::Rc;

use pdfium::SharedPage;

use super::pages::Placed;
use crate::error::EngineError;
use crate::identity::PageObjectNumber;

/// What a kept page tells the residency about itself: the native calls
/// upstream's residency makes on a page pointer.
pub trait ResidentPage {
    /// `EPDFPage_GetParsedSize`'s estimate: the bytes the parse made.
    fn parsed_bytes(&self) -> u64;
    /// `EPDFPage_IsContentCurrent`.
    fn is_content_current(&self) -> bool;
    /// `EPDFPage_ResetRenderCache`: `false` when the cache could not be
    /// emptied (a render of the page is still open).
    fn reset_render_cache(&self) -> bool;
}

impl ResidentPage for SharedPage {
    fn parsed_bytes(&self) -> u64 {
        self.view().parsed_size().map_or(0, |size| size.estimated_bytes)
    }

    fn is_content_current(&self) -> bool {
        self.view().is_content_current()
    }

    fn reset_render_cache(&self) -> bool {
        self.view().reset_render_cache()
    }
}

/// How much a residency keeps, and for how long (`PageResidencyPolicy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageResidencyPolicy {
    /// Bytes of parsed pages kept at once; a page a job holds, or one page
    /// bigger than the budget, may be over it alone. 0 keeps nothing.
    pub budget_bytes: u64,
    /// Pages no job uses close after this many milliseconds; 0 disables
    /// the timer.
    pub idle_ms: u64,
}

/// `DEFAULT_PAGE_RESIDENCY_POLICY`: 384 MB, what a server thread of the
/// hosted plan can spare and well inside the heap a phone browser allows a
/// tab, and 30 s idle.
pub const DEFAULT_PAGE_RESIDENCY_POLICY: PageResidencyPolicy =
    PageResidencyPolicy { budget_bytes: 384 * 1024 * 1024, idle_ms: 30_000 };

/// A residency that keeps nothing: a page closes when its last holder
/// releases it (`PagePtrPool` without a residency).
pub const KEEP_NOTHING: PageResidencyPolicy = PageResidencyPolicy { budget_bytes: 0, idle_ms: 0 };

/// `PAGE_OVERHEAD_BYTES`: what a page costs besides its objects, the page
/// itself and its caches.
pub const PAGE_OVERHEAD_BYTES: u64 = 64 * 1024;

/// Who a kept page belongs to: one per open document's pool.
pub type Owner = u64;

/// How many pages the residency loaded, closed and reset, for tests and
/// diagnostics: the native calls upstream's host tests count.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResidencyCounts {
    pub loads: u64,
    pub closes: u64,
    pub resets: u64,
}

struct Kept<P> {
    owner: Owner,
    page: Rc<P>,
    /// Jobs holding the page; it never closes while held.
    refs: u32,
    /// What it costs, as last measured.
    bytes: u64,
    /// When it was last taken or released, for least-recently-used order.
    last_used: u64,
}

/// The parsed pages of every document on one host (`PageResidency`).
pub struct PageResidency<P = SharedPage> {
    policy: PageResidencyPolicy,
    pages: BTreeMap<(Owner, PageObjectNumber), Kept<P>>,
    /// What each closed page cost when it was last open: the room its next
    /// load needs.
    closed_sizes: BTreeMap<(Owner, PageObjectNumber), u64>,
    /// Per owner, per view: where the pages it shows are.
    working_sets: BTreeMap<Owner, BTreeMap<String, BTreeMap<PageObjectNumber, Placed>>>,
    total_bytes: u64,
    clock: u64,
    /// Milliseconds now, for the idle timer.
    now: Box<dyn Fn() -> u64>,
    /// When the idle timer was armed, in [`PageResidency::now`]'s
    /// milliseconds.
    armed_at: Option<u64>,
    next_owner: Owner,
    counts: ResidencyCounts,
}

impl<P> std::fmt::Debug for PageResidency<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PageResidency")
            .field("policy", &self.policy)
            .field("size", &self.pages.len())
            .field("bytes", &self.total_bytes)
            .field("counts", &self.counts)
            .finish()
    }
}

impl<P: ResidentPage> Default for PageResidency<P> {
    fn default() -> Self {
        Self::new(DEFAULT_PAGE_RESIDENCY_POLICY)
    }
}

impl<P: ResidentPage> PageResidency<P> {
    /// A residency on the system's monotonic clock.
    pub fn new(policy: PageResidencyPolicy) -> Self {
        let start = std::time::Instant::now();
        Self::with_clock(policy, Box::new(move || start.elapsed().as_millis() as u64))
    }

    /// A residency whose idle timer reads `now`, in milliseconds.
    pub fn with_clock(policy: PageResidencyPolicy, now: Box<dyn Fn() -> u64>) -> Self {
        Self {
            policy,
            pages: BTreeMap::new(),
            closed_sizes: BTreeMap::new(),
            working_sets: BTreeMap::new(),
            total_bytes: 0,
            clock: 0,
            now,
            armed_at: None,
            next_owner: 0,
            counts: ResidencyCounts::default(),
        }
    }

    pub fn policy(&self) -> PageResidencyPolicy {
        self.policy
    }

    /// Change how much is kept from here on. Pages kept over a smaller
    /// budget close as room is next made.
    pub fn set_policy(&mut self, policy: PageResidencyPolicy) {
        self.policy = policy;
        self.make_room(0);
    }

    /// A new owner: one per open document's pool.
    pub fn register_owner(&mut self) -> Owner {
        self.next_owner += 1;
        self.next_owner
    }

    /// The page, loaded with `load` when it isn't kept; held until
    /// [`PageResidency::release`] (`acquire`).
    pub fn acquire(
        &mut self,
        owner: Owner,
        page_object_number: PageObjectNumber,
        load: impl FnOnce() -> Result<P, EngineError>,
    ) -> Result<Rc<P>, EngineError> {
        let key = (owner, page_object_number);
        if !self.take(key) {
            self.make_room(self.expected_bytes(key));
            let page = load()?;
            self.counts.loads += 1;
            self.clock += 1;
            self.pages.insert(
                key,
                Kept {
                    owner,
                    page: Rc::new(page),
                    refs: 0,
                    bytes: PAGE_OVERHEAD_BYTES,
                    last_used: self.clock,
                },
            );
            self.total_bytes += PAGE_OVERHEAD_BYTES;
        }
        let page = {
            let kept = self.pages.get_mut(&key).expect("taken or added above");
            kept.refs += 1;
            Rc::clone(&kept.page)
        };
        self.measure(key);
        self.make_room(0);
        Ok(page)
    }

    /// A job is done with the page; it stays parsed while there is room
    /// (`release`).
    pub fn release(&mut self, owner: Owner, page_object_number: PageObjectNumber) {
        let key = (owner, page_object_number);
        let Some(kept) = self.pages.get_mut(&key) else { return };
        if kept.refs == 0 {
            return;
        }
        kept.refs -= 1;
        if kept.refs > 0 {
            return;
        }
        self.clock += 1;
        kept.last_used = self.clock;
        // A page whose image cache can't be emptied (a render of it is still
        // open) can't be kept.
        self.counts.resets += 1;
        if !kept.page.reset_render_cache() {
            self.close(key);
            return;
        }
        self.arm_timer();
        self.make_room(0);
    }

    /// What `view` shows of `owner`'s pages, replacing its last set; an
    /// empty set withdraws it. A page listed twice keeps its better place.
    /// Bookkeeping only: it touches no PDFium state.
    pub fn set_working_set(
        &mut self,
        owner: Owner,
        view: &str,
        pages: impl IntoIterator<Item = (PageObjectNumber, Placed)>,
    ) {
        let mut shown: BTreeMap<PageObjectNumber, Placed> = BTreeMap::new();
        for (page, placed) in pages {
            match shown.get(&page) {
                Some(listed) if !placed.before(listed) => {}
                _ => {
                    shown.insert(page, placed);
                }
            }
        }
        if shown.is_empty() {
            if let Some(views) = self.working_sets.get_mut(&owner) {
                views.remove(view);
                if views.is_empty() {
                    self.working_sets.remove(&owner);
                }
            }
        } else {
            self.working_sets.entry(owner).or_default().insert(view.to_owned(), shown);
        }
        // Pages that left the set may now be idle and unshown: the timer
        // closes them.
        if self.total_bytes > 0 {
            self.arm_timer();
        }
    }

    /// True while a job holds the page; an idle page is not held.
    pub fn is_held(&self, owner: Owner, page_object_number: PageObjectNumber) -> bool {
        self.pages.get(&(owner, page_object_number)).is_some_and(|kept| kept.refs > 0)
    }

    /// True while the page is kept: a job for it parses nothing new.
    pub fn is_kept(&self, owner: Owner, page_object_number: PageObjectNumber) -> bool {
        self.pages.contains_key(&(owner, page_object_number))
    }

    /// How many of `owner`'s pages a job holds.
    pub fn held_count(&self, owner: Owner) -> usize {
        self.pages.values().filter(|kept| kept.owner == owner && kept.refs > 0).count()
    }

    /// Where the working sets have the page: the best place any of the
    /// owner's views gives it (`placeOf`).
    pub fn place_of(&self, owner: Owner, page_object_number: PageObjectNumber) -> Placed {
        let mut best = Placed::ELSEWHERE;
        for shown in self.working_sets.get(&owner).into_iter().flat_map(BTreeMap::values) {
            if let Some(placed) = shown.get(&page_object_number)
                && placed.before(&best)
            {
                best = *placed;
            }
        }
        best
    }

    /// Closes every page of `owner`, held or not: its document closes
    /// (`closeOwner`). A holder keeps its page until it lets go.
    pub fn close_owner(&mut self, owner: Owner) {
        let keys: Vec<_> = self.pages.keys().filter(|(o, _)| *o == owner).copied().collect();
        for key in keys {
            self.close(key);
        }
        self.closed_sizes.retain(|(o, _), _| *o != owner);
        self.working_sets.remove(&owner);
    }

    /// Closes the pages of `owner` no job holds: a write changed its content
    /// in place (`closeIdleOf`).
    pub fn close_idle_of(&mut self, owner: Owner) {
        let keys: Vec<_> = self
            .pages
            .iter()
            .filter(|(_, kept)| kept.owner == owner && kept.refs == 0)
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            self.close(key);
        }
    }

    /// Closes every page no job holds (`closeIdle`).
    pub fn close_idle(&mut self) {
        let keys: Vec<_> =
            self.pages.iter().filter(|(_, kept)| kept.refs == 0).map(|(key, _)| *key).collect();
        for key in keys {
            self.close(key);
        }
    }

    /// The idle timer, asked between calls: once the pages have been idle
    /// for [`PageResidencyPolicy::idle_ms`], the ones no job holds and no
    /// viewer shows on screen close (`closeUnshown`).
    pub fn tick(&mut self) {
        let Some(armed_at) = self.armed_at else { return };
        if (self.now)().saturating_sub(armed_at) < self.policy.idle_ms {
            return;
        }
        self.armed_at = None;
        let keys: Vec<_> = self
            .pages
            .iter()
            .filter(|(key, kept)| {
                kept.refs == 0 && self.place_of(key.0, key.1).place != super::pages::Place::Visible
            })
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            self.close(key);
        }
    }

    /// Pages kept, held or not.
    pub fn size(&self) -> usize {
        self.pages.len()
    }

    /// What the pages kept cost, as last measured.
    pub fn bytes(&self) -> u64 {
        self.total_bytes
    }

    /// How many pages it loaded, closed and reset so far.
    pub fn counts(&self) -> ResidencyCounts {
        self.counts
    }

    /// The page is kept, unless its content changed meanwhile, which closes
    /// it (`take`). Answers whether it is kept now.
    fn take(&mut self, key: (Owner, PageObjectNumber)) -> bool {
        let Some(kept) = self.pages.get(&key) else { return false };
        if kept.refs == 0 && !kept.page.is_content_current() {
            self.close(key);
            return false;
        }
        self.clock += 1;
        self.pages.get_mut(&key).expect("checked above").last_used = self.clock;
        true
    }

    fn measure(&mut self, key: (Owner, PageObjectNumber)) {
        let Some(kept) = self.pages.get_mut(&key) else { return };
        let bytes = PAGE_OVERHEAD_BYTES + kept.page.parsed_bytes();
        self.total_bytes = self.total_bytes + bytes - kept.bytes;
        kept.bytes = bytes;
    }

    fn expected_bytes(&self, key: (Owner, PageObjectNumber)) -> u64 {
        self.closed_sizes.get(&key).copied().unwrap_or(PAGE_OVERHEAD_BYTES)
    }

    /// Closes pages no job holds until `incoming` more bytes fit: the worst
    /// placed first, least recently used first among equals (`makeRoom`).
    ///
    /// A page bigger than the budget on its own may stay over it: of such
    /// pages, the one that would close last. Every other page fits in the
    /// budget beside it; when the pages jobs hold don't, they are the pages
    /// over it, and the spared page closes too. A budget of 0 keeps nothing.
    fn make_room(&mut self, incoming: u64) {
        let mut spared =
            if self.policy.budget_bytes > 0 { self.last_to_close_over_budget() } else { None };
        loop {
            let spared_bytes = spared.and_then(|key| self.pages.get(&key)).map_or(0, |k| k.bytes);
            if self.total_bytes - spared_bytes + incoming <= self.policy.budget_bytes {
                return;
            }
            if let Some(victim) = self.first_to_close(spared) {
                self.close(victim);
            } else if let Some(key) = spared
                && self.pages.get(&key).is_some_and(|kept| kept.refs == 0)
            {
                self.close(key);
                spared = None;
            } else {
                return;
            }
        }
    }

    /// The page no job holds that closes first, other than `spared`.
    fn first_to_close(
        &self,
        spared: Option<(Owner, PageObjectNumber)>,
    ) -> Option<(Owner, PageObjectNumber)> {
        let mut victim: Option<((Owner, PageObjectNumber), Placed, u64)> = None;
        for (key, kept) in &self.pages {
            if kept.refs > 0 || Some(*key) == spared {
                continue;
            }
            let placed = self.place_of(key.0, key.1);
            let worse = match &victim {
                None => true,
                Some((_, victim_place, victim_used)) => {
                    victim_place.before(&placed)
                        || (!placed.before(victim_place) && kept.last_used < *victim_used)
                }
            };
            if worse {
                victim = Some((*key, placed, kept.last_used));
            }
        }
        victim.map(|(key, _, _)| key)
    }

    /// Of the pages bigger than the budget, the one that closes last: the
    /// best placed, the most recently used among equals.
    fn last_to_close_over_budget(&self) -> Option<(Owner, PageObjectNumber)> {
        let mut kept_best: Option<((Owner, PageObjectNumber), Placed, u64)> = None;
        for (key, kept) in &self.pages {
            if kept.bytes <= self.policy.budget_bytes {
                continue;
            }
            let placed = self.place_of(key.0, key.1);
            let better = match &kept_best {
                None => true,
                Some((_, best_place, best_used)) => {
                    placed.before(best_place)
                        || (!best_place.before(&placed) && kept.last_used > *best_used)
                }
            };
            if better {
                kept_best = Some((*key, placed, kept.last_used));
            }
        }
        kept_best.map(|(key, _, _)| key)
    }

    /// Closes the page, remembering what it cost for its next load.
    fn close(&mut self, key: (Owner, PageObjectNumber)) {
        let Some(kept) = self.pages.remove(&key) else { return };
        self.closed_sizes.insert(key, kept.bytes);
        self.total_bytes -= kept.bytes;
        self.counts.closes += 1;
        if self.total_bytes == 0 {
            self.armed_at = None;
        }
        // The residency's claim goes here; the page closes with its last
        // holder.
        drop(kept);
    }

    fn arm_timer(&mut self) {
        if self.policy.idle_ms == 0 {
            return;
        }
        self.armed_at = Some((self.now)());
    }
}
