//! The host's parsed pages, kept within a budget of bytes
//! (`packages/engine/main/test/page-residency.test.ts`), against a fake
//! page: each page parses into a known number of bytes and can be told its
//! content changed. What the viewers show (working sets) decides which kept
//! pages close first.
//!
//! Upstream's cases that load a page in slices (`acquireInSlices`: an
//! aborted load set aside, a half-loaded page finished at once, room made as
//! a page grows during its load) have no counterpart: this host answers one
//! call at a time, with nothing between slices to deliver an abort. Its idle
//! timer is asked between calls ([`PageResidency::tick`]) rather than firing
//! on its own, so the cases that advance a fake timer here advance a fake
//! clock and tick.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use epdf_engine::error::EngineError;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::pages::{Place, Placed};
use epdf_engine::session::residency::{
    Owner, PAGE_OVERHEAD_BYTES, PageResidency, PageResidencyPolicy, ResidentPage,
};

const MB: u64 = 1024 * 1024;

/// What the fake runtime was asked, in order.
#[derive(Debug, Default)]
struct Calls {
    events: Vec<String>,
    loads: Vec<u32>,
    closes: Vec<u32>,
    resets: Vec<u32>,
}

/// A page of the fake runtime: it parses into `bytes`, and closes when
/// dropped (`FPDF_ClosePage`).
struct FakePage {
    number: u32,
    bytes: u64,
    current: Rc<Cell<bool>>,
    calls: Rc<RefCell<Calls>>,
}

impl ResidentPage for FakePage {
    fn parsed_bytes(&self) -> u64 {
        self.bytes
    }

    fn is_content_current(&self) -> bool {
        self.current.get()
    }

    fn reset_render_cache(&self) -> bool {
        self.calls.borrow_mut().resets.push(self.number);
        true
    }
}

impl Drop for FakePage {
    fn drop(&mut self) {
        let mut calls = self.calls.borrow_mut();
        calls.closes.push(self.number);
        calls.events.push(format!("close {}", self.number));
    }
}

/// A runtime of pages, each of which parses into a known number of bytes.
struct Runtime {
    pages: BTreeMap<u32, u64>,
    calls: Rc<RefCell<Calls>>,
    open: RefCell<Vec<(u32, Rc<Cell<bool>>)>>,
}

impl Runtime {
    fn new(pages: &[(u32, u64)]) -> Self {
        Self {
            pages: pages.iter().copied().collect(),
            calls: Rc::default(),
            open: RefCell::default(),
        }
    }

    fn load(&self, number: u32) -> Result<FakePage, EngineError> {
        let mut calls = self.calls.borrow_mut();
        calls.loads.push(number);
        calls.events.push(format!("load {number}"));
        let bytes = self.pages[&number];
        let current = Rc::new(Cell::new(true));
        self.open.borrow_mut().push((number, Rc::clone(&current)));
        Ok(FakePage { number, bytes, current, calls: Rc::clone(&self.calls) })
    }

    /// Marks every open page of `number` as no longer matching its document.
    fn change_content(&self, number: u32) {
        for (page, current) in self.open.borrow().iter() {
            if *page == number {
                current.set(false);
            }
        }
    }

    fn loads(&self) -> Vec<u32> {
        self.calls.borrow().loads.clone()
    }

    fn closes(&self) -> Vec<u32> {
        self.calls.borrow().closes.clone()
    }
}

fn pon(number: u32) -> PageObjectNumber {
    PageObjectNumber::new(number).expect("a page object number")
}

fn residency(budget_bytes: u64) -> PageResidency<FakePage> {
    PageResidency::with_clock(PageResidencyPolicy { budget_bytes, idle_ms: 0 }, Box::new(|| 0))
}

/// Acquires and releases a page, as one job's reader does.
fn use_page(residency: &mut PageResidency<FakePage>, runtime: &Runtime, owner: Owner, number: u32) {
    acquire(residency, runtime, owner, number);
    residency.release(owner, pon(number));
}

fn acquire(
    residency: &mut PageResidency<FakePage>,
    runtime: &Runtime,
    owner: Owner,
    number: u32,
) -> Rc<FakePage> {
    residency.acquire(owner, pon(number), || runtime.load(number)).expect("a page")
}

/// A page a view shows, as the host hears it.
fn shown(page: u32, place: Place, pixels: f64) -> (PageObjectNumber, Placed) {
    (pon(page), Placed { place, pixels })
}

#[test]
fn keeps_a_released_page_and_hands_it_back_without_loading_it_again() {
    let runtime = Runtime::new(&[(1, MB)]);
    let mut residency = residency(10 * MB);
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    use_page(&mut residency, &runtime, owner, 1);

    assert_eq!(runtime.loads(), [1]);
    assert_eq!(runtime.closes(), Vec::<u32>::new());
    assert_eq!(runtime.calls.borrow().resets, [1, 1]);
    assert_eq!(residency.bytes(), MB + PAGE_OVERHEAD_BYTES);
}

#[test]
fn closes_the_least_recently_used_pages_to_stay_within_its_budget() {
    let runtime = Runtime::new(&[(1, 4 * MB), (2, 4 * MB), (3, 4 * MB)]);
    let mut residency = residency(9 * MB);
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    use_page(&mut residency, &runtime, owner, 2);
    use_page(&mut residency, &runtime, owner, 1); // 2 is now the least recently used.
    use_page(&mut residency, &runtime, owner, 3);

    assert_eq!(runtime.loads(), [1, 2, 3]);
    assert_eq!(runtime.closes(), [2]);
    assert_eq!(residency.size(), 2);
    assert!(residency.bytes() <= 9 * MB);
}

#[test]
fn never_closes_a_page_a_job_holds_so_one_page_may_be_over_the_budget_alone() {
    let runtime = Runtime::new(&[(1, 2 * MB), (2, 20 * MB)]);
    let mut residency = residency(10 * MB);
    let owner = residency.register_owner();

    let first = acquire(&mut residency, &runtime, owner, 1);
    let second = acquire(&mut residency, &runtime, owner, 2);
    assert_eq!(runtime.closes(), Vec::<u32>::new());
    assert!(residency.is_held(owner, pon(1)));

    drop((first, second));
    residency.release(owner, pon(1));
    residency.release(owner, pon(2));
    // The big page stays: it is the one room was made for. The small page
    // fits in the budget beside it.
    assert_eq!(runtime.closes(), Vec::<u32>::new());
    assert_eq!(residency.size(), 2);
}

#[test]
fn keeps_a_page_bigger_than_the_budget_between_jobs_with_the_pages_that_fit_beside_it() {
    let runtime = Runtime::new(&[(1, 20 * MB), (2, 2 * MB), (3, 2 * MB)]);
    let mut residency = residency(10 * MB);
    let owner = residency.register_owner();
    residency.set_working_set(owner, "stage", [shown(1, Place::Visible, 1_000_000.0)]);
    residency.set_working_set(
        owner,
        "rail",
        [shown(2, Place::Visible, 30_000.0), shown(3, Place::Visible, 30_000.0)],
    );

    // Every job on the big page uses one parse of it, though it is over the
    // budget alone; the rail's small pages load and stay beside it.
    for page in [1, 1, 2, 3, 1, 2, 3, 1] {
        use_page(&mut residency, &runtime, owner, page);
    }
    assert_eq!(runtime.loads(), [1, 2, 3]);
    assert_eq!(runtime.closes(), Vec::<u32>::new());
}

#[test]
fn closes_the_big_page_kept_over_the_budget_when_a_held_page_needs_the_room() {
    // Upstream grows page 2 past the budget over the slices of its load;
    // here it loads at once, and the room is made once it has.
    let runtime = Runtime::new(&[(1, 20 * MB), (2, 30 * MB)]);
    let mut residency = residency(10 * MB);
    let owner = residency.register_owner();
    residency.set_working_set(
        owner,
        "stage",
        [shown(1, Place::Visible, 1_000_000.0), shown(2, Place::Near, 0.0)],
    );

    use_page(&mut residency, &runtime, owner, 1);
    assert_eq!(runtime.closes(), Vec::<u32>::new());
    // Page 2 is over the budget while its job holds it: only one page may be
    // over the budget, and page 2 can't close, so page 1 does.
    let held = acquire(&mut residency, &runtime, owner, 2);
    assert_eq!(runtime.closes(), [1]);
    drop(held);
    residency.release(owner, pon(2));
    assert_eq!(residency.size(), 1);
    assert!(residency.is_kept(owner, pon(2)));
}

#[test]
fn makes_room_before_a_load_for_the_size_the_page_had_when_it_was_last_open() {
    let runtime = Runtime::new(&[(1, 6 * MB), (2, 3 * MB)]);
    let mut residency = residency(8 * MB);
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    residency.close_idle();
    use_page(&mut residency, &runtime, owner, 2);
    runtime.calls.borrow_mut().events.clear();

    // Page 1 needs 6 MB again, which doesn't fit beside page 2: page 2
    // closes before page 1 loads, not after.
    use_page(&mut residency, &runtime, owner, 1);
    assert_eq!(runtime.calls.borrow().events, ["close 2", "load 1"]);
}

#[test]
fn closes_a_page_whose_content_changed_when_it_is_next_taken_and_loads_it_again() {
    let runtime = Runtime::new(&[(1, MB)]);
    let mut residency = residency(100 * MB);
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    runtime.change_content(1);
    use_page(&mut residency, &runtime, owner, 1);

    assert_eq!(runtime.loads(), [1, 1]);
    assert_eq!(runtime.closes(), [1]);
}

#[test]
fn closes_the_idle_pages_of_one_document_or_every_page_of_a_closing_one() {
    let runtime = Runtime::new(&[(1, MB), (2, MB)]);
    let mut residency = residency(100 * MB);
    let first = residency.register_owner();
    let second = residency.register_owner();

    use_page(&mut residency, &runtime, first, 1);
    let held = acquire(&mut residency, &runtime, first, 2);
    use_page(&mut residency, &runtime, second, 1);
    assert_eq!(residency.size(), 3);

    residency.close_idle_of(first);
    // The page a job holds stays, and so do the other document's.
    assert_eq!(runtime.closes(), [1]);
    assert!(residency.is_held(first, pon(2)));
    assert!(residency.is_kept(second, pon(1)));

    residency.close_owner(first);
    drop(held);
    assert_eq!(runtime.closes(), [1, 2], "a closing document's held page closes with its holder");
    residency.close_owner(second);
    assert_eq!(runtime.closes(), [1, 2, 1]);
    assert_eq!(residency.size(), 0);
}

#[test]
fn closes_pages_no_job_uses_once_they_have_been_idle_a_while() {
    let now = Rc::new(Cell::new(0u64));
    let clock = Rc::clone(&now);
    let runtime = Runtime::new(&[(1, MB)]);
    let mut residency = PageResidency::with_clock(
        PageResidencyPolicy { budget_bytes: 100 * MB, idle_ms: 1000 },
        Box::new(move || clock.get()),
    );
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    now.set(999);
    residency.tick();
    assert_eq!(runtime.closes(), Vec::<u32>::new());
    now.set(1000);
    residency.tick();
    assert_eq!(runtime.closes(), [1]);
}

#[test]
fn closes_pages_no_view_shows_first_then_near_ones_then_visible_ones_from_the_fewest_pixels_up() {
    let runtime = Runtime::new(&[1, 2, 3, 4, 5, 6, 7].map(|page| (page, 4 * MB)));
    // Three pages fit.
    let mut residency = residency(13 * MB);
    let owner = residency.register_owner();
    residency.set_working_set(
        owner,
        "stage",
        [
            shown(1, Place::Visible, 1_000_000.0),
            shown(2, Place::Visible, 30_000.0),
            shown(7, Place::Visible, 500_000.0),
            shown(3, Place::Near, 0.0),
        ],
    );

    for page in [1, 2, 4] {
        use_page(&mut residency, &runtime, owner, page);
    }
    use_page(&mut residency, &runtime, owner, 3); // 4, in no set, goes before 2, visible and older
    assert_eq!(runtime.closes(), [4]);
    use_page(&mut residency, &runtime, owner, 7); // 3, near, goes before 2, visible and older
    assert_eq!(runtime.closes(), [4, 3]);
    use_page(&mut residency, &runtime, owner, 5); // 2 shows the fewest pixels; 1, older, the most
    assert_eq!(runtime.closes(), [4, 3, 2]);
    use_page(&mut residency, &runtime, owner, 6); // 5, in no set
    assert_eq!(runtime.closes(), [4, 3, 2, 5]);
    use_page(&mut residency, &runtime, owner, 3); // 6, in no set
    use_page(&mut residency, &runtime, owner, 2); // 3, near, before 7, visible
    assert_eq!(runtime.closes(), [4, 3, 2, 5, 6, 3]);
    assert!(residency.is_kept(owner, pon(1)));
}

#[test]
fn only_a_page_a_job_holds_keeps_the_total_over_the_budget_a_visible_page_closes_at_its_release() {
    let runtime = Runtime::new(&[(1, 4 * MB), (2, 4 * MB), (3, 4 * MB)]);
    let mut residency = residency(9 * MB);
    let owner = residency.register_owner();
    residency.set_working_set(owner, "stage", [shown(1, Place::Visible, 1_000_000.0)]);

    // Three jobs hold three pages at once: over the budget, as held pages
    // may be.
    let held: Vec<_> = [1, 2, 3].map(|page| acquire(&mut residency, &runtime, owner, page)).into();
    drop(held);
    residency.release(owner, pon(1));
    assert_eq!(runtime.closes(), [1]); // the only page no job holds
    residency.release(owner, pon(2));
    residency.release(owner, pon(3));
    assert_eq!(runtime.closes(), [1]); // within the budget again
}

#[test]
fn the_idle_timer_spares_what_a_view_shows_on_screen_and_follows_the_working_sets() {
    let now = Rc::new(Cell::new(0u64));
    let clock = Rc::clone(&now);
    let runtime = Runtime::new(&[1, 2, 3, 4].map(|page| (page, MB)));
    let mut residency = PageResidency::with_clock(
        PageResidencyPolicy { budget_bytes: 100 * MB, idle_ms: 1000 },
        Box::new(move || clock.get()),
    );
    let owner = residency.register_owner();
    residency.set_working_set(
        owner,
        "stage",
        [shown(1, Place::Visible, 1_000_000.0), shown(3, Place::Visible, 200_000.0)],
    );
    // Another view shows page 2, and has page 3 near.
    residency.set_working_set(
        owner,
        "rail",
        [shown(2, Place::Visible, 30_000.0), shown(3, Place::Near, 0.0)],
    );
    for page in [1, 2, 3, 4] {
        use_page(&mut residency, &runtime, owner, page);
    }

    let elapse = |residency: &mut PageResidency<FakePage>| {
        now.set(now.get() + 1000);
        residency.tick();
    };
    elapse(&mut residency);
    assert_eq!(runtime.closes(), [4]); // in no set; 3 is visible in one view
    residency.set_working_set(owner, "stage", [shown(1, Place::Visible, 1_000_000.0)]);
    elapse(&mut residency);
    assert_eq!(runtime.closes(), [4, 3]); // only near now
    residency.set_working_set(owner, "rail", []); // the rail withdraws
    elapse(&mut residency);
    assert_eq!(runtime.closes(), [4, 3, 2]);
    assert!(residency.is_kept(owner, pon(1)));
}

#[test]
fn a_residency_without_a_budget_keeps_nothing() {
    // `a session without a residency keeps nothing`.
    let runtime = Runtime::new(&[(1, MB)]);
    let mut residency = residency(0);
    let owner = residency.register_owner();

    use_page(&mut residency, &runtime, owner, 1);
    use_page(&mut residency, &runtime, owner, 1);
    assert_eq!(runtime.loads(), [1, 1]);
    assert_eq!(runtime.closes(), [1, 1]);
}

// ── Through the host (`page-residency-host.test.ts`, `page-retention.test.ts`) ─
//
// Upstream counts the native calls that load and close pages through a
// proxy over its runtime; here the residency counts them. The host cases
// that need a render or a read to pause between slices of its page load, an
// abort to stop a glyph loop part way, or requests held while a render runs
// have no counterpart: this host answers one call at a time.

mod host {
    use std::hash::{DefaultHasher, Hash, Hasher};

    use epdf_engine::host::Host;
    use epdf_engine::session::residency::ResidencyCounts;
    use serde_json::{Value, json};

    /// `heavyPdf`: pages of small coloured squares, a parse (and a render)
    /// of many steps. Pages are objects 3, 5, 7…
    fn heavy_pdf(page_count: u32, squares: u32) -> Vec<u8> {
        let mut objects: Vec<(u32, String)> =
            vec![(1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned())];
        let mut kids = Vec::new();
        for p in 0..page_count {
            let number = 3 + 2 * p;
            kids.push(format!("{number} 0 R"));
            let mut body = String::new();
            for i in 0..squares {
                let x = (i * 7 + p * 3) % 590;
                let y = (i * 13) % 830;
                body.push_str(&format!(
                    "{} {} {} rg {x} {y} 4 4 re f\n",
                    f64::from(i % 7) / 7.0,
                    f64::from(i % 5) / 5.0,
                    f64::from(i % 3) / 3.0
                ));
            }
            objects.push((
                number,
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 840] /Contents {} 0 R >>",
                    number + 1
                ),
            ));
            objects.push((
                number + 1,
                format!("<< /Length {} >>\nstream\n{body}endstream", body.len()),
            ));
        }
        objects.push((
            2,
            format!("<< /Type /Pages /Kids [{}] /Count {page_count} >>", kids.join(" ")),
        ));
        objects.sort_by_key(|(number, _)| *number);
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (number, body) in &objects {
            offsets.push(out.len());
            out.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    const FIRST: u64 = 3;
    const SECOND: u64 = 5;
    const THIRD: u64 = 7;
    /// One of these pages parses into about 8 MB natively (upstream's wasm
    /// pages, with their 32-bit pointers, into 5 to 6): this budget keeps
    /// one, never two.
    const ONE_PAGE_BUDGET: u64 = 12 * 1024 * 1024;
    /// And this one keeps two, never three.
    const TWO_PAGE_BUDGET: u64 = 20 * 1024 * 1024;

    fn page(number: u64) -> Value {
        json!({ "kind": "objectNumber", "objectNumber": number })
    }

    fn send(host: &mut Host, mut request: Value, payloads: Vec<Vec<u8>>) -> (Value, Vec<Vec<u8>>) {
        request["jobId"] = json!(1);
        request["docId"] = json!("doc");
        let response = host.call_filled(&request.to_string(), payloads);
        let envelope: Value = serde_json::from_str(&response.json).unwrap();
        assert!(envelope.get("error").is_none(), "{request}: {envelope}");
        (envelope["result"].clone(), response.payloads)
    }

    fn open(host: Host, bytes: Vec<u8>) -> Host {
        let mut host = host;
        send(&mut host, json!({ "kind": "open.fatMem" }), vec![bytes]);
        host
    }

    fn render(host: &mut Host, number: u64, viewport: Value) -> u64 {
        let (_, payloads) = send(
            host,
            json!({ "kind": "pages.render", "page": page(number),
                "options": { "viewport": viewport } }),
            Vec::new(),
        );
        let mut hasher = DefaultHasher::new();
        payloads[0].hash(&mut hasher);
        hasher.finish()
    }

    fn digest(host: &mut Host, number: u64) -> u64 {
        render(host, number, json!({ "kind": "scale", "scale": 0.25 }))
    }

    fn counts(host: &Host) -> ResidencyCounts {
        host.sessions().residency().borrow().counts()
    }

    #[test]
    fn a_page_parses_once_for_many_renders() {
        let mut host = open(Host::new(), heavy_pdf(2, 30_000));
        let first = digest(&mut host, FIRST);
        assert_eq!(digest(&mut host, FIRST), first);
        assert_eq!(digest(&mut host, FIRST), first);
        assert_eq!(counts(&host).loads, 1);
        assert_eq!(counts(&host).closes, 0);
    }

    #[test]
    fn reads_and_writes_that_leave_content_alone_keep_the_page_parsed() {
        let mut host = open(Host::new(), heavy_pdf(2, 30_000));
        let before = digest(&mut host, FIRST);
        send(&mut host, json!({ "kind": "pages.list" }), Vec::new());
        send(&mut host, json!({ "kind": "annotations.list" }), Vec::new());
        send(&mut host, json!({ "kind": "metadata.update", "patch": { "title": "kept" } }), vec![]);
        assert_eq!(digest(&mut host, FIRST), before);

        // An annotation drawn over the page: new pixels, from the same parse.
        send(
            &mut host,
            json!({ "kind": "annotations.create", "page": page(FIRST),
                "draft": { "subtype": "square",
                    "box": { "x": 100, "y": 100, "width": 200, "height": 200 } } }),
            Vec::new(),
        );
        assert_ne!(digest(&mut host, FIRST), before);
        assert_eq!(counts(&host).loads, 1);
    }

    #[test]
    fn a_write_that_changes_the_page_closes_it() {
        let mut host = open(Host::new(), heavy_pdf(2, 30_000));
        digest(&mut host, FIRST);
        send(
            &mut host,
            json!({ "kind": "document.apply", "change": { "ops": [
                { "type": "pages.rotate", "pages": [page(FIRST)], "rotation": 90 }] } }),
            Vec::new(),
        );
        digest(&mut host, FIRST);
        // Closed when next taken: its content no longer matches.
        assert!(counts(&host).closes > 0, "{:?}", counts(&host));
        assert_eq!(counts(&host).loads, 2);
    }

    #[test]
    fn a_content_write_closes_the_documents_kept_pages_around_it() {
        // `closePagesChangedBy`: a `contentWrite` closes its document's idle
        // pages before and after it.
        let mut host = open(Host::new(), heavy_pdf(2, 30_000));
        digest(&mut host, FIRST);
        digest(&mut host, SECOND);
        send(
            &mut host,
            json!({ "kind": "pages.flatten", "pages": [page(SECOND)], "usage": "display" }),
            Vec::new(),
        );
        assert_eq!(counts(&host).closes, 2, "{:?}", counts(&host));
        let session = host
            .sessions()
            .session(&epdf_engine::session::registry::SessionKey::document("doc"))
            .unwrap();
        assert_eq!(host.sessions().residency().borrow().size(), 0);
        assert_eq!(session.held_page_count(), 0);
    }

    #[test]
    fn pages_that_fit_together_both_stay_and_pages_that_do_not_replace_each_other() {
        let mut roomy = open(Host::new(), heavy_pdf(2, 30_000));
        for number in [FIRST, SECOND, FIRST, SECOND] {
            digest(&mut roomy, number);
        }
        assert_eq!(counts(&roomy).loads, 2);

        // One page fits, two don't: each page closes the other.
        let mut tight =
            open(Host::new().with_parsed_page_budget(ONE_PAGE_BUDGET), heavy_pdf(2, 30_000));
        for number in [FIRST, SECOND, FIRST, SECOND] {
            digest(&mut tight, number);
        }
        assert_eq!(counts(&tight).loads, 4);
    }

    #[test]
    fn a_page_bigger_than_the_whole_budget_parses_once_for_all_the_jobs_on_it() {
        let mut host = open(Host::new().with_parsed_page_budget(1024 * 1024), heavy_pdf(2, 30_000));
        let first = digest(&mut host, FIRST);
        send(&mut host, json!({ "kind": "pages.geometry", "page": page(FIRST) }), Vec::new());
        assert_eq!(digest(&mut host, FIRST), first);
        assert_eq!(digest(&mut host, FIRST), first);
        assert_eq!(counts(&host).loads, 1);
        assert_eq!(counts(&host).closes, 0);
    }

    #[test]
    fn a_working_set_decides_which_page_a_load_closes() {
        let mut host =
            open(Host::new().with_parsed_page_budget(TWO_PAGE_BUDGET), heavy_pdf(3, 30_000));
        digest(&mut host, SECOND);
        digest(&mut host, FIRST);
        // The view shows the second page big and the first small, so the
        // third page's load closes the first, though the second was used
        // longer ago. The working set is never answered beyond its tag.
        let (answer, _) = send(
            &mut host,
            json!({ "kind": "pages.workingSet", "view": "stage", "pages": [
                { "page": page(SECOND), "role": "visible", "pixels": 1_000_000 },
                { "page": page(FIRST), "role": "visible", "pixels": 30_000 },
            ] }),
            Vec::new(),
        );
        assert_eq!(answer, Value::Null);
        digest(&mut host, THIRD);
        assert_eq!(counts(&host).loads, 3);
        digest(&mut host, SECOND); // still parsed
        assert_eq!(counts(&host).loads, 3);
    }

    // ── `page-retention.test.ts` ──────────────────────────────────────────

    const RETENTION: &str = "packages/engine/main/test/fixtures/jpx_resolution_levels.pdf";
    const WIDTHS: [u32; 5] = [1600, 640, 3000, 150, 640];

    fn retention() -> (Host, u64) {
        let mut host = open(Host::new(), test_support::fixture(RETENTION));
        let (pages, _) = send(&mut host, json!({ "kind": "pages.list" }), Vec::new());
        let first = pages["pages"][0]["ref"]["objectNumber"].as_u64().unwrap();
        (host, first)
    }

    fn at_width(host: &mut Host, number: u64, width: u32) -> u64 {
        render(host, number, json!({ "kind": "width", "width": width }))
    }

    fn appearances(host: &mut Host, number: u64) -> Vec<u64> {
        let (_, payloads) = send(
            host,
            json!({ "kind": "annotations.renderAppearances", "page": page(number),
                "family": "annotations",
                "options": { "viewport": { "kind": "scale", "scale": 0.5 } } }),
            Vec::new(),
        );
        payloads
            .iter()
            .map(|bytes| {
                let mut hasher = DefaultHasher::new();
                bytes.hash(&mut hasher);
                hasher.finish()
            })
            .collect()
    }

    #[test]
    fn a_kept_page_renders_the_same_bytes_as_a_freshly_loaded_one_whatever_rendered_before() {
        // The fixture's JPEG 2000 image decodes at a resolution that depends
        // on the render size and is cached on the page: a page that carried
        // its image cache from one job to the next would render differently.
        let (mut host, number) = retention();
        let kept: Vec<u64> = WIDTHS.iter().map(|w| at_width(&mut host, number, *w)).collect();
        let fresh: Vec<u64> = WIDTHS
            .iter()
            .map(|w| {
                let (mut host, number) = retention();
                at_width(&mut host, number, *w)
            })
            .collect();
        let distinct: std::collections::BTreeSet<_> = fresh.iter().collect();
        assert!(distinct.len() > 1);
        assert_eq!(kept, fresh);
        assert_eq!(counts(&host).loads, 1, "the page was not kept between the renders");
    }

    #[test]
    fn annotation_appearances_render_as_a_fresh_documents_after_a_page_render() {
        let (mut host, number) = retention();
        at_width(&mut host, number, 3000);
        let kept = appearances(&mut host, number);
        let (mut fresh_host, number) = retention();
        let fresh = appearances(&mut fresh_host, number);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept, fresh);
    }

    #[test]
    fn an_image_a_redaction_changed_shows_as_in_a_document_that_never_rendered_it() {
        let redact = |host: &mut Host, number: u64| {
            send(
                host,
                json!({ "kind": "annotations.create", "page": page(number),
                    "draft": { "subtype": "redact",
                        "rect": { "x": 100, "y": 100, "width": 100, "height": 100 },
                        "interiorColor": "#000000" } }),
                Vec::new(),
            );
            send(
                host,
                json!({ "kind": "redaction.apply", "scope": { "pages": [page(number)] } }),
                Vec::new(),
            );
        };
        let (mut host, number) = retention();
        let before = at_width(&mut host, number, 640);
        redact(&mut host, number);
        let kept = at_width(&mut host, number, 640);
        let (mut fresh_host, fresh_number) = retention();
        redact(&mut fresh_host, fresh_number);
        let fresh = at_width(&mut fresh_host, fresh_number, 640);
        assert_ne!(kept, before);
        assert_eq!(kept, fresh);
    }

    #[test]
    fn a_change_to_the_page_shows_as_in_a_document_that_made_it_without_a_kept_page() {
        let flatten = |host: &mut Host, number: u64| {
            send(
                host,
                json!({ "kind": "pages.flatten", "pages": [page(number)], "usage": "display" }),
                Vec::new(),
            );
        };
        let (mut host, number) = retention();
        at_width(&mut host, number, 640);
        flatten(&mut host, number);
        let kept = at_width(&mut host, number, 640);
        let (mut fresh_host, fresh_number) = retention();
        flatten(&mut fresh_host, fresh_number);
        assert_eq!(kept, at_width(&mut fresh_host, fresh_number, 640));
    }
}
