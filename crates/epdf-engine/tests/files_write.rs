//! G14's other gate: piece info and attachments say what the TypeScript
//! engine made them say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run files-write
//!
//! The piece-info cases call the feature functions with the case's inputs.
//! The attachment cases became `attachments.create` and `attachments.delete`
//! ops of a `document.apply` change upstream (c2d7a5194), so each builds that
//! change from the case, sends it through `Host::call` with the bytes as a
//! payload, and lists through `attachments.list`. The vector records no
//! wire request: its generator drove the TypeScript API.
//!
//! # Two clock-stamped fields are dropped from the comparison
//!
//! An application dictionary's `/LastModified` (`modifiedAt`) and an
//! embedded file's creation date (`createdAt`) are both stamped from the
//! clock, and the two engines run at different instants. Their presence is
//! asserted separately, the same way `/M` is in the annotation suites.

mod common;

use common::replay::Replay;
use epdf_engine::cancel::CancelToken;
use epdf_engine::clock::{FixedClock, Timestamp};
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::files::piece_info::{
    PieceInfoPatch, PieceInfoPatchValue, delete_piece_info, read_piece_info,
    read_piece_info_applications, update_piece_info,
};
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use pdfium::PieceInfoHolder;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;

const PIECE_INFO_FIXTURE: &str = "tooling/engine-conformance/fixtures/piece_info.pdf";
const ATTACHMENT_FIXTURE: &str = "tooling/engine-conformance/fixtures/attachments.pdf";

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
    covers: String,
    piece_info: Option<PieceInfoInput>,
    attachment: Option<AttachmentInput>,
    #[serde(default)]
    refused: bool,
    #[serde(default)]
    refused_with: Option<String>,
    #[serde(default)]
    result: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PieceInfoInput {
    application: String,
    #[serde(default)]
    on_page: bool,
    #[serde(default)]
    patch: Option<serde_json::Value>,
    #[serde(default)]
    clear: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentInput {
    #[serde(default)]
    create: Option<CreateInput>,
    #[serde(default)]
    delete_key: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateInput {
    name: String,
    bytes: Vec<u8>,
    #[serde(default)]
    mime_type: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("files").join("write.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("write.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "files.write");
    file
}

/// Replace the two clock-stamped fields with the token the generator wrote.
fn normalize_stamps(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(normalize_stamps).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, entry)| {
                    if (key == "modifiedAt" || key == "createdAt") && entry.is_string() {
                        (key.clone(), serde_json::Value::from("<stamped>"))
                    } else {
                        (key.clone(), normalize_stamps(entry))
                    }
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Run one case against a fresh copy of its fixture. A refusal answers its
/// error code.
fn run(case: &Case) -> Result<serde_json::Value, EngineErrorCode> {
    if let Some(input) = &case.piece_info {
        return run_piece_info(case, input).map_err(|e| e.code);
    }
    run_attachment(case, case.attachment.as_ref().expect("an operation"))
}

fn run_piece_info(
    case: &Case,
    input: &PieceInfoInput,
) -> Result<serde_json::Value, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(PIECE_INFO_FIXTURE))
        .unwrap_or_else(|e| panic!("{}: read {PIECE_INFO_FIXTURE}: {e}", case.name));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let cancel = CancelToken::never();

    let holder = if input.on_page {
        PieceInfoHolder::Page(owned.document().page_object_number_by_index(0))
    } else {
        PieceInfoHolder::Catalog
    };
    if input.clear {
        delete_piece_info(owned.document(), holder, &input.application, &cancel)?;
    } else {
        let patch: PieceInfoPatch =
            serde_json::from_value(input.patch.clone().unwrap_or_else(|| serde_json::json!({})))
                .unwrap_or_else(|e| panic!("{}: the patch does not decode: {e}", case.name));
        update_piece_info(
            owned.document(),
            holder,
            &input.application,
            &patch,
            // Fixed, because `/LastModified` is dropped from the comparison
            // and a wall clock would make this test's own output depend on
            // when it ran.
            &FixedClock(Timestamp::utc(1_700_000_000_000)),
            &cancel,
        )?;
    }
    let applications = read_piece_info_applications(owned.document(), holder, &cancel)?;
    let read = read_piece_info(owned.document(), holder, &input.application, &cancel)?;
    Ok(serde_json::json!({ "applications": applications, "read": read }))
}

/// The case's write as the one op of a `document.apply` change, then the
/// listing, as the generator's `doc.attachments.create`/`delete` and
/// `doc.attachments.list()` ran them.
fn run_attachment(
    case: &Case,
    input: &AttachmentInput,
) -> Result<serde_json::Value, EngineErrorCode> {
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-write");
    let (op, payloads) = match (&input.create, &input.delete_key) {
        (Some(create), None) => {
            let mut file = serde_json::json!({ "name": create.name });
            if let Some(mime) = &create.mime_type {
                file["mimeType"] = mime.clone().into();
            }
            if let Some(description) = &create.description {
                file["description"] = description.clone().into();
            }
            let op = serde_json::json!({ "type": "attachments.create", "file": file,
                "resources": { "file": { "payload": 0 } } });
            (op, vec![create.bytes.clone()])
        }
        (None, Some(key)) => (
            serde_json::json!({ "type": "attachments.delete",
                "ref": { "kind": "key", "key": key } }),
            Vec::new(),
        ),
        _ => panic!("{}: an attachment case is one create or one delete", case.name),
    };
    let applied = replay
        .send(
            &serde_json::json!({ "kind": "document.apply", "effect": "write", "jobId": "<job>",
                "opId": "<op>", "change": { "ops": [op] } }),
            payloads,
        )
        .outcome
        .map_err(|error| error_code(&error))?;
    let listed = replay
        .replay(&serde_json::json!({ "kind": "attachments.list", "effect": "read",
            "jobId": "<job>" }))
        .unwrap_or_else(|e| panic!("{}: list: {e}", case.name));
    if input.create.is_none() {
        return Ok(serde_json::json!({ "listed": listed }));
    }
    let item = &applied["result"]["items"][0];
    assert_eq!(item["type"], "attachments.create", "{}: {applied}", case.name);
    Ok(serde_json::json!({ "created": item["attachment"], "listed": listed }))
}

fn error_code(error: &serde_json::Value) -> EngineErrorCode {
    match error["code"].as_str() {
        Some("InvalidArg") => EngineErrorCode::InvalidArg,
        Some("NotFound") => EngineErrorCode::NotFound,
        _ => panic!("a refusal this runner does not map: {error}"),
    }
}

#[test]
fn a_files_write_leaves_what_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 12, "the case list shrank");
    let mut applied = 0usize;

    for case in &file.cases {
        if case.refused {
            continue;
        }
        applied += 1;
        let mine = run(case).unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
        assert_eq!(
            test_support::canonical_numbers(&normalize_stamps(&mine)),
            test_support::canonical_numbers(case.result.as_ref().expect("a recorded result")),
            "{}: {}",
            case.name,
            case.covers,
        );
    }

    assert!(applied >= 9, "the applied list shrank");
}

#[test]
fn a_refused_files_write_is_refused_for_the_same_reason() {
    let file = load();
    let mut refusals = 0usize;

    for case in &file.cases {
        if !case.refused {
            continue;
        }
        refusals += 1;
        let error = run(case).expect_err(&case.name);
        let expected = match case.refused_with.as_deref() {
            Some("NotFound") => EngineErrorCode::NotFound,
            Some("InvalidArg") => EngineErrorCode::InvalidArg,
            other => panic!("{}: unhandled refusal code {other:?}", case.name),
        };
        assert_eq!(error, expected, "{}: {}", case.name, case.covers);
    }

    assert!(refusals >= 5, "the refusal list shrank");
}

#[test]
fn a_piece_info_write_stamps_a_real_date() {
    // Dropped from the comparison, so its shape is asserted here: a PDF
    // date, not an empty string. One per patch, however many keys it
    // touches, because §14.5 lets a consumer discard an application's data
    // when the document has changed since it wrote, and that comparison is
    // against this date.
    let bytes = std::fs::read(test_support::workspace_root().join(PIECE_INFO_FIXTURE))
        .unwrap_or_else(|e| panic!("read {PIECE_INFO_FIXTURE}: {e}"));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");

    let patch: PieceInfoPatch = serde_json::from_value(serde_json::json!({
        "one": "a",
        "two": "b",
        "three": "c",
    }))
    .expect("the patch decodes");
    update_piece_info(
        owned.document(),
        PieceInfoHolder::Catalog,
        "com.embedpdf",
        &patch,
        &FixedClock(Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect("the patch lands");

    let snapshot = read_piece_info(
        owned.document(),
        PieceInfoHolder::Catalog,
        "com.embedpdf",
        &CancelToken::never(),
    )
    .expect("the application reads")
    .expect("the application exists");
    let stamped = snapshot.modified_at.expect("a /LastModified");
    assert!(stamped.starts_with("2023-"), "the fixed clock did not reach the file: {stamped}");
}

// ── The attachment ops as changes (`attachmentChanges.ts`) ─────────────────

/// Apply `change` under `op_id`, answering the result or the error.
fn apply(
    replay: &mut Replay,
    op_id: &str,
    change: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> Result<serde_json::Value, serde_json::Value> {
    replay
        .send(
            &serde_json::json!({ "kind": "document.apply", "effect": "write", "jobId": "<job>",
                "opId": op_id, "change": change }),
            payloads,
        )
        .outcome
        .map(|answer| answer["result"].clone())
}

fn listing(replay: &mut Replay) -> Vec<serde_json::Value> {
    let listed = replay
        .replay(&serde_json::json!({ "kind": "attachments.list", "effect": "read",
            "jobId": "<job>" }))
        .expect("list");
    listed["attachments"].as_array().expect("attachments").clone()
}

fn create_op(name: &str, payload: usize) -> serde_json::Value {
    serde_json::json!({ "type": "attachments.create", "file": { "name": name },
        "resources": { "file": { "payload": payload } } })
}

fn delete_op(key: &str) -> serde_json::Value {
    serde_json::json!({ "type": "attachments.delete", "ref": { "kind": "key", "key": key } })
}

fn keys(listed: &[serde_json::Value]) -> Vec<&str> {
    listed.iter().map(|entry| entry["ref"]["key"].as_str().expect("a key")).collect()
}

#[test]
fn an_undone_create_deletes_the_file_and_its_undo_brings_it_back() {
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-undo");
    let before = listing(&mut replay);
    let created = apply(
        &mut replay,
        "create",
        serde_json::json!({ "ops": [create_op("added.txt", 0)] }),
        vec![b"hello".to_vec()],
    )
    .expect("the create applies");
    assert_eq!(created["meta"]["undoable"], true, "{created}");

    let undone = apply(&mut replay, "undo", serde_json::json!({ "undoOf": "create" }), Vec::new())
        .expect("the undo applies");
    assert_eq!(undone["items"][0]["type"], "attachments.delete", "{undone}");
    assert_eq!(listing(&mut replay), before, "the undo left the file");

    // The undo of the undo is a redo: the file a delete removed comes back.
    let redone = apply(&mut replay, "redo", serde_json::json!({ "undoOf": "undo" }), Vec::new())
        .expect("the redo applies");
    let item = &redone["items"][0];
    assert_eq!(item["type"], "attachments.restore", "{redone}");
    assert_eq!(item["attachment"]["ref"], serde_json::json!({ "kind": "key", "key": "added.txt" }));
    assert_eq!(item["attachment"]["size"], 5, "{redone}");
    assert!(keys(&listing(&mut replay)).contains(&"added.txt"));
}

#[test]
fn an_undone_delete_brings_back_the_file_as_it_read() {
    // `aaa-first`'s display name is not its key, and its checksum is the
    // fixture's own rather than one PDFium would compute: both come back.
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-undo");
    let before = listing(&mut replay);
    let bytes_before = replay
        .send(
            &serde_json::json!({ "kind": "attachments.readFile", "effect": "read",
                "jobId": "<job>", "ref": { "kind": "key", "key": "aaa-first" } }),
            Vec::new(),
        )
        .payloads;
    assert_eq!(bytes_before, [b"the notes\n".to_vec()], "the file read before the delete");
    apply(
        &mut replay,
        "delete",
        serde_json::json!({ "ops": [delete_op("aaa-first")] }),
        Vec::new(),
    )
    .expect("the delete applies");
    assert!(!keys(&listing(&mut replay)).contains(&"aaa-first"));

    let undone = apply(&mut replay, "undo", serde_json::json!({ "undoOf": "delete" }), Vec::new())
        .expect("the undo applies");
    assert_eq!(undone["items"][0]["type"], "attachments.restore", "{undone}");
    assert_eq!(undone["items"][0]["attachment"], before[0], "{undone}");
    assert_eq!(listing(&mut replay), before);
    let bytes_after = replay
        .send(
            &serde_json::json!({ "kind": "attachments.readFile", "effect": "read",
                "jobId": "<job>", "ref": { "kind": "key", "key": "aaa-first" } }),
            Vec::new(),
        )
        .payloads;
    assert_eq!(bytes_after, bytes_before);
}

/// The file under `key` as the fork's guard reads it
/// (`EPDFAttachment_ExportCanonicalToOwnedBuffer`): its key, the object
/// number of its file specification, the specification, and every stream it
/// reaches as it reads.
fn canonical(replay: &Replay, key: &str) -> Option<Vec<u8>> {
    let session = replay
        .host
        .sessions()
        .session(&epdf_engine::session::registry::SessionKey::document(replay.doc_id.clone()))
        .expect("the session");
    session.document().expect("open").document().export_attachment_canonical(key)
}

#[test]
fn an_undone_delete_puts_back_the_same_file_specification_at_its_object_number() {
    // Upstream restores from the fork's raw capture: the file specification
    // comes back at its own object number, its streams as they were, and no
    // `/Params` entry the file did not have. The canonical bytes hold the
    // number and every object it reaches, so they compare all three.
    for key in ["aaa-first", "zzz-last"] {
        let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-undo");
        let before = canonical(&replay, key).expect("the file is there");
        apply(&mut replay, "delete", serde_json::json!({ "ops": [delete_op(key)] }), Vec::new())
            .expect("the delete applies");
        assert_eq!(canonical(&replay, key), None, "{key} was not deleted");

        apply(&mut replay, "undo", serde_json::json!({ "undoOf": "delete" }), Vec::new())
            .expect("the undo applies");
        assert_eq!(canonical(&replay, key).as_ref(), Some(&before), "{key} came back different");

        // And its redo removes it again: the guard is the file as the undo
        // left it.
        apply(&mut replay, "redo", serde_json::json!({ "undoOf": "undo" }), Vec::new())
            .expect("the redo applies");
        assert_eq!(canonical(&replay, key), None, "{key} was not deleted again");
    }
}

#[test]
fn an_undone_create_leaves_a_file_someone_changed_since() {
    // `removeAttachment` deletes the file only while it reads, in canonical
    // bytes, as the create left it.
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-undo");
    apply(
        &mut replay,
        "create",
        serde_json::json!({ "ops": [create_op("added.txt", 0)] }),
        vec![b"hello".to_vec()],
    )
    .expect("the create applies");
    apply(
        &mut replay,
        "replace",
        serde_json::json!({ "ops": [delete_op("added.txt"), create_op("added.txt", 0)] }),
        vec![b"someone else".to_vec()],
    )
    .expect("the replacement applies");

    let undone = apply(&mut replay, "undo", serde_json::json!({ "undoOf": "create" }), Vec::new())
        .expect("the undo answers");
    assert_eq!(undone["items"][0]["type"], "skipped", "{undone}");
    assert!(keys(&listing(&mut replay)).contains(&"added.txt"), "the undo deleted another file");
}

#[test]
fn a_refused_change_takes_back_the_attachment_ops_before_it() {
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-rollback");
    let before = listing(&mut replay);
    // The third op names a key the first one made: refused, so the delete
    // and the create before it are taken back.
    let refused = apply(
        &mut replay,
        "refused",
        serde_json::json!({ "ops": [
            create_op("added.txt", 0), delete_op("zzz-last"), create_op("added.txt", 1),
        ] }),
        vec![b"one".to_vec(), b"two".to_vec()],
    )
    .expect_err("a duplicate key is refused");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(listing(&mut replay), before, "the change was not taken back whole");
}

#[test]
fn an_undo_leaves_alone_a_file_someone_put_under_the_name_since() {
    let mut replay = Replay::open(ATTACHMENT_FIXTURE, "files-guard");
    apply(
        &mut replay,
        "first",
        serde_json::json!({ "ops": [create_op("added.txt", 0)] }),
        vec![b"first".to_vec()],
    )
    .expect("the create applies");
    apply(&mut replay, "gone", serde_json::json!({ "ops": [delete_op("added.txt")] }), Vec::new())
        .expect("the delete applies");
    apply(
        &mut replay,
        "second",
        serde_json::json!({ "ops": [create_op("added.txt", 0)] }),
        vec![b"second, longer".to_vec()],
    )
    .expect("the second create applies");

    let undone = apply(&mut replay, "undo", serde_json::json!({ "undoOf": "first" }), Vec::new())
        .expect("the undo answers");
    assert_eq!(
        undone["items"][0]["type"], "skipped",
        "the undo deleted a file it did not make: {undone}"
    );
    assert_eq!(undone["items"][0]["op"], "attachments.delete", "{undone}");
    let listed = listing(&mut replay);
    let entry = listed.iter().find(|e| e["ref"]["key"] == "added.txt").expect("still there");
    assert_eq!(entry["size"], 14);
}

/// `pieceinfo.test.ts`, "pieceInfo update preflight": an invalid value
/// refuses the whole patch with `InvalidArg` before the first write.
#[test]
fn an_invalid_piece_info_patch_is_refused_before_anything_is_written() {
    let mut replay = Replay::open(PIECE_INFO_FIXTURE, "pieceinfo-preflight");
    let application = "EMBD_PreflightTest";
    let update = |patch: serde_json::Value| {
        serde_json::json!({ "kind": "pieceInfo.update", "effect": "write", "jobId": "<job>",
            "opId": "<op>", "application": application, "patch": patch })
    };
    replay.replay(&update(serde_json::json!({ "seed": "before" }))).expect("the seed lands");
    let seeded = serde_json::json!({ "seed": { "type": "string", "value": "before" } });

    // Upstream's first patch, `invalid: Infinity`, has no JSON spelling: it
    // is checked below, where a Rust patch can hold it.
    for patch in [
        serde_json::json!({ "candidate": "must-not-land", "invalid": { "name": "" } }),
        serde_json::json!({ "candidate": "must-not-land", "invalid": ["valid", 42] }),
        serde_json::json!({ "candidate": "must-not-land", "invalid": { "unsupported": true } }),
        serde_json::json!({ "candidate": "must-not-land", "": "invalid-key" }),
    ] {
        let refused = replay.replay(&update(patch.clone())).expect_err("an invalid patch");
        assert_eq!(refused["code"], "InvalidArg", "{patch}: {refused}");
        let read = replay
            .replay(&serde_json::json!({ "kind": "pieceInfo.read", "effect": "read",
                "jobId": "<job>", "application": application }))
            .expect("read");
        assert_eq!(read["entries"], seeded, "{patch} wrote something");
    }

    let bytes = std::fs::read(test_support::workspace_root().join(PIECE_INFO_FIXTURE))
        .expect("the fixture");
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let patch: PieceInfoPatch = [
        ("candidate".to_owned(), PieceInfoPatchValue::String("must-not-land".into())),
        ("invalid".to_owned(), PieceInfoPatchValue::Number(f64::INFINITY)),
    ]
    .into_iter()
    .collect();
    let refused = update_piece_info(
        owned.document(),
        PieceInfoHolder::Catalog,
        application,
        &patch,
        &FixedClock(Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect_err("an infinite number");
    assert!(refused.is(EngineErrorCode::InvalidArg), "{refused}");
    let read = read_piece_info(
        owned.document(),
        PieceInfoHolder::Catalog,
        application,
        &CancelToken::never(),
    )
    .expect("read");
    assert!(read.is_none(), "the infinite patch wrote something: {read:?}");
}
