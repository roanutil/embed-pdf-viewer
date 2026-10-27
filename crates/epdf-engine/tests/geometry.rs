//! `pages.geometry` against real documents, in both ownership modes.

use std::ffi::c_void;

use epdf_engine::access::{BorrowedPage, DocumentAccess, OwnedDocument, PageAccess};
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::geometry::{PageGeometrySnapshot, read_page_geometry};
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};

fn open(fixture: &str) -> OwnedDocument {
    let lib = Library::acquire();
    let document = Document::from_bytes(lib, &test_support::fixture(fixture), None)
        .unwrap_or_else(|e| panic!("open {fixture}: {e}"));
    OwnedDocument::new(document).expect("index pages")
}

fn first_page_number(doc: &OwnedDocument) -> PageObjectNumber {
    let page = doc.document().page(0).expect("page 0");
    PageObjectNumber::new(page.object_number()).expect("page 0 is an indirect object")
}

/// Every glyph box a run publishes is normalized, and the run's rect encloses
/// the non-empty glyphs in it. Both are properties the orientation conformance
/// suite asserts.
fn assert_snapshot_invariants(snapshot: &PageGeometrySnapshot, char_count: u32) {
    let mut next_char = 0;
    for run in &snapshot.runs {
        assert_eq!(run.start(), next_char, "run starts do not tile");
        next_char += run.glyph_count() as u32;

        let rect = run.rect();
        assert!(rect.left <= rect.right && rect.bottom <= rect.top, "run rect not normalized");

        for i in 0..run.glyph_count() {
            let bounds = run.glyph_loose_bounds(i).expect("glyph in range");
            assert!(
                bounds.left <= bounds.right && bounds.bottom <= bounds.top,
                "glyph box not normalized"
            );
            let quad = run.glyph_loose_quad(i).expect("glyph in range");
            // The uniform quad view and the uniform box view describe the same
            // cell, so the box must contain the quad's corners.
            for p in [quad.p1, quad.p2, quad.p3, quad.p4] {
                assert!(p.x >= bounds.left - 1e-6 && p.x <= bounds.right + 1e-6);
                assert!(p.y >= bounds.bottom - 1e-6 && p.y <= bounds.top + 1e-6);
            }
        }
    }
    assert_eq!(next_char, char_count, "runs do not cover every character");
}

#[test]
fn reads_geometry_for_an_upright_page() {
    let doc = open(test_support::REPORT);
    let page = doc.page(first_page_number(&doc)).expect("page by object number");
    let char_count = page.view().text().expect("text page").char_count();

    let snapshot = read_page_geometry(&page, &CancelToken::never()).expect("read geometry");
    assert!(!snapshot.runs.is_empty());
    assert_snapshot_invariants(&snapshot, char_count);
    // report.pdf is ordinary upright text, so nothing pays for orientation.
    assert!(snapshot.runs.iter().all(|r| !r.is_rotated()));
}

#[test]
fn reads_oriented_cells_from_a_page_with_rotated_text() {
    let doc = open(test_support::ROTATED_TEXT);
    let page = doc.page(first_page_number(&doc)).expect("page by object number");
    let char_count = page.view().text().expect("text page").char_count();

    let snapshot = read_page_geometry(&page, &CancelToken::never()).expect("read geometry");
    assert_snapshot_invariants(&snapshot, char_count);
    assert!(
        snapshot.runs.iter().any(|r| r.is_rotated()),
        "rotated_text.pdf produced no rotated run, so the rotated arm is untested"
    );
}

#[test]
fn a_borrowed_page_and_an_owned_page_read_identically() {
    // The property hazards 3.4 exists for: one reader, two ownership modes.
    // Stage A lends the pointer, Stage B owns it, and the answer is the same.
    let doc = open(test_support::REPORT);
    let owned = doc.page(first_page_number(&doc)).expect("page by object number");

    let raw = unsafe { owned.view().as_raw() }.cast::<c_void>();
    // SAFETY: `owned` keeps the page open for the whole of this scope, and
    // `borrowed` closes nothing.
    let borrowed = unsafe { BorrowedPage::from_raw(raw) }.expect("wrap the lent pointer");

    let from_owned = read_page_geometry(&owned, &CancelToken::never()).unwrap();
    let from_borrowed = read_page_geometry(&borrowed, &CancelToken::never()).unwrap();
    assert_eq!(from_owned, from_borrowed);
    assert_eq!(borrowed.object_number().unwrap(), owned.object_number().unwrap());
}

#[test]
fn a_null_borrowed_pointer_is_rejected_rather_than_dereferenced() {
    let err = unsafe { BorrowedPage::from_raw(std::ptr::null_mut()) }.unwrap_err();
    assert!(err.is(EngineErrorCode::InvalidArg));
}

#[test]
fn a_cancelled_token_stops_the_read_before_it_reports_geometry() {
    let doc = open(test_support::REPORT);
    let page = doc.page(first_page_number(&doc)).expect("page by object number");

    let token = CancelToken::never();
    token.cancel();
    let err = read_page_geometry(&page, &token).unwrap_err();
    assert!(err.is(EngineErrorCode::Aborted));
}

#[test]
fn a_page_object_number_the_document_does_not_have_is_not_found() {
    let doc = open(test_support::REPORT);
    let missing = PageObjectNumber::new(u32::MAX).unwrap();
    let err = doc.page(missing).unwrap_err();
    assert!(err.is(EngineErrorCode::NotFound));
}

#[test]
fn every_page_in_the_document_reads() {
    let doc = open(test_support::REPORT);
    assert!(doc.page_count() > 0);
    for index in 0..doc.page_count() {
        let page = doc.document().page(index).expect("load page");
        let number = PageObjectNumber::new(page.object_number()).expect("indirect page object");
        let by_number = doc.page(number).expect("page by object number");
        let char_count = by_number.view().text().expect("text page").char_count();
        let snapshot = read_page_geometry(&by_number, &CancelToken::never()).expect("geometry");
        assert_snapshot_invariants(&snapshot, char_count);
    }
}
