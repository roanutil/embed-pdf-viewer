//! G5: the page text reader agrees with the TypeScript engine.
//!
//! The fixture half of the entry gate. `charmap.rs` pins the translation on
//! hand-built snapshots, where a case can be exactly one deviation; this pins
//! the extraction on real pages, where the deviations are whatever the
//! document happens to contain.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run vectors

use epdf_engine::access::{DocumentAccess, OwnedDocument};
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::text::charmap::{
    CharMapAnchor, PageTextSnapshot, char_map_violation, slice_text_by_chars,
};
use epdf_engine::features::text::reader::read_page_text;
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};
use serde::Deserialize;

/// The snapshot as the wire carries it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WireSnapshot {
    text: String,
    char_count: u32,
    #[serde(default)]
    char_map: Vec<(u32, u32)>,
}

impl WireSnapshot {
    fn to_snapshot(&self) -> PageTextSnapshot {
        PageTextSnapshot {
            text: self.text.clone(),
            char_count: self.char_count,
            char_map: self.char_map.iter().map(|(c, t)| CharMapAnchor::new(*c, *t)).collect(),
        }
    }
}

fn document(case: &test_support::corpus::VectorFile) -> OwnedDocument {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.case_name, case.fixture));
    let document = Document::from_bytes(Library::acquire(), &bytes, case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.case_name));
    OwnedDocument::new(document).unwrap_or_else(|e| panic!("{}: index: {e}", case.case_name))
}

#[test]
fn page_text_agrees_with_the_typescript_engine() {
    let cases = test_support::corpus::load("pages.text");
    assert!(!cases.is_empty(), "no pages.text vectors");

    let mut pages = 0usize;
    for case in &cases {
        let document = document(case);
        for page_vector in &case.pages {
            let number = PageObjectNumber::new(page_vector.page_object_number)
                .unwrap_or_else(|e| panic!("{}: object number: {e}", case.case_name));
            let page = document
                .page(number)
                .unwrap_or_else(|e| panic!("{}: load page: {e}", case.case_name));
            let actual = read_page_text(&page, &CancelToken::never())
                .unwrap_or_else(|e| panic!("{}: read text: {e}", case.case_name));
            let expected: WireSnapshot = serde_json::from_value(page_vector.snapshot.clone())
                .unwrap_or_else(|e| panic!("{}: decode vector: {e}", case.case_name));

            let where_ = format!("{} page {}", case.case_name, page_vector.index);
            assert_eq!(actual.char_count, expected.char_count, "{where_}: charCount");
            assert_eq!(
                actual.text_len_utf16(),
                expected.text.chars().map(char::len_utf16).sum::<usize>() as u32,
                "{where_}: text length in UTF-16 units",
            );
            assert_eq!(actual.text, expected.text, "{where_}: text");
            assert_eq!(actual.char_map, expected.to_snapshot().char_map, "{where_}: char map",);
            pages += 1;
        }
    }
    assert!(pages >= 4, "only {pages} pages compared");
}

#[test]
fn every_extracted_snapshot_satisfies_the_map_invariants() {
    // The reader cross-checks before returning, so this can only fail if the
    // cross-check itself is wrong. Worth having for that reason: it is the
    // check on the check.
    let cases = test_support::corpus::load("pages.text");
    for case in &cases {
        let document = document(case);
        for page_vector in &case.pages {
            let number = PageObjectNumber::new(page_vector.page_object_number).expect("number");
            let page = document.page(number).expect("page");
            let snapshot = read_page_text(&page, &CancelToken::never()).expect("read");
            assert_eq!(
                char_map_violation(
                    snapshot.char_count,
                    snapshot.text_len_utf16(),
                    &snapshot.char_map,
                ),
                None,
                "{} page {}",
                case.case_name,
                page_vector.index,
            );
        }
    }
}

#[test]
fn slicing_an_extracted_page_by_its_whole_range_returns_its_text() {
    // The identity every selection rests on, checked against real pages
    // rather than hand-built ones.
    let cases = test_support::corpus::load("pages.text");
    for case in &cases {
        let document = document(case);
        for page_vector in &case.pages {
            let number = PageObjectNumber::new(page_vector.page_object_number).expect("number");
            let page = document.page(number).expect("page");
            let snapshot = read_page_text(&page, &CancelToken::never()).expect("read");
            assert_eq!(
                slice_text_by_chars(&snapshot, 0, snapshot.char_count),
                snapshot.text,
                "{} page {}",
                case.case_name,
                page_vector.index,
            );
        }
    }
}

#[test]
fn a_cancelled_text_read_reports_aborted() {
    use epdf_engine::error::EngineErrorCode;

    let cases = test_support::corpus::load("pages.text");
    let case = &cases[0];
    let document = document(case);
    let number = PageObjectNumber::new(case.pages[0].page_object_number).expect("number");
    let page = document.page(number).expect("page");

    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        read_page_text(&page, &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}
