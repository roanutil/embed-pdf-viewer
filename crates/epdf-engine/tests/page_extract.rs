//! G9: exporting a subset of pages as a standalone PDF.
//!
//! The gate is a round trip: extract, reopen the bytes, and compare the page
//! count and sizes against what was asked for. That is the only check that
//! proves the output is a document rather than a plausible-looking buffer.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::extract::extract_pages;
use epdf_engine::features::pages::list_pages;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use pdfium::{Document, Library};

fn report() -> Vec<u8> {
    test_support::fixture(test_support::REPORT)
}

fn rotated() -> Vec<u8> {
    std::fs::read(
        test_support::workspace_root()
            .join("tooling/engine-conformance/fixtures/rotated_pages.pdf"),
    )
    .expect("read rotated_pages.pdf")
}

fn session(bytes: Vec<u8>) -> DocumentSession {
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    session
}

fn numbers(session: &mut DocumentSession) -> Vec<PageObjectNumber> {
    session.all_records().expect("records").iter().map(|r| r.page_object_number).collect()
}

/// Reopen extracted bytes and report each page's size.
fn reopen(bytes: &[u8]) -> Vec<(f64, f64)> {
    let document = Document::from_bytes(Library::acquire(), bytes, None)
        .expect("the extracted bytes are a document");
    list_pages(&document, &CancelToken::never())
        .expect("list")
        .pages
        .into_iter()
        .map(|p| (p.size.width, p.size.height))
        .collect()
}

#[test]
fn one_page_round_trips() {
    let mut session = session(report());
    let all = numbers(&mut session);
    let bytes = extract_pages(&mut session, &all[..1], &CancelToken::never()).expect("extract");

    assert!(bytes.starts_with(b"%PDF"), "the output is not a PDF");
    let sizes = reopen(&bytes);
    assert_eq!(sizes.len(), 1);
}

#[test]
fn every_page_round_trips_with_its_size() {
    // The gate: reopen and compare count and sizes against the source.
    let mut session = session(report());
    let all = numbers(&mut session);
    let source: Vec<(f64, f64)> =
        list_pages(session.document().expect("open").document(), &CancelToken::never())
            .expect("list")
            .pages
            .into_iter()
            .map(|p| (p.size.width, p.size.height))
            .collect();

    let bytes = extract_pages(&mut session, &all, &CancelToken::never()).expect("extract");
    assert_eq!(reopen(&bytes), source, "the extracted document differs from its source");
}

#[test]
fn the_callers_order_is_preserved() {
    // Extracting [3, 1] must produce those pages in that order. A caller
    // reordering pages this way is using the operation as intended, and
    // sorting the request would silently do something else.
    let mut session = session(rotated());
    let all = numbers(&mut session);
    assert_eq!(all.len(), 4);

    // rotated_pages.pdf has four pages whose /Rotate differs, so the page
    // order is observable in the output's rotations.
    let picked = vec![all[3], all[1]];
    let bytes = extract_pages(&mut session, &picked, &CancelToken::never()).expect("extract");

    let document = Document::from_bytes(Library::acquire(), &bytes, None).expect("reopen");
    let listing = list_pages(&document, &CancelToken::never()).expect("list");
    assert_eq!(listing.page_count, 2);
    assert_eq!(
        listing.pages.iter().map(|p| p.rotation.degrees()).collect::<Vec<_>>(),
        [270, 90],
        "the pages came back in document order rather than the caller's",
    );
}

#[test]
fn an_empty_selection_is_refused() {
    let mut session = session(report());
    let refused = extract_pages(&mut session, &[], &CancelToken::never()).expect_err("empty");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
}

#[test]
fn a_duplicate_page_is_refused_rather_than_deduplicated() {
    // Collapsing it would return a document with fewer pages than were asked
    // for, which is a worse answer than an error.
    let mut session = session(report());
    let all = numbers(&mut session);
    let refused = extract_pages(&mut session, &[all[0], all[0]], &CancelToken::never())
        .expect_err("duplicate");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("duplicate"), "{}", refused.message);
}

#[test]
fn a_page_the_document_does_not_have_is_not_found() {
    let mut session = session(report());
    let missing = PageObjectNumber::new(99_999).expect("number");
    let refused =
        extract_pages(&mut session, &[missing], &CancelToken::never()).expect_err("missing");
    assert_eq!(refused.code, EngineErrorCode::NotFound);
}

#[test]
fn extracting_leaves_the_source_session_untouched() {
    // A read, not a mutation: no revision bump, no registry change. A caller
    // holding a revision token across an extract must still find it valid.
    let mut session = session(report());
    let all = numbers(&mut session);
    let token = session.page_state(all[0]).expect("state").revision;
    let before = session.mutation_sequence();

    extract_pages(&mut session, &all[..2], &CancelToken::never()).expect("extract");

    session.validate_revision(&token).expect("the token survived the extract");
    assert_eq!(session.mutation_sequence(), before, "extracting bumped the mutation sequence");
    assert_eq!(session.all_records().expect("records").len(), 14, "the registry changed");
}

#[test]
fn a_cancelled_extract_reports_aborted() {
    let mut session = session(report());
    let all = numbers(&mut session);
    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        extract_pages(&mut session, &all[..1], &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}

#[test]
fn extracting_from_a_closed_session_is_refused() {
    let mut session = session(report());
    let all = numbers(&mut session);
    session.close();
    assert_eq!(
        extract_pages(&mut session, &all[..1], &CancelToken::never()).expect_err("closed").code,
        EngineErrorCode::DocNotOpen,
    );
}
