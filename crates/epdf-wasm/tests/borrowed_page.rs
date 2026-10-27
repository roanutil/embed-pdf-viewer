//! The Stage A bridge, end to end, over a real page.
//!
//! The unit tests in the crate cover the refusals. This one covers the path
//! that matters: a page the caller owns, lent across the C ABI, read, and the
//! JSON envelope compared against the same frozen vectors
//! `crates/epdf-engine/tests/geometry_corpus.rs` uses. If the envelope, the
//! borrow or the handoff is wrong, this fails where a null-pointer test would
//! not.
//!
//! It runs on the host rather than in WASM, because what is being tested is
//! the ABI and the ownership contract, and those are the same either way. The
//! linked module is a separate gate.
//!
//! The export answers the file's coordinates, as upstream's
//! `PageGeometryReader` does and as `RustPageGeometryReader.ts` reads it; the
//! TypeScript worker host's boundary turns that into page space. The vectors
//! are page space, so this test turns the answer with the engine's own
//! transform (`page_geometry_of`) before it compares.

use std::ffi::c_void;

use epdf_engine::features::geometry::PageGeometrySnapshot;
use epdf_engine::features::pages::visible_box_reader;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::page_space::text::page_geometry_of;
use pdfium::{Document, Library};
use serde_json::Value;
use test_support::corpus;

/// Reads the envelope the way the host will: take the pointer and the length,
/// copy the bytes out, hand the buffer straight back.
fn call_geometry(page: *mut c_void) -> Value {
    let mut len = 0u32;
    let ptr = unsafe { epdf_wasm::epdf_page_geometry(page, &mut len) };
    assert!(!ptr.is_null(), "a non-null out_len always gets a buffer");
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    unsafe { epdf_wasm::epdf_buffer_free(ptr, len) };
    serde_json::from_slice(&bytes).expect("the envelope is JSON")
}

/// Every corpus page read through the C ABI matches the TypeScript reader.
///
/// The same comparison as the geometry corpus gate, one layer out: this one
/// goes through the envelope, so a bridge that loses a field or wraps the
/// snapshot wrongly fails here even while the reader itself is correct.
#[test]
fn the_borrowed_page_export_matches_the_frozen_vectors() {
    let cases = corpus::load("pages.geometry");
    let mut compared = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let library = Library::acquire();
        let bytes = test_support::fixture(&case.fixture);
        let document = Document::from_bytes(library, &bytes, case.password.as_deref())
            .unwrap_or_else(|e| panic!("{}: open {}: {e}", case.case_name, case.fixture));
        let mut box_of = visible_box_reader(&document);

        for page_vector in &case.pages {
            let page = document.page(page_vector.index).expect("page by index");
            // The borrow the host performs. `page` stays alive for the call and
            // is dropped here, not inside Rust: that is the whole Stage A rule.
            let envelope = call_geometry(unsafe { page.as_raw() }.cast());

            assert!(
                envelope.get("error").is_none(),
                "{} page {}: {}",
                case.case_name,
                page_vector.index,
                envelope["error"]
            );
            let in_file: PageGeometrySnapshot = serde_json::from_value(envelope["result"].clone())
                .unwrap_or_else(|e| {
                    panic!(
                        "{} page {}: the result is not a snapshot: {e}",
                        case.case_name, page_vector.index
                    )
                });
            let number = PageObjectNumber::new(page_vector.page_object_number)
                .expect("the corpus never carries a direct page object");
            let visible = box_of(number).expect("the page's visible box");
            let in_page = serde_json::to_value(page_geometry_of(in_file, visible))
                .expect("a snapshot serializes");
            let expected = corpus::quantized(&page_vector.snapshot);
            let actual = corpus::quantized(&in_page);
            if let Some(difference) = corpus::first_difference(&expected, &actual) {
                failures
                    .push(format!("{} page {}: {difference}", case.case_name, page_vector.index));
            }
            compared += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "{} page(s) diverged:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
    assert!(compared > 0, "the corpus compared no pages");
}

/// The export reads a borrowed page without taking it over.
///
/// Stage A's rule is "retains nothing, closes nothing". If the export closed
/// the page or took ownership, the second call would read freed memory or
/// return an error; if it retained one, the host's own read afterwards would
/// see a changed page. Calling three times and then using the page from the
/// owner's side is the cheapest way to say that out loud.
#[test]
fn borrowing_the_same_page_repeatedly_leaves_it_to_its_owner() {
    let library = Library::acquire();
    let bytes = test_support::fixture(test_support::REPORT);
    let document = Document::from_bytes(library, &bytes, None).expect("open");
    let page = document.page(0).expect("page 0");

    let first = call_geometry(unsafe { page.as_raw() }.cast());
    let second = call_geometry(unsafe { page.as_raw() }.cast());
    let third = call_geometry(unsafe { page.as_raw() }.cast());
    assert_eq!(first, second);
    assert_eq!(second, third);

    // The owner's page still works, which it would not if the export had
    // closed it.
    assert!(page.width() > 0.0);
    assert!(page.text().expect("text page").char_count() > 0);
}
