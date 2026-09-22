use epdf_engine::access::{BorrowedPage, DocumentAccess, OwnedDocument, PageAccess};
use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::geometry::{PageGeometrySnapshot, read_page_geometry};
use epdf_engine::identity::PageObjectNumber;
use pdfium::{Document, Library};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{process, vec};

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

/*
* pnpm exec tsx --conditions=development \
    tooling/engine-parity/read-geometry.ts \
    packages/engine/main/test/fixtures/rotated_text.pdf 0
*/

#[test]
fn reads_geometry_for_an_upright_page() {
    let repo_root = test_support::workspace_root();
    let adapter = repo_root.join("tooling/engine-parity/read-geometry.ts");
    let fixture = repo_root.join("package/engine/main/test/fixtures/rotated_text.pdf");
    let typescript_output_data = process::Command::new("pnpm")
        .args([
            "exec",
            "tsx",
            "--conditions=development",
            adapter.to_str().unwrap(),
            fixture.to_str().unwrap(),
            "0",
        ])
        .output()
        .expect("typescript stdout")
        .stdout;

    let typescript_snapshot: serde_json::Value =
        serde_json::from_slice(&typescript_output_data).expect("typescript snapshot json");

    let doc = open(test_support::REPORT);
    let page = doc
        .page(first_page_number(&doc))
        .expect("page by object number");
    let char_count = page.view().text().expect("text page").char_count();

    let rust_snapshot = read_page_geometry(&page, &CancelToken::never()).expect("read geometry");
    let rust_snapshot_value = serde_json::to_value(rust_snapshot).expect("rust snapshot json");
}
