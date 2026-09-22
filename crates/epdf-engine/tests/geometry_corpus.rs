//! G2's differential gate: every committed `pages.geometry` vector, read again
//! through the Rust reader, glyph for glyph.
//!
//! The vectors under `tooling/engine-conformance/vectors/pages.geometry/` were
//! produced by the shipping TypeScript `PageGeometryReader`. This test is the
//! glyph-level differential harness the port order asks G2 to gate on; the
//! property suite in `geometry.rs` cannot supply it, because a reader that is
//! wrong the same way in every run still satisfies round-trip, tiling and
//! parallelism.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run vectors
//!
//! Both sides compare quantized units rather than floats: the oracle reads
//! PDFium compiled to WASM and this reads the same fork compiled natively, and
//! the corpus is not the place to litigate the last bit of an f32. See
//! `test_support::corpus`.

use epdf_engine::access::{DocumentAccess, OwnedDocument};
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::geometry::read_page_geometry;
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};
use test_support::corpus;

const OPERATION: &str = "pages.geometry";

fn open(case: &corpus::VectorFile) -> OwnedDocument {
    let library = Library::acquire();
    let bytes = test_support::fixture(&case.fixture);
    let document = Document::from_bytes(library, &bytes, case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open {}: {e}", case.case_name, case.fixture));
    OwnedDocument::new(document).unwrap_or_else(|e| panic!("{}: index pages: {e}", case.case_name))
}

/// Every glyph of every corpus page matches the TypeScript reader's snapshot.
///
/// One test rather than one per case: a `#[test]` per file would need the
/// corpus to be known at compile time, and a corpus that only grows when
/// somebody remembers to add a test function is not a corpus. Failures name
/// the case, the page and the first field that diverged, so a single red test
/// still points at one line.
#[test]
fn every_corpus_page_matches_the_typescript_reader() {
    let cases = corpus::load(OPERATION);
    let mut compared_pages = 0usize;
    let mut compared_glyphs = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case in &cases {
        let document = open(case);
        for page_vector in &case.pages {
            let expected = corpus::quantized(&page_vector.snapshot);

            // Page identity first. If the two implementations disagree about
            // which object number sits at an index, comparing their geometry
            // compares two different pages and passes for the wrong reason.
            let at_index = match document.document().page(page_vector.index) {
                Ok(page) => page.object_number(),
                Err(e) => {
                    failures.push(format!(
                        "{} page {}: load by index: {e}",
                        case.case_name, page_vector.index
                    ));
                    continue;
                }
            };
            if at_index != page_vector.page_object_number {
                failures.push(format!(
                    "{} page {}: TypeScript reports object number {}, Rust reports {at_index}",
                    case.case_name, page_vector.index, page_vector.page_object_number
                ));
                continue;
            }

            let number = PageObjectNumber::new(page_vector.page_object_number)
                .expect("the corpus never carries a direct page object");
            let page = match document.page(number) {
                Ok(page) => page,
                Err(e) => {
                    failures.push(format!(
                        "{} page {}: page by object number: {e}",
                        case.case_name, page_vector.index
                    ));
                    continue;
                }
            };

            let snapshot = match read_page_geometry(&page, &CancelToken::never()) {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    failures.push(format!(
                        "{} page {}: read_page_geometry: {e}",
                        case.case_name, page_vector.index
                    ));
                    continue;
                }
            };
            compared_glyphs += snapshot.runs.iter().map(|run| run.glyph_count()).sum::<usize>();

            let actual =
                corpus::quantized(&serde_json::to_value(&snapshot).expect("snapshot serializes"));
            if let Some(difference) = corpus::first_difference(&expected, &actual) {
                failures.push(format!(
                    "{} page {} ({}): {difference}",
                    case.case_name, page_vector.index, case.fixture
                ));
            }
            compared_pages += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {compared_pages} corpus page(s) diverged from the TypeScript reader:\n  {}",
        failures.len(),
        failures.join("\n  "),
    );
    // A corpus that silently stops loading is a suite that passes by reading
    // nothing, which is the failure mode this gate exists to prevent.
    assert!(compared_pages >= cases.len(), "every case must contribute at least one page");
    assert!(compared_glyphs > 0, "the corpus compared no glyphs at all");
}

/// The corpus covers both run variants and the degenerate-glyph convention.
///
/// Without this, deleting every rotated fixture would leave the gate green.
#[test]
fn the_corpus_exercises_both_run_variants_and_empty_glyphs() {
    let mut upright = 0usize;
    let mut rotated = 0usize;
    let mut empty_glyphs = 0usize;
    let mut ascent_flipped = 0usize;

    for case in corpus::load(OPERATION) {
        for page in case.pages {
            for run in page.snapshot["runs"].as_array().expect("runs is an array") {
                if run.get("rotation").is_some() {
                    rotated += 1;
                    if run["ascentFlip"] == serde_json::Value::Bool(true) {
                        ascent_flipped += 1;
                    }
                } else {
                    upright += 1;
                }
                for glyph in run["glyphs"].as_array().expect("glyphs is an array") {
                    // Bit 2 is the empty flag; see GLYPH_FLAG_EMPTY.
                    if glyph["flags"].as_u64().expect("flags is a number") & 2 != 0 {
                        empty_glyphs += 1;
                    }
                }
            }
        }
    }

    assert!(upright > 0, "no upright runs in the corpus");
    assert!(rotated > 0, "no rotated runs in the corpus");
    assert!(ascent_flipped > 0, "no mirrored content in the corpus");
    assert!(empty_glyphs > 0, "no degenerate glyphs in the corpus");
}
