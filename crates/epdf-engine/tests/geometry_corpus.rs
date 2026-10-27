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
//! The vectors are page space, as the TypeScript engine's API answers, so
//! each page is read through [`Host::call`]: the reader answers the file's
//! coordinates and the host boundary turns them into page space
//! (`pageGeometryOf`), which is the path the wire takes.
//!
//! Both sides compare quantized units rather than floats: the oracle reads
//! PDFium compiled to WASM and this reads the same fork compiled natively, and
//! the corpus is not the place to litigate the last bit of an f32. See
//! `test_support::corpus`.

use epdf_engine::host::Host;
use pdfium::{Document, Library};
use serde_json::{Value, json};
use test_support::corpus;

const OPERATION: &str = "pages.geometry";
const DOC_ID: &str = "corpus";

/// A host with the case's fixture open as [`DOC_ID`].
fn open(case: &corpus::VectorFile) -> Host {
    let mut host = Host::new();
    let response = host.call_filled(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": DOC_ID,
            "password": case.password })
        .to_string(),
        vec![test_support::fixture(&case.fixture)],
    );
    let envelope: Value = serde_json::from_str(&response.json).expect("the open answers JSON");
    assert!(
        envelope.get("error").is_none(),
        "{}: open {}: {envelope}",
        case.case_name,
        case.fixture
    );
    host
}

/// One page's geometry as the wire answers it: its `result` or its `error`.
fn geometry(host: &mut Host, job_id: u64, page_object_number: u32) -> Result<Value, Value> {
    let request = json!({
        "kind": "pages.geometry",
        "effect": "read",
        "jobId": job_id,
        "docId": DOC_ID,
        "page": { "kind": "objectNumber", "objectNumber": page_object_number },
    });
    let mut envelope: Value =
        serde_json::from_str(&host.call_filled(&request.to_string(), Vec::new()).json)
            .expect("every response is JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope.get_mut("result").map(Value::take).unwrap_or(Value::Null)),
    }
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
        let mut host = open(case);
        let document = Document::from_bytes(
            Library::acquire(),
            &test_support::fixture(&case.fixture),
            case.password.as_deref(),
        )
        .unwrap_or_else(|e| panic!("{}: open {}: {e}", case.case_name, case.fixture));
        for (job, page_vector) in case.pages.iter().enumerate() {
            let expected = corpus::quantized(&page_vector.snapshot);

            // Page identity first. If the two implementations disagree about
            // which object number sits at an index, comparing their geometry
            // compares two different pages and passes for the wrong reason.
            let at_index = match document.page(page_vector.index) {
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

            let snapshot = match geometry(&mut host, job as u64 + 2, page_vector.page_object_number)
            {
                Ok(snapshot) => snapshot,
                Err(e) => {
                    failures.push(format!(
                        "{} page {}: pages.geometry: {e}",
                        case.case_name, page_vector.index
                    ));
                    continue;
                }
            };
            compared_glyphs += snapshot["runs"]
                .as_array()
                .map(|runs| runs.iter().map(|run| run["glyphs"].as_array().map_or(0, Vec::len)))
                .into_iter()
                .flatten()
                .sum::<usize>();

            let actual = corpus::quantized(&snapshot);
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
                    if run["ascentFlip"] == Value::Bool(true) {
                        ascent_flipped += 1;
                    }
                } else {
                    upright += 1;
                }
                for glyph in run["glyphs"].as_array().expect("glyphs is an array") {
                    // `empty` is on the wire only when it holds.
                    if glyph.get("empty") == Some(&Value::Bool(true)) {
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
