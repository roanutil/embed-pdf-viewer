//! G8's read gate: the embedded-file listing and downloads agree.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run attachments
//!
//! The bytes ARE compared here, unlike the annotation appearance vectors, and
//! they can be: an embedded file goes through PDFium's stream filters, and a
//! deflate stream has one correct decoding on every build. There is no
//! rasterizer in the path and so no reason for two engines to differ.
//!
//! What is NOT compared is the message on a failed download. The code is the
//! contract and the message is diagnosis, and holding the two implementations
//! to the same English would make every clearer error message a test failure.

use epdf_engine::cancel::CancelToken;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::files::attachments::{list_attachments, read_attachment_file};
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
    items: Vec<serde_json::Value>,
    downloads: Vec<Downloaded>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Downloaded {
    key: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    size: Option<u32>,
    #[serde(default)]
    bytes: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("files").join("attachments.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("attachments.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "files.attachments");
    file
}

fn open(case: &Case) -> Document {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    Document::from_bytes(Library::acquire(), &bytes, None)
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn the_listing_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 2, "the corpus shrank");
    let mut listed = 0usize;

    for case in &file.cases {
        let document = open(case);
        let items = list_attachments(&document, &CancelToken::never())
            .unwrap_or_else(|e| panic!("{}: list: {e}", case.name));
        assert_eq!(
            items.len(),
            case.items.len(),
            "{}: a different number of entries: {}",
            case.name,
            case.covers,
        );
        for (index, (mine, theirs)) in items.iter().zip(&case.items).enumerate() {
            assert_eq!(
                serde_json::to_value(mine).expect("serialize"),
                *theirs,
                "{}: entry {index}: {}",
                case.name,
                case.covers,
            );
        }
        listed += items.len();
    }

    assert!(listed > 0, "the corpus lists no embedded files at all");
}

#[test]
fn a_download_produces_the_same_bytes_or_fails_for_the_same_reason() {
    let file = load();
    let mut decoded = 0usize;
    let mut refused = 0usize;

    for case in &file.cases {
        let document = open(case);
        for expected in &case.downloads {
            let actual =
                read_attachment_file(&document, &expected.key, None, &CancelToken::never());
            match (&actual, &expected.error_code) {
                (Ok(content), None) => {
                    decoded += 1;
                    assert_eq!(
                        Some(content.name.clone()),
                        expected.name,
                        "{}: {}: the name",
                        case.name,
                        expected.key,
                    );
                    assert_eq!(
                        content.mime_type, expected.mime_type,
                        "{}: {}: the mime type",
                        case.name, expected.key,
                    );
                    assert_eq!(
                        Some(content.size),
                        expected.size,
                        "{}: {}: the size",
                        case.name,
                        expected.key,
                    );
                    assert_eq!(
                        Some(hex(&content.bytes)),
                        expected.bytes,
                        "{}: {}: the bytes",
                        case.name,
                        expected.key,
                    );
                }
                (Err(error), Some(code)) => {
                    refused += 1;
                    assert!(
                        error.is(expected_code(code)),
                        "{}: {}: refused with a different code: {error}",
                        case.name,
                        expected.key,
                    );
                }
                (Ok(_), Some(code)) => {
                    panic!(
                        "{}: {} decoded but TypeScript failed with {code}",
                        case.name, expected.key
                    )
                }
                (Err(error), None) => {
                    panic!(
                        "{}: {} failed but TypeScript decoded it: {error}",
                        case.name, expected.key
                    )
                }
            }
        }
    }

    // Both outcomes have to happen, or one branch is checking nothing.
    assert!(decoded > 0, "no attachment in the corpus decodes");
    assert!(refused > 0, "no attachment in the corpus refuses to decode");
}

fn expected_code(name: &str) -> EngineErrorCode {
    match name {
        "InvalidArg" => EngineErrorCode::InvalidArg,
        "NotFound" => EngineErrorCode::NotFound,
        "MalformedPdf" => EngineErrorCode::MalformedPdf,
        "Unknown" => EngineErrorCode::Unknown,
        other => panic!("the vector names an error code {other} this runner does not map"),
    }
}

#[test]
fn an_unknown_key_is_not_found_rather_than_an_empty_result() {
    // The difference between "there is no such file" and "the file is empty"
    // is the whole of what a caller does next, and the tree has a zero-byte
    // entry that would let a reader confuse them.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "tree").expect("the tree fixture");
    let document = open(case);

    let missing = read_attachment_file(&document, "no-such-key", None, &CancelToken::never())
        .expect_err("an unknown key is not found");
    assert!(missing.is(EngineErrorCode::NotFound));

    let empty = read_attachment_file(&document, "mmm-middle", None, &CancelToken::never())
        .expect("the zero-byte entry decodes");
    assert_eq!(empty.size, 0);
    assert!(empty.bytes.is_empty());
    assert_eq!(empty.name, "bare.bin");
}

#[test]
fn a_key_is_the_address_and_a_name_is_not() {
    // Two entries in the tree share one embedded stream and differ in key,
    // and one of them has a `/UF` that differs from its `/F`. Addressing by
    // name would pick the wrong one or fail.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "tree").expect("the tree fixture");
    let document = open(case);
    let items = list_attachments(&document, &CancelToken::never()).expect("list");

    let first = items.iter().find(|i| i.key == "aaa-first").expect("aaa-first");
    let last = items.iter().find(|i| i.key == "zzz-last").expect("zzz-last");
    assert_ne!(first.key, last.key);
    assert_ne!(first.file.name, last.file.name, "the two entries display differently");

    // And the same bytes come out of both, because they name one stream.
    let a = read_attachment_file(&document, &first.key, None, &CancelToken::never()).expect("a");
    let b = read_attachment_file(&document, &last.key, None, &CancelToken::never()).expect("b");
    assert_eq!(a.bytes, b.bytes);
    assert_ne!(a.name, b.name);

    // The display name is not an address: nothing resolves by it.
    assert!(
        read_attachment_file(&document, &first.file.name, None, &CancelToken::never()).is_err(),
        "a display name resolved as a key",
    );
}

#[test]
fn a_decode_budget_refuses_rather_than_truncating() {
    // The ceiling exists because a PDF can embed a stream that decompresses
    // to far more than the file it arrived in, and the decode happens inside
    // PDFium where nothing else can stop it. A truncated file would be worse
    // than no file: it would look like a file.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "tree").expect("the tree fixture");
    let document = open(case);

    let refused = read_attachment_file(&document, "aaa-first", Some(4), &CancelToken::never())
        .expect_err("a ten-byte file under a four-byte ceiling");
    assert!(refused.is(EngineErrorCode::InvalidArg));

    // At the exact size it decodes, so the ceiling is inclusive.
    let allowed = read_attachment_file(&document, "aaa-first", Some(10), &CancelToken::never())
        .expect("a ten-byte file under a ten-byte ceiling");
    assert_eq!(allowed.bytes.len(), 10);
}
