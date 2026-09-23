//! The Stage B bridge spike's gate.
//!
//! Phase 0's exit gate asks that the "bridge spike passes failure-after-write,
//! cache invalidation, close-under-lease". Those three are the last three tests
//! here. The ones before them are the protocol rules Section 6 states, each of
//! which is a way for the host to corrupt a Rust-owned session if the state
//! machine does not refuse it.
//!
//! Every rejection here is enforced at runtime rather than by the compiler,
//! and that is the point of the spike: no Rust borrow spans the host's call, so
//! nothing about the lease is checkable statically. If this protocol is wrong,
//! it is wrong in a way that only tests can find.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::spike::lease::{LeaseOutcome, SpikeSession};
use pdfium::{Document, Library};

const WROTE: LeaseOutcome = LeaseOutcome { wrote: true, failed: false };
const READ_ONLY: LeaseOutcome = LeaseOutcome { wrote: false, failed: false };
const WROTE_THEN_FAILED: LeaseOutcome = LeaseOutcome { wrote: true, failed: true };

fn session() -> SpikeSession {
    let library = Library::acquire();
    let bytes = test_support::fixture(test_support::REPORT);
    SpikeSession::new(Document::from_bytes(library, &bytes, None).expect("open"))
}

#[test]
fn a_lease_round_trips_and_leaves_the_session_usable() {
    let mut session = session();
    let pages = session.page_count().expect("read before");

    let (token, handle) = session.checkout().expect("checkout");
    assert!(!handle.is_null(), "the host gets a real document pointer");
    session.return_lease(token, READ_ONLY).expect("return");
    let settled = session.finalize(token).expect("finalize");

    assert_eq!(settled.mutation_sequence, 0, "a read-only lease advances nothing");
    assert_eq!(session.page_count().expect("read after"), pages);
}

#[test]
fn a_second_checkout_is_refused_while_one_is_outstanding() {
    let mut session = session();
    let (token, _) = session.checkout().expect("first checkout");

    let refused = session.checkout().expect_err("a second checkout is a protocol violation");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);

    // And the first lease is still the live one, not invalidated by the
    // refusal.
    session.return_lease(token, READ_ONLY).expect("return");
    session.finalize(token).expect("finalize");
    session.checkout().expect("a checkout works again once settled");
}

#[test]
fn other_document_operations_are_refused_while_a_lease_is_out() {
    let mut session = session();
    let (token, _) = session.checkout().expect("checkout");

    let refused = session.page_count().expect_err("reads are refused mid-lease");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);

    // Returning is not enough: the revision is stale until the outcome is
    // finalized, so the refusal has to survive the return.
    session.return_lease(token, WROTE).expect("return");
    let still_refused = session.page_count().expect_err("reads are refused until finalize");
    assert_eq!(still_refused.code, EngineErrorCode::InvalidArg);

    session.finalize(token).expect("finalize");
    session.page_count().expect("reads work once the outcome is settled");
}

#[test]
fn the_protocol_refuses_out_of_order_and_stale_tokens() {
    let mut session = session();

    // A settled token, so the next finalize has a real value to refuse rather
    // than a fabricated one.
    let settled_token = first_token(&mut session);
    assert_eq!(
        session.finalize(settled_token).expect_err("nothing to finalize").code,
        EngineErrorCode::InvalidArg,
    );

    let (token, _) = session.checkout().expect("checkout");
    assert_eq!(
        session.finalize(token).expect_err("finalize before return").code,
        EngineErrorCode::InvalidArg,
        "finalizing while the host still holds the pointer would settle a revision mid-write",
    );

    session.return_lease(token, WROTE).expect("return");
    assert_eq!(
        session.return_lease(token, WROTE).expect_err("double return").code,
        EngineErrorCode::InvalidArg,
        "a second return would let one write finalize twice",
    );
    session.finalize(token).expect("finalize");

    // A token from a settled lease must not work on the next one.
    let (next, _) = session.checkout().expect("second checkout");
    assert_eq!(
        session.return_lease(token, WROTE).expect_err("stale token").code,
        EngineErrorCode::InvalidArg,
    );
    session.return_lease(next, READ_ONLY).expect("the current token still works");
}

/// A token the session has never issued, for the "nothing to finalize" case.
fn first_token(session: &mut SpikeSession) -> epdf_engine::spike::lease::LeaseToken {
    let (token, _) = session.checkout().expect("checkout");
    session.return_lease(token, READ_ONLY).expect("return");
    session.finalize(token).expect("finalize");
    token
}

// --- The three the exit gate names ------------------------------------------

#[test]
fn failure_after_write_still_finalizes_the_write() {
    // Section 7: mutations are not rollback-atomic, and no throw path may
    // pretend no write occurred. The host wrote and then failed; the sequence
    // must advance anyway, and the failure must travel with it.
    let mut session = session();
    let before = session.mutation_sequence();

    let (token, _) = session.checkout().expect("checkout");
    session.return_lease(token, WROTE_THEN_FAILED).expect("return after a failed write");
    let settled = session.finalize(token).expect("finalize runs even so");

    assert_eq!(settled.mutation_sequence, before + 1, "the write happened, so the sequence moves");
    assert!(settled.failed, "and the caller still learns the operation failed");
    assert!(!session.is_closed(), "a failed write does not close the session");
    session.page_count().expect("the session is usable afterwards");
}

#[test]
fn a_write_invalidates_the_cached_form_model_whether_or_not_it_succeeded() {
    // `formModelCache.ts` keys its one snapshot on the mutation sequence, so
    // anything that moves the sequence drops it. Tested on both outcomes
    // because tying invalidation to success is the easy mistake: the cache
    // would then survive a half-applied write and describe a document that no
    // longer exists.
    for outcome in [WROTE, WROTE_THEN_FAILED] {
        let mut session = session();
        session.load_form_model().expect("load");
        assert!(session.form_model_loaded());

        let (token, _) = session.checkout().expect("checkout");
        session.return_lease(token, outcome).expect("return");
        let settled = session.finalize(token).expect("finalize");

        assert!(settled.form_model_invalidated, "{outcome:?} should have dropped the model");
        assert!(!session.form_model_loaded());
    }

    // A read-only lease leaves it alone, or the cache would be worthless.
    let mut session = session();
    session.load_form_model().expect("load");
    let (token, _) = session.checkout().expect("checkout");
    session.return_lease(token, READ_ONLY).expect("return");
    let settled = session.finalize(token).expect("finalize");
    assert!(!settled.form_model_invalidated);
    assert!(session.form_model_loaded());
}

#[test]
fn close_under_lease_waits_for_the_lease_before_releasing_the_document() {
    // The one that would be a use-after-free rather than a wrong answer. The
    // host holds a pointer into the document; a close that dropped it there
    // would hand the host a dangling FPDF_DOCUMENT, and PDFium would find out
    // before the host did.
    let mut session = session();
    let (token, handle) = session.checkout().expect("checkout");
    assert!(!handle.is_null());

    let released = session.close();
    assert!(!released, "a close cannot release a document the host is holding");
    assert!(!session.is_closed(), "and it is not closed until it is");

    // New work is refused from the moment close is called, which is the other
    // half of the contract: reject new work, then drain.
    assert_eq!(
        session.page_count().expect_err("reads refused once closing").code,
        EngineErrorCode::DocNotOpen,
    );
    assert_eq!(
        session.checkout().expect_err("checkouts refused once closing").code,
        EngineErrorCode::DocNotOpen,
    );

    // The return and the finalize are what the close is waiting for, so they
    // are still accepted. Refusing them would deadlock the session.
    session.return_lease(token, WROTE).expect("the outstanding lease still comes home");
    assert!(!session.is_closed(), "still not closed: the outcome is unfinalized");
    let settled = session.finalize(token).expect("finalization completes the close");

    assert_eq!(settled.mutation_sequence, 1, "the write that was in flight still counts");
    assert!(session.is_closed(), "now the document is released");
    assert_eq!(
        session.page_count().expect_err("a closed session serves nothing").code,
        EngineErrorCode::DocNotOpen,
    );
}

#[test]
fn closing_an_idle_session_releases_it_at_once() {
    let mut session = session();
    assert!(session.close(), "nothing is outstanding, so there is nothing to wait for");
    assert!(session.is_closed());
    // Idempotent, per the lifecycle contract.
    assert!(session.close());
}

#[test]
fn a_write_through_the_leased_pointer_lands_and_rust_sees_it() {
    // The question the state machine cannot answer. Everything above is about
    // what the protocol refuses; this is about whether the thing it permits
    // actually works: a caller takes the document pointer, mutates PDFium
    // through it, gives it back, and the session that owns the document sees
    // the change.
    //
    // The caller here is Rust rather than TypeScript, and that substitution is
    // the one thing this test does not cover. What crosses the boundary is an
    // `FPDF_DOCUMENT` and nothing else, so the pointer handoff is the same
    // either way; what is not covered is whether `AnnotationMutator` composes
    // over a session it does not own. Proving that needs the runtime to hand
    // back its Emscripten module, which is a change to shipping surface for
    // the sake of a throwaway, so it is recorded as untested rather than
    // bought.
    let mut session = session();
    let before = session.annotation_count(0).expect("count before");

    let (token, document) = session.checkout().expect("checkout");

    // What the host does with the lease: open a page off the borrowed
    // document, add an annotation, close the page. Straight PDFium, because
    // that is all the host has.
    let added = unsafe {
        let doc = document.cast::<pdfium_sys::fpdf_document_t__>();
        let page = pdfium_sys::FPDF_LoadPage(doc, 0);
        assert!(!page.is_null(), "the leased document opens its own pages");
        // FPDF_ANNOT_SQUARE is 4.
        let annotation = pdfium_sys::FPDFPage_CreateAnnot(page, 4);
        let created = !annotation.is_null();
        if created {
            pdfium_sys::FPDFPage_CloseAnnot(annotation);
        }
        pdfium_sys::FPDF_ClosePage(page);
        created
    };
    assert!(added, "PDFium accepted a write through the leased pointer");

    session.return_lease(token, WROTE).expect("return");
    let settled = session.finalize(token).expect("finalize");
    assert_eq!(settled.mutation_sequence, 1);

    let after = session.annotation_count(0).expect("count after");
    assert_eq!(after, before + 1, "the owner sees the annotation the borrower added");
}
