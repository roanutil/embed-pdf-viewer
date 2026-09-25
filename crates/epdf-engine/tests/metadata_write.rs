//! G14's gate: the information dictionary says what the TypeScript engine
//! made it say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run metadata-write
//!
//! # The whole dictionary is compared, not the patched fields
//!
//! A patch touching one field has to leave the other eight alone, and the
//! only way to see that is to compare everything.

use epdf_engine::cancel::CancelToken;
use epdf_engine::dto::MetadataPatch;
use epdf_engine::features::metadata_write::update_metadata;
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
    patch: serde_json::Value,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("metadata").join("update.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("update.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "metadata.update");
    file
}

/// Apply one case's patch to a fresh copy of the fixture.
fn run(file: &VectorFile, case: &Case) -> serde_json::Value {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    let patch: MetadataPatch = serde_json::from_value(case.patch.clone())
        .unwrap_or_else(|e| panic!("{}: the patch does not decode: {e}", case.name));
    let result = update_metadata(owned.document(), &patch, &CancelToken::never())
        .unwrap_or_else(|e| panic!("{}: {e}", case.name));
    serde_json::to_value(&result.metadata).expect("serialize")
}

#[test]
fn an_updated_document_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 12, "the case list shrank");

    for case in &file.cases {
        assert_eq!(
            test_support::canonical_numbers(&run(&file, case)),
            test_support::canonical_numbers(case.metadata.as_ref().expect("recorded metadata")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn a_patch_that_says_nothing_changes_nothing() {
    // The identity case, checked against the document rather than against
    // the recorded answer: an empty patch has to leave the dictionary
    // exactly as it found it, and comparing it to a read of the untouched
    // fixture is what says so.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    let before =
        epdf_engine::features::metadata::read_metadata(owned.document(), &CancelToken::never())
            .expect("the metadata reads");
    let after = update_metadata(owned.document(), &MetadataPatch::default(), &CancelToken::never())
        .expect("an empty patch is not an error")
        .metadata;

    assert_eq!(before, after, "an empty patch changed something");
}

#[test]
fn a_clear_and_an_empty_value_are_the_same_edit() {
    // Not what the TypeScript's comment claims, and this is what the claim
    // was checked against. `EPDF_SetMetaText` removes the entry for an
    // empty string as readily as for a null, so `{ title: "" }` and
    // `{ title: null }` leave the same document: no `/Title` at all.
    //
    // Asserted against the DICTIONARY rather than the DTO, because the
    // reader normalizes a present-but-empty entry to absent anyway and
    // would agree either way. It is the fork that cannot tell them apart,
    // so a caller has no way to say "the title is deliberately blank".
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));

    let apply = |patch: serde_json::Value| -> Option<String> {
        let mut session = DocumentSession::new();
        session.open_with(bytes.clone(), None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");
        let patch: MetadataPatch = serde_json::from_value(patch).expect("the patch decodes");
        update_metadata(owned.document(), &patch, &CancelToken::never()).expect("the patch lands");
        owned.document().meta_text("Title").expect("a dictionary read")
    };

    assert_eq!(apply(serde_json::json!({ "title": null })), None, "a clear left the key behind");
    assert_eq!(apply(serde_json::json!({ "title": "" })), None, "an empty value kept the key");

    // And the same for a custom key, which takes the two states per entry
    // rather than through a patch field.
    let custom = |value: serde_json::Value| -> Option<String> {
        let mut session = DocumentSession::new();
        session.open_with(bytes.clone(), None, SessionAuthority::unrestricted()).expect("open");
        let owned = session.document().expect("open");
        let patch: MetadataPatch =
            serde_json::from_value(serde_json::json!({ "custom": { "Department": value } }))
                .expect("the patch decodes");
        update_metadata(owned.document(), &patch, &CancelToken::never()).expect("the patch lands");
        owned.document().meta_text("Department").expect("a dictionary read")
    };
    assert_eq!(custom(serde_json::Value::Null), None, "a custom clear left the key behind");
    assert_eq!(custom(serde_json::json!("")), None, "a custom empty value kept the key");
}

#[test]
fn a_date_the_wire_cannot_hold_is_refused() {
    // Refused rather than written as whatever it parses to. The wire
    // carries ISO 8601 and the file carries PDF dates, and a string that is
    // neither would otherwise become a `/CreationDate` nothing can read.
    let file = load();
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    // The patch type validates on the way in, which is the first of the two
    // gates: a string that is not ISO 8601 never becomes a patch at all.
    let decoded: Result<MetadataPatch, _> =
        serde_json::from_value(serde_json::json!({ "created": "not-a-date" }));
    assert!(decoded.is_err(), "a malformed date decoded into a patch");

    // And the document is untouched, because nothing was applied.
    let before =
        epdf_engine::features::metadata::read_metadata(owned.document(), &CancelToken::never())
            .expect("the metadata reads");
    assert!(before.created.is_some(), "the fixture still has its creation date");
}
