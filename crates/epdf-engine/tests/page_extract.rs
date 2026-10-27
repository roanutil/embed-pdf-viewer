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
    let bytes =
        extract_pages(&mut session, &all[..1], None, &CancelToken::never()).expect("extract");

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

    let bytes = extract_pages(&mut session, &all, None, &CancelToken::never()).expect("extract");
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
    let bytes = extract_pages(&mut session, &picked, None, &CancelToken::never()).expect("extract");

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
    let refused = extract_pages(&mut session, &[], None, &CancelToken::never()).expect_err("empty");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
}

#[test]
fn a_duplicate_page_is_refused_rather_than_deduplicated() {
    // Collapsing it would return a document with fewer pages than were asked
    // for, which is a worse answer than an error.
    let mut session = session(report());
    let all = numbers(&mut session);
    let refused = extract_pages(&mut session, &[all[0], all[0]], None, &CancelToken::never())
        .expect_err("duplicate");
    assert_eq!(refused.code, EngineErrorCode::InvalidArg);
    assert!(refused.message.contains("duplicate"), "{}", refused.message);
}

#[test]
fn a_page_the_document_does_not_have_is_not_found() {
    let mut session = session(report());
    let missing = PageObjectNumber::new(99_999).expect("number");
    let refused =
        extract_pages(&mut session, &[missing], None, &CancelToken::never()).expect_err("missing");
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

    extract_pages(&mut session, &all[..2], None, &CancelToken::never()).expect("extract");

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
        extract_pages(&mut session, &all[..1], None, &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}

#[test]
fn extracting_from_a_closed_session_is_refused() {
    let mut session = session(report());
    let all = numbers(&mut session);
    session.close();
    assert_eq!(
        extract_pages(&mut session, &all[..1], None, &CancelToken::never())
            .expect_err("closed")
            .code,
        EngineErrorCode::DocNotOpen,
    );
}

// `pages.extract` through the host, as `PagesExtractor` answers it.

/// Extract `pages` of `fixture` through `Host::call`, with `flatten` when
/// given, and reopen the bytes as their own document.
fn extract_through_host(
    fixture: &str,
    flatten: Option<serde_json::Value>,
) -> epdf_engine::host::Host {
    use serde_json::json;
    let mut host = epdf_engine::host::Host::new();
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "src" }).to_string(),
        vec![std::fs::read(test_support::workspace_root().join(fixture)).expect("fixture")],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let listed: serde_json::Value = serde_json::from_str(
        &host
            .call_filled(
                &json!({ "kind": "pages.list", "jobId": 2, "docId": "src" }).to_string(),
                Vec::new(),
            )
            .json,
    )
    .unwrap();
    let first = listed["result"]["pages"][0]["ref"].clone();
    let mut request =
        json!({ "kind": "pages.extract", "jobId": 3, "docId": "src", "pages": [first] });
    if let Some(flatten) = flatten {
        request["flatten"] = flatten;
    }
    let extracted = host.call_filled(&request.to_string(), Vec::new());
    assert!(!extracted.json.contains("\"error\""), "{}", extracted.json);
    let bytes = extracted.payloads.into_iter().next().expect("the extracted bytes");
    let opened = host.call_filled(
        &json!({ "kind": "open.fatMem", "jobId": 4, "docId": "out" }).to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    host
}

fn annotation_count(host: &mut epdf_engine::host::Host) -> usize {
    let listed: serde_json::Value = serde_json::from_str(
        &host
            .call_filled(
                &serde_json::json!({ "kind": "annotations.list", "jobId": 5, "docId": "out" })
                    .to_string(),
                Vec::new(),
            )
            .json,
    )
    .unwrap();
    listed["result"]["annotations"].as_array().map_or(0, Vec::len)
}

#[test]
fn a_flattened_extract_paints_the_annotations_in_and_keeps_none() {
    const APPEARANCES: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
    let mut plain = extract_through_host(APPEARANCES, None);
    assert!(annotation_count(&mut plain) > 0, "a plain copy keeps its annotations");
    let mut flat = extract_through_host(
        APPEARANCES,
        Some(serde_json::json!({ "usage": "print", "includeAnnotations": true,
            "includeFormFields": false })),
    );
    assert_eq!(annotation_count(&mut flat), 0, "nothing interactive is left");
}

#[test]
fn an_extract_keeps_the_documents_title() {
    let mut host =
        extract_through_host("tooling/engine-conformance/fixtures/rich_metadata.pdf", None);
    let read = |host: &mut epdf_engine::host::Host, doc: &str| -> serde_json::Value {
        let answer: serde_json::Value = serde_json::from_str(
            &host
                .call_filled(
                    &serde_json::json!({ "kind": "metadata.read", "jobId": 6, "docId": doc })
                        .to_string(),
                    Vec::new(),
                )
                .json,
        )
        .unwrap();
        answer["result"]["metadata"]["title"].clone()
    };
    let title = read(&mut host, "src");
    assert!(title.is_string(), "the fixture has a title: {title}");
    assert_eq!(read(&mut host, "out"), title);
}
