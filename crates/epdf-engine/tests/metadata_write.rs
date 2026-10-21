//! G14's gate: the information dictionary says what the TypeScript engine
//! made it say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run metadata-write
//!
//! Each case replays the request the generator recorded (`metadata.update`
//! or `metadata.updateCustom`) through the host, then reads the dictionary
//! back the way the generator did: `metadata.read` with the custom keys of
//! `metadata.readCustom` folded in under `custom` (our 1b8c92a3f).
//!
//! # The whole dictionary is compared, not the patched fields
//!
//! A patch touching one field has to leave the other eight alone, and the
//! only way to see that is to compare everything.

mod common;

use common::replay::Replay;
use epdf_engine::cancel::CancelToken;
use epdf_engine::dto::MetadataPatch;
use epdf_engine::features::metadata_write::update_metadata;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;
use serde_json::{Value, json};

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
    request: Value,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    metadata: Value,
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

/// The dictionary as the generator read it back: the standard fields with
/// the custom keys under `custom`.
fn read_back(replay: &mut Replay) -> Value {
    let mut metadata = replay
        .replay(&json!({ "kind": "metadata.read", "effect": "read", "jobId": "<job>" }))
        .expect("metadata.read");
    let custom = replay
        .replay(&json!({ "kind": "metadata.readCustom", "effect": "read", "jobId": "<job>" }))
        .expect("metadata.readCustom");
    metadata["custom"] = custom;
    metadata
}

#[test]
fn an_updated_document_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 15, "the case list shrank");

    for case in &file.cases {
        let doc_id = case.request["docId"].as_str().expect("a recorded docId");
        let mut replay = Replay::open(&file.fixture, doc_id);
        let answer = replay.replay(&case.request);
        match (&answer, case.refused) {
            (Err(error), true) => assert_eq!(
                error["code"].as_str(),
                case.refused_with.as_deref(),
                "{}: refused for another reason: {error}",
                case.name
            ),
            (Ok(answer), false) => {
                // The single verb's answer is the part of the dictionary it
                // writes: the standard fields, or the custom keys alone.
                let result = &answer["result"];
                if case.request["kind"] == "metadata.updateCustom" {
                    assert_eq!(result["custom"], case.metadata["custom"], "{}", case.name);
                    assert!(result.get("metadata").is_none(), "{}: {result}", case.name);
                } else {
                    let mut standard = case.metadata.clone();
                    standard.as_object_mut().unwrap().remove("custom");
                    assert_eq!(result["metadata"], standard, "{}", case.name);
                }
                assert_eq!(result["meta"]["opId"], answer["event"]["origin"]["tx"]["id"]);
                assert_eq!(result["meta"]["affectedPages"], json!([]), "{}", case.name);
                assert_eq!(result["meta"]["cacheDelta"], Value::Null, "{}", case.name);
            }
            (other, refused) => {
                panic!("{}: refused is {refused} but the answer was {other:?}", case.name)
            }
        }
        assert_eq!(
            test_support::canonical_numbers(&read_back(&mut replay)),
            test_support::canonical_numbers(&case.metadata),
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
        .expect("an empty patch is not an error");

    assert_eq!(before, after, "an empty patch changed something");
}

#[test]
fn a_clear_and_an_empty_value_are_different_edits() {
    // `null` removes the entry and `''` writes a present-but-empty one,
    // which reads back as `''` (`writeMetaText`): three states on the wire,
    // three in the file. Asserted against the dictionary as well as the
    // answer, so a reader that normalized `''` to absent would not hide a
    // write that dropped the key.
    let file = load();
    let send = |request: Value, key: &str| -> (Value, Option<String>) {
        let mut replay = Replay::open(&file.fixture, "empty-and-clear");
        let answer = replay.replay(&request).expect("the patch lands");
        let session = replay
            .host
            .sessions()
            .session(&epdf_engine::session::registry::SessionKey::document("empty-and-clear"))
            .expect("open");
        let stored = session.document().expect("open").document().meta_text(key).expect("a read");
        (answer["result"].clone(), stored)
    };

    let update = |title: Value| json!({ "kind": "metadata.update", "patch": { "title": title } });
    let (cleared, stored) = send(update(Value::Null), "Title");
    assert_eq!(cleared["metadata"]["title"], Value::Null);
    assert_eq!(stored, None, "a clear left the key behind");
    let (emptied, stored) = send(update(json!("")), "Title");
    assert_eq!(emptied["metadata"]["title"], json!(""));
    assert_eq!(stored.as_deref(), Some(""), "an empty value dropped the key");

    // And the same for a custom key, which takes the two states per entry.
    let custom =
        |value: Value| json!({ "kind": "metadata.updateCustom", "patch": { "Department": value } });
    let (cleared, stored) = send(custom(Value::Null), "Department");
    assert_eq!(cleared["custom"].get("Department"), None);
    assert_eq!(stored, None, "a custom clear left the key behind");
    let (emptied, stored) = send(custom(json!("")), "Department");
    assert_eq!(emptied["custom"]["Department"], json!(""));
    assert_eq!(stored.as_deref(), Some(""), "a custom empty value dropped the key");
}

#[test]
fn an_unwritable_custom_key_refuses_the_whole_patch() {
    // `applyCustomMetadataPatch` checks every key before it writes any, so
    // a good key beside a bad one is not written either.
    let file = load();
    let mut replay = Replay::open(&file.fixture, "bad-key");
    let before = read_back(&mut replay);
    let refused = replay
        .replay(&json!({ "kind": "metadata.updateCustom",
            "patch": { "Department": "Legal", "Title": "shadow" } }))
        .expect_err("a standard key is refused");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "Title", "{refused}");
    assert_eq!(
        refused["message"], "'Title' is a standard key; set it with metadata.update",
        "{refused}"
    );
    assert_eq!(read_back(&mut replay), before);
}

#[test]
fn a_date_the_wire_cannot_hold_is_refused() {
    // Refused rather than written as whatever it parses to. The wire
    // carries ISO 8601 and the file carries PDF dates, and a string that is
    // neither would otherwise become a `/CreationDate` nothing can read.
    let file = load();
    let mut replay = Replay::open(&file.fixture, "bad-date");
    let before = read_back(&mut replay);

    // The patch type validates on the way in: a string that is not ISO 8601
    // never becomes a patch at all.
    let decoded: Result<MetadataPatch, _> =
        serde_json::from_value(json!({ "createdAt": "not-a-date" }));
    assert!(decoded.is_err(), "a malformed date decoded into a patch");
    let refused = replay
        .replay(&json!({ "kind": "metadata.update", "patch": { "createdAt": "not-a-date" } }))
        .expect_err("a malformed date is refused");
    assert_eq!(refused["code"], "WireFormat", "{refused}");

    // And the document is untouched, because nothing was applied.
    assert_eq!(read_back(&mut replay), before);
    assert!(before["createdAt"].is_string(), "the fixture still has its creation date");
}
