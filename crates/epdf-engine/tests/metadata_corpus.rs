//! G6: the metadata read agrees with the TypeScript engine.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run metadata
//!
//! Each case reads through the host the way the generator did:
//! `metadata.read`, with the keys of `metadata.readCustom` folded in under
//! `custom` (our 1b8c92a3f; upstream reads them apart since a4b8259f7).
//!
//! The distinction the corpus is built around is three-state: a key that is
//! absent, one that is present and empty, and one with a value. A reader that
//! collapsed the first two would look correct on every other document in the
//! repository, which is why `rich_metadata.pdf` exists.

use std::collections::BTreeMap;

use epdf_engine::cancel::CancelToken;
use epdf_engine::features::metadata::{read_custom_metadata, read_metadata};
use epdf_engine::host::Host;
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
    metadata: WireMetadata,
}

/// The snapshot as the wire carries it.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct WireMetadata {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    keywords: Option<String>,
    producer: Option<String>,
    creator: Option<String>,
    created_at: Option<String>,
    modified_at: Option<String>,
    trapped: String,
    custom: BTreeMap<String, String>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("metadata").join("read.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("read.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "metadata.read");
    file
}

fn open(case: &Case) -> Document {
    let bytes = std::fs::read(test_support::workspace_root().join(&case.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, case.fixture));
    Document::from_bytes(Library::acquire(), &bytes, case.password.as_deref())
        .unwrap_or_else(|e| panic!("{}: open: {e}", case.name))
}

/// The case's metadata as the host answers it, the custom keys folded in.
fn read_through_host(case: &Case) -> serde_json::Value {
    let mut host = Host::new();
    let call = |host: &mut Host, request: serde_json::Value, payloads: Vec<Vec<u8>>| {
        let response = host.call_filled(&request.to_string(), payloads);
        let mut envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
        assert!(envelope.get("error").is_none(), "{}: {request} failed: {envelope}", case.name);
        envelope["result"].take()
    };
    call(
        &mut host,
        serde_json::json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "d",
            "password": case.password }),
        vec![test_support::fixture(&case.fixture)],
    );
    // `{ metadata }` and `{ custom }`.
    let mut metadata = call(
        &mut host,
        serde_json::json!({ "kind": "metadata.read", "effect": "read", "jobId": 2, "docId": "d" }),
        Vec::new(),
    )["metadata"]
        .take();
    metadata["custom"] = call(
        &mut host,
        serde_json::json!({ "kind": "metadata.readCustom", "effect": "read", "jobId": 3,
            "docId": "d" }),
        Vec::new(),
    )["custom"]
        .take();
    metadata
}

#[test]
fn metadata_agrees_with_the_typescript_engine() {
    let file = load();
    assert!(file.cases.len() >= 4, "the corpus shrank");

    for case in &file.cases {
        let actual: WireMetadata = serde_json::from_value(read_through_host(case))
            .unwrap_or_else(|e| panic!("{}: the answer is not the wire's shape: {e}", case.name));
        assert_eq!(actual, case.metadata, "{}: {}", case.name, case.covers);
    }
}

#[test]
fn an_absent_key_and_an_empty_one_are_different_answers() {
    // Stated separately from the comparison above, because it is the
    // distinction a patch operates on and the one a reader is most likely to
    // collapse. `rich_metadata.pdf` has /Subject () and no /Keywords at all.
    let file = load();
    let case =
        file.cases.iter().find(|c| c.name == "rich").expect("the rich fixture is in the corpus");

    let document = open(case);
    let metadata = read_metadata(&document, &CancelToken::never()).expect("read");

    assert_eq!(metadata.subject, Some(String::new()), "/Subject () is present and empty");
    assert_eq!(metadata.keywords, None, "no /Keywords at all");
}

#[test]
fn a_utf16_title_survives_the_round_trip() {
    // The value ABI is UTF-16 and the key ABI is UTF-8, which is the kind of
    // asymmetry that produces mojibake on exactly one of the two.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rich").expect("rich");
    let document = open(case);
    let metadata = read_metadata(&document, &CancelToken::never()).expect("read");

    let title = metadata.title.expect("a title");
    assert!(title.contains('ø'), "{title}");
    assert!(title.contains('中'), "{title}");
    assert!(title.contains('\u{1F600}'), "an astral character survived: {title}");
}

#[test]
fn custom_keys_exclude_the_reserved_ones() {
    // Otherwise /Title would be reported twice, once as a field and once as
    // custom data.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rich").expect("rich");
    let document = open(case);
    let metadata = read_metadata(&document, &CancelToken::never()).expect("read");

    let custom = read_custom_metadata(&document, &CancelToken::never()).expect("read custom");
    assert!(metadata.title.is_some());
    for reserved in ["Title", "Author", "Subject", "Producer", "Creator", "CreationDate", "ModDate"]
    {
        assert!(!custom.contains_key(reserved), "/{reserved} was reported as custom data",);
    }
    assert_eq!(custom.get("EmbedPDFCustom").map(String::as_str), Some("a custom value"));
    // A custom key that is present and empty is carried, not dropped: the map
    // is of strings, and "" is a string.
    assert_eq!(custom.get("EmbedPDFEmpty").map(String::as_str), Some(""));
}

#[test]
fn both_pdf_date_forms_convert() {
    // An offset date and a Z date, which are the two shapes PDF permits and
    // the two a naive parser gets wrong in different ways.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "rich").expect("rich");
    let document = open(case);
    let metadata = read_metadata(&document, &CancelToken::never()).expect("read");

    // Each keeps the offset the file wrote (fd175ba50, `pdfDateToIso`).
    assert_eq!(metadata.created_at.as_deref(), Some("2024-01-15T09:30:00+01:00"));
    assert_eq!(metadata.modified_at.as_deref(), Some("2024-02-20T12:00:00Z"));
}

#[test]
fn a_cancelled_metadata_read_reports_aborted() {
    use epdf_engine::error::EngineErrorCode;

    let file = load();
    let document = open(&file.cases[0]);
    let cancel = CancelToken::never();
    cancel.cancel();
    assert_eq!(
        read_metadata(&document, &cancel).expect_err("cancelled").code,
        EngineErrorCode::Aborted,
    );
}
