//! G3's cancel traces: what a token actually stops, and what it cannot.
//!
//! `multiplatform-plan.md` Section 7 sorts cancellation into four cases, and
//! the point of these tests is that the engine's behaviour matches the case it
//! claims rather than the case a caller would like. A token that quietly did
//! nothing would pass every other test in this repository.
//!
//! The four cases, and where each is checked:
//!
//! 1. Queued, not started. The scheduler's decision, not the token's, so there
//!    is nothing here to test: `SessionRegistry::abort` cancels the token and
//!    a job that never started never observes it.
//! 2. Cooperative, before any write. Covered below: a read cancelled before it
//!    starts and a read cancelled partway through both report `Aborted`.
//! 3. Non-preemptible. One synchronous native call cannot be interrupted, and
//!    the tests here assert that the engine does not pretend otherwise.
//! 4. After the first write. Mutation work, and there are no native mutators
//!    yet; noted so the gap is visible rather than assumed covered.
//!
//! # What these tests do and do not pin
//!
//! `read_page_geometry` checks its token in three places: on entry, after
//! loading the text page, and once per glyph. Deleting any one of them leaves
//! this suite green, because the other two still catch the cancellation;
//! deleting all three fails three tests. Keeping only the per-glyph check also
//! leaves it green, so that one is genuinely exercised rather than dead.
//!
//! So what is pinned is that the reader honours cancellation at all, not the
//! position of any individual checkpoint. That is the right granularity for a
//! read: the checkpoints are deliberately redundant, and a test that failed
//! when one moved would be a test of the loop's shape. It is the wrong
//! granularity for a mutator, where the checkpoint before the apply boundary
//! is the whole contract, and G11 will need a test that pins that one
//! specifically.

use std::cell::Cell;

use epdf_engine::access::{DocumentAccess, OwnedDocument, PageAccess};
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::geometry::read_page_geometry;
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library, PageView};

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
        .expect("object number")
}

#[test]
fn a_token_that_was_never_cancelled_does_not_stop_a_read() {
    // The control. Without it, every assertion below would also pass on a
    // reader that always failed.
    let document = document();
    let page = document.page(first_page(&document)).expect("page");
    let snapshot = read_page_geometry(&page, &CancelToken::never()).expect("read");
    assert!(!snapshot.runs.is_empty(), "the page has text to read");
}

#[test]
fn a_read_cancelled_before_it_starts_reports_aborted() {
    let document = document();
    let page = document.page(first_page(&document)).expect("page");
    let cancel = CancelToken::never();
    cancel.cancel();

    let error = read_page_geometry(&page, &cancel).expect_err("cancelled");
    assert_eq!(error.code, EngineErrorCode::Aborted);
}

/// A page that cancels the token partway through being read.
///
/// The only honest way to test a cooperative checkpoint is to cancel between
/// two of them, and the reader's checkpoints are inside its own loop. This
/// wrapper cancels on the nth call to `view`, which the reader makes once, so
/// it stands in for a caller that aborted while the read was running.
struct CancelsOnRead<'a> {
    inner: &'a dyn PageAccess,
    cancel: CancelToken,
    calls: Cell<u32>,
    cancel_at: u32,
}

impl PageAccess for CancelsOnRead<'_> {
    fn view(&self) -> &PageView<'_> {
        let seen = self.calls.get() + 1;
        self.calls.set(seen);
        if seen >= self.cancel_at {
            self.cancel.cancel();
        }
        self.inner.view()
    }
}

#[test]
fn a_read_cancelled_while_it_runs_reports_aborted_rather_than_a_partial_snapshot() {
    // The failure this guards against is not an error, it is a success: a
    // reader that noticed the cancellation and returned what it had so far
    // would hand a caller a page that is missing text, with no indication
    // that anything went wrong.
    let document = document();
    let page = document.page(first_page(&document)).expect("page");
    let cancel = CancelToken::never();
    let wrapper =
        CancelsOnRead { inner: &page, cancel: cancel.clone(), calls: Cell::new(0), cancel_at: 1 };

    let error = read_page_geometry(&wrapper, &cancel).expect_err("cancelled mid-read");
    assert_eq!(error.code, EngineErrorCode::Aborted);
    assert!(cancel.is_cancelled());
}

#[test]
fn cancelling_one_read_does_not_cancel_another() {
    // Tokens are per job. A shared one would make an abort of any job stop
    // every job, which is the bug a single global flag produces.
    let document = document();
    let page = document.page(first_page(&document)).expect("page");
    let first = CancelToken::never();
    let second = CancelToken::never();
    first.cancel();

    assert_eq!(
        read_page_geometry(&page, &first).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
    read_page_geometry(&page, &second).expect("the other job is unaffected");
}

#[test]
fn a_cancelled_token_stays_cancelled() {
    // There is no reset. A job that observed its cancellation cannot be told
    // to carry on, because whoever cancelled it may already have released the
    // inputs it was reading.
    let cancel = CancelToken::never();
    cancel.cancel();
    assert!(cancel.is_cancelled());
    assert!(cancel.is_cancelled(), "checking did not clear it");
    assert_eq!(cancel.check().expect_err("still cancelled").code, EngineErrorCode::Aborted);
}

#[test]
fn a_clone_shares_the_flag_with_its_original() {
    // How an abort reaches a job at all: the registry holds one clone and the
    // job holds another.
    let held_by_registry = CancelToken::never();
    let held_by_job = held_by_registry.clone();

    assert!(!held_by_job.is_cancelled());
    held_by_registry.cancel();
    assert!(held_by_job.is_cancelled(), "the clone did not see the cancellation");
}

#[test]
fn rendering_is_not_interruptible_and_does_not_pretend_to_be() {
    // Case 3. FPDF_RenderPageBitmap is one synchronous native call: the wait
    // ends, the work completes. Cancelling during it changes nothing, and an
    // engine that reported Aborted here would be claiming it stopped work that
    // in fact finished.
    let document = document();
    let page = document.page(first_page(&document)).expect("page");
    let cancel = CancelToken::never();
    cancel.cancel();

    // The render path takes no token, which is the honest encoding of case 3:
    // there is no checkpoint to offer.
    let bitmap = page.view().render_bgra(120, 160).expect("render");
    assert_eq!(bitmap.width, 120);
    assert_eq!(bitmap.height, 160);
    assert!(cancel.is_cancelled(), "the token is set and the render still ran");
}
