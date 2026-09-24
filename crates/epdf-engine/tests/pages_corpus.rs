//! G6: the page listing agrees with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run metadata
//!
//! Nothing in this read loads a page, which is what keeps listing a large
//! document cheap. The gate that matters is the one the port order names:
//! page identity has to be stable between a lazy per-page lookup and a full
//! enumeration, so the object numbers this produces are compared against the
//! ones the session's registry produces by a different route.

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::pages::{PageListSnapshot, list_pages};
use epdf_engine::geom::PdfRotation;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use pdfium::{Document, Library};
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    fixture: String,
    covers: String,
    #[serde(default)]
    password: Option<String>,
    pages: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("metadata").join("pages.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("pages.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.list");
    file
}

fn bytes(case: &Case) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture))
}

fn open(case: &Case) -> Document {
    Document::from_bytes(Library::acquire(), &bytes(case), case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

fn listed(case: &Case) -> PageListSnapshot {
    list_pages(&open(case), &CancelToken::never())
        .unwrap_or_else(|e| panic!("{}: list: {e}", case.name))
}

#[test]
fn the_listing_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 5, "the corpus shrank");

    for case in &file.cases {
        // Compared as JSON rather than field by field, so a field added on one
        // side and not the other fails here rather than being skipped.
        //
        // Quantized first, because the two languages spell the same number
        // differently in JSON: Rust writes 0.0 where JavaScript writes 0, and
        // a raw comparison would report every page as a difference. The
        // quantizer is the corpus-wide one, so a coordinate is pinned to 1e-4
        // of a PDF point rather than to a float's last bit.
        let actual = test_support::corpus::quantized(
            &serde_json::to_value(listed(case)).expect("serialize"),
        );
        let expected = test_support::corpus::quantized(&case.pages);
        if let Some(difference) = test_support::corpus::first_difference(&expected, &actual) {
            panic!("{}: {difference} ({})", case.name, case.covers);
        }
    }
}

#[test]
fn no_page_in_the_corpus_carries_an_action() {
    // PageLayout.actions is not ported: it comes from ActionModelReader, which
    // is its own group. The comparison above would not notice, because a
    // field absent on both sides matches. This asserts the reason it is
    // absent, so the day a fixture gains a page action the gap becomes a
    // failure rather than a silent difference.
    let file = load();
    for case in &file.cases {
        let pages = case.pages["pages"].as_array().expect("pages");
        for page in pages {
            assert!(
                page.get("actions").is_none(),
                "{}: page {} carries an action, which the Rust listing does not read yet",
                case.name,
                page["index"],
            );
        }
    }
}

#[test]
fn rotation_is_reported_and_the_size_is_normalized() {
    // rotated_pages.pdf is four pages with identical content at 0, 90, 180 and
    // 270. The listing must report the rotation AND the unrotated size: a
    // reader using the rotated size would disagree with the page a caller
    // opens through the normalized loader.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("the rotated fixture");
    let snapshot = listed(case);

    assert_eq!(snapshot.page_count, 4);
    let rotations: Vec<PdfRotation> = snapshot.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(
        rotations,
        [
            PdfRotation::None,
            PdfRotation::Clockwise90,
            PdfRotation::Clockwise180,
            PdfRotation::Clockwise270,
        ]
    );
    for page in &snapshot.pages {
        assert_eq!(
            (page.size.width, page.size.height),
            (612.0, 792.0),
            "page {} reported a rotated size",
            page.index,
        );
    }
}

#[test]
fn listing_and_the_session_registry_agree_about_identity() {
    // The gate the port order names: page identity stable between the lazy
    // per-page lookup and the full enumeration. Two different routes to the
    // same object numbers, and they have to produce the same answer or a
    // reference minted by one is meaningless to the other.
    let file = load();
    for case in &file.cases {
        let snapshot = listed(case);

        let mut session = DocumentSession::new();
        session
            .open_with(bytes(case), case.password.as_deref(), SessionAuthority::unrestricted())
            .unwrap_or_else(|e| panic!("{}: open session: {e}", case.name));

        let records = session.all_records().expect("records").to_vec();
        assert_eq!(records.len(), snapshot.pages.len(), "{}: page counts differ", case.name);
        for (record, page) in records.iter().zip(&snapshot.pages) {
            assert_eq!(record.page_index, page.index, "{}: index", case.name);
            assert_eq!(
                record.page_object_number, page.page_object_number,
                "{}: object number at index {}",
                case.name, page.index,
            );
        }

        // And the per-page lookup route, which is the one that could diverge.
        for page in &snapshot.pages {
            let record = session
                .record_by_object_number(page.page_object_number)
                .unwrap_or_else(|e| panic!("{}: lookup: {e}", case.name));
            assert_eq!(record.page_index, page.index, "{}: lazy lookup disagreed", case.name);
        }
    }
}

#[test]
fn a_cancelled_listing_reports_aborted() {
    use epdf_engine::error::EngineErrorCode;

    let file = load();
    let document = open(&file.cases[0]);
    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        list_pages(&document, &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}
