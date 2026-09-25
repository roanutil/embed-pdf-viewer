//! G13's first gate: a page list is reordered, rotated and thinned the way
//! the TypeScript engine does it.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run pages-mutate
//!
//! # What the comparison watches
//!
//! Which OBJECT NUMBER ends up at which index. An index is a position and
//! only the object number is identity, so a reorder that produced the right
//! shape with the wrong pages in it would otherwise pass.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::pages_write::{delete_pages, move_pages, rotate_pages};
use epdf_engine::geom::PdfRotation;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    #[serde(rename = "move")]
    move_: Option<MoveInput>,
    rotate: Option<RotateInput>,
    delete: Option<DeleteInput>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    layout: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MoveInput {
    pages: Vec<u32>,
    dest_index: u32,
}

#[derive(Debug, Deserialize)]
struct RotateInput {
    pages: Vec<u32>,
    rotation: u16,
}

#[derive(Debug, Deserialize)]
struct DeleteInput {
    pages: Vec<u32>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("pages").join("mutate.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("mutate.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "pages.mutate");
    file
}

fn pages(numbers: &[u32]) -> Vec<PageObjectNumber> {
    // A zero would be a malformed page rather than a bad request, and no
    // case sends one; the unwrap says so rather than hiding it.
    numbers.iter().map(|n| PageObjectNumber::new(*n).expect("a page object number")).collect()
}

/// Run one case against a fresh copy of the fixture.
fn run(
    file: &VectorFile,
    case: &Case,
) -> Result<serde_json::Value, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let cancel = CancelToken::never();

    let result = if let Some(input) = &case.move_ {
        move_pages(owned.document(), &pages(&input.pages), input.dest_index, &cancel)?
    } else if let Some(input) = &case.rotate {
        let rotation = PdfRotation::try_from(input.rotation).expect("a /Rotate value");
        rotate_pages(owned.document(), &pages(&input.pages), rotation, &cancel)?
    } else {
        let input = case.delete.as_ref().expect("an operation");
        delete_pages(owned.document(), &pages(&input.pages), &cancel)?
    };
    Ok(serde_json::to_value(&result.layout).expect("serialize"))
}

#[test]
fn a_page_mutation_leaves_the_layout_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 12, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let mine = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            test_support::canonical_numbers(&mine),
            test_support::canonical_numbers(case.layout.as_ref().expect("a recorded layout")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert!(applied >= 8, "the applied list shrank");
}

#[test]
fn a_refused_page_mutation_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let error = run(&file, case).expect_err(&case.name);
        let expected = match case.refused_with.as_deref() {
            Some("NotFound") => EngineErrorCode::NotFound,
            Some("InvalidArg") => EngineErrorCode::InvalidArg,
            other => panic!("{}: unhandled refusal code {other:?}", case.name),
        };
        assert!(error.is(expected), "{}: {} (got {error})", case.name, case.covers);
    }

    assert!(refusals >= 5, "the refusal list shrank");
}

#[test]
fn a_destination_past_the_end_says_which_end() {
    // PDFium refuses this one too, with the same code, and its refusal is
    // about the request as a whole. The message here names the frame the
    // destination is measured in, which is the thing a caller gets wrong:
    // the index space AFTER the block is detached, not before.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "move-past-the-end-refused").expect("the case");
    let message = run(&file, case).expect_err("past the end").to_string();
    assert!(
        message.contains("after the block is detached"),
        "the message does not name the frame: {message}",
    );
}

#[test]
fn a_refused_mutation_changes_nothing() {
    // The half the vector cannot show. A list naming one page that does not
    // exist must leave the others alone, and the only way to see that is to
    // ask the same document afterwards.
    let file = load();
    for name in ["rotate-unknown-page-refused", "delete-unknown-page-refused"] {
        let case = file.cases.iter().find(|c| c.name == name).expect(name);
        let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
            .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
        let mut session = DocumentSession::new();
        session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");

        let before =
            epdf_engine::features::pages::list_pages(owned.document(), &CancelToken::never())
                .expect("the pages list");

        let cancel = CancelToken::never();
        let error = if let Some(input) = &case.rotate {
            let rotation = PdfRotation::try_from(input.rotation).expect("a /Rotate value");
            rotate_pages(owned.document(), &pages(&input.pages), rotation, &cancel).unwrap_err()
        } else {
            let input = case.delete.as_ref().expect("an operation");
            delete_pages(owned.document(), &pages(&input.pages), &cancel).unwrap_err()
        };
        assert!(error.is(EngineErrorCode::NotFound), "{name}: {error}");

        let after =
            epdf_engine::features::pages::list_pages(owned.document(), &CancelToken::never())
                .expect("the pages list");
        assert_eq!(before, after, "{name}: the refusal changed the document");
    }
}
