//! G9's gate: an extracted document is the one that was asked for.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run extract
//!
//! # Not the bytes, and that is the port order's own position
//!
//! `multiplatform-port-order.md` G10 says the conformance suites do not
//! establish byte-stable PDF serialization. Two runs of one serializer can
//! differ in object numbering and in the file identifier, so comparing the
//! buffers would compare PDFium's writer to itself with a coin toss attached.
//!
//! What is compared is what an extract is for. Reopen the result and ask it
//! how many pages it has, how big each one is, which way each is rotated, and
//! what text is on it. A buffer that answers all four the way the TypeScript's
//! did is the document that was asked for, whatever its object numbers say.

use epdf_engine::access::{DocumentAccess, OwnedDocument};
use epdf_engine::cancel::CancelToken;
use epdf_engine::features::extract::extract_pages;
use epdf_engine::features::pages::list_pages;
use epdf_engine::features::text::reader::read_page_text;
use epdf_engine::identity::{PageObjectNumber, PageRef};
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
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    fixture: String,
    covers: String,
    /// Zero-based source page indices.
    source_pages: Vec<u32>,
    /// The pages those indices resolved to, as the request named them,
    /// recorded so this runner compares its own resolution rather than
    /// repeating it blind.
    sources: Vec<PageRef>,
    page_count: u32,
    /// The reopened document, page by page.
    pages: Vec<ExtractedPage>,
}

#[derive(Debug, Deserialize, PartialEq)]
struct ExtractedPage {
    width: f64,
    height: f64,
    rotation: u16,
    text: String,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("pages").join("extract.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("extract.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.extract");
    file
}

fn session(case: &Case) -> DocumentSession {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    let mut session = DocumentSession::new();
    session
        .open_with(bytes, None, SessionAuthority::unrestricted())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name));
    session
}

/// The object numbers a case's source indices name.
fn source_object_numbers(session: &mut DocumentSession, case: &Case) -> Vec<PageObjectNumber> {
    let records = session.all_records().expect("records");
    case.source_pages
        .iter()
        .map(|index| {
            records
                .get(*index as usize)
                .unwrap_or_else(|| panic!("{}: no source page {index}", case.name))
                .page_object_number
        })
        .collect()
}

/// Reopen extracted bytes and describe every page.
///
/// Through `OwnedDocument` rather than a session, because a session is for
/// holding a document open across calls and this one exists for the length of
/// one description.
fn describe(bytes: &[u8]) -> Vec<ExtractedPage> {
    let document = OwnedDocument::new(
        Document::from_bytes(Library::acquire(), bytes, None)
            .expect("the extracted bytes reopen as a document"),
    )
    .expect("index");
    let listing = list_pages(document.document(), &CancelToken::never()).expect("list");

    listing
        .pages
        .iter()
        .map(|layout| {
            let page = document.page(layout.page_object_number).expect("page");
            let text = read_page_text(&page, &CancelToken::never()).expect("text");
            ExtractedPage {
                width: layout.size.width,
                height: layout.size.height,
                rotation: layout.rotation.degrees(),
                text: text.text,
            }
        })
        .collect()
}

#[test]
fn an_extract_reopens_as_the_document_the_typescript_engine_produced() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");

    for case in &file.cases {
        let mut source = session(case);
        let numbers = source_object_numbers(&mut source, case);
        assert_eq!(
            numbers.iter().copied().map(PageRef::new).collect::<Vec<_>>(),
            case.sources,
            "{}: the source indices resolve to different pages",
            case.name,
        );

        // The pages the TypeScript engine was sent, as it sent them.
        let sent: Vec<PageObjectNumber> =
            case.sources.iter().map(|page| page.object_number()).collect();
        let bytes = extract_pages(&mut source, &sent, None, &CancelToken::never())
            .unwrap_or_else(|e| panic!("{}: extract: {e}", case.name));
        let mine = describe(&bytes);

        assert_eq!(
            mine.len() as u32,
            case.page_count,
            "{}: a different page count: {}",
            case.name,
            case.covers,
        );
        for (index, (mine, theirs)) in mine.iter().zip(&case.pages).enumerate() {
            assert_eq!(mine, theirs, "{}: page {index}: {}", case.name, case.covers);
        }
    }
}

#[test]
fn the_callers_order_reaches_the_output() {
    // The one property a page-count comparison cannot see. Extracting 3, 0
    // and 1 has to produce them in that order, and a reader that sorted the
    // selection would produce a document with the same three pages in the
    // wrong places.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "reordered").expect("the reordered case");
    assert_eq!(case.source_pages, vec![3, 0, 1], "the case stopped being out of order");

    let mut source = session(case);
    let numbers = source_object_numbers(&mut source, case);
    let bytes = extract_pages(&mut source, &numbers, None, &CancelToken::never()).expect("extract");
    let mine = describe(&bytes);

    // Text is what identifies a page here: the three are the same size and
    // the same rotation, so nothing else tells them apart.
    let texts: Vec<&str> = mine.iter().map(|p| p.text.as_str()).collect();
    let theirs: Vec<&str> = case.pages.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, theirs);

    // And it is genuinely not document order: sorting the selection would
    // have produced a different first page.
    let mut sorted = numbers.clone();
    sorted.sort_by_key(|n| n.get());
    assert_ne!(sorted, numbers, "the case no longer distinguishes the two orders");
}

#[test]
fn rotation_survives_the_copy() {
    // `/Rotate` is presentation metadata on the page dictionary, so an import
    // that rebuilt pages rather than copying them would silently flatten it.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rotated").expect("the rotated case");

    let mut source = session(case);
    let numbers = source_object_numbers(&mut source, case);
    let bytes = extract_pages(&mut source, &numbers, None, &CancelToken::never()).expect("extract");
    let mine = describe(&bytes);

    assert_eq!(
        mine.iter().map(|p| p.rotation).collect::<Vec<_>>(),
        vec![0, 90, 180, 270],
        "the four rotations came through in order",
    );
}
