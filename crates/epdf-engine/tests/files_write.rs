//! G14's other gate: piece info and attachments say what the TypeScript
//! engine made them say.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run files-write
//!
//! # Two clock-stamped fields are dropped from the comparison
//!
//! An application dictionary's `/LastModified` and an embedded file's
//! creation date are both stamped from the clock, and the two engines run
//! at different instants. Their presence is asserted separately, the same
//! way `/M` is in the annotation suites.

use epdf_engine::cancel::CancelToken;
use epdf_engine::clock::{FixedClock, Timestamp};
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::files::attachments::{create_attachment, delete_attachment};
use epdf_engine::features::files::piece_info::{
    PieceInfoPatch, clear_piece_info, read_piece_info, read_piece_info_applications,
    update_piece_info,
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
                    if (key == "lastModified" || key == "creationDate") && entry.is_string() {
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

/// Run one case against a fresh copy of its fixture.
fn run(case: &Case) -> Result<serde_json::Value, epdf_engine::error::EngineError> {
    let fixture = if case.piece_info.is_some() { PIECE_INFO_FIXTURE } else { ATTACHMENT_FIXTURE };
    let bytes = std::fs::read(test_support::workspace_root().join(fixture))
        .unwrap_or_else(|e| panic!("{}: read {fixture}: {e}", case.name));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let owned = session.document().expect("open");
    let cancel = CancelToken::never();

    if let Some(input) = &case.piece_info {
        let holder = if input.on_page {
            PieceInfoHolder::Page(owned.document().page_object_number_by_index(0))
        } else {
            PieceInfoHolder::Catalog
        };
        if input.clear {
            clear_piece_info(owned.document(), holder, &input.application, &cancel)?;
        } else {
            let patch: PieceInfoPatch = serde_json::from_value(
                input.patch.clone().unwrap_or_else(|| serde_json::json!({})),
            )
            .unwrap_or_else(|e| panic!("{}: the patch does not decode: {e}", case.name));
            update_piece_info(
                owned.document(),
                holder,
                &input.application,
                &patch,
                // Fixed, because `/LastModified` is dropped from the
                // comparison and a wall clock would make this test's own
                // output depend on when it ran.
                &FixedClock(Timestamp::utc(1_700_000_000_000)),
                &cancel,
            )?;
        }
        let applications = read_piece_info_applications(owned.document(), holder, &cancel)?;
        let read = read_piece_info(owned.document(), holder, &input.application, &cancel)?;
        return Ok(serde_json::json!({ "applications": applications, "read": read }));
    }

    let input = case.attachment.as_ref().expect("an operation");
    if let Some(create) = &input.create {
        let created = create_attachment(
            owned.document(),
            &create.name,
            &create.bytes,
            create.mime_type.as_deref(),
            create.description.as_deref(),
            &cancel,
        )?;
        let listed =
            epdf_engine::features::files::attachments::list_attachments(owned.document(), &cancel)?;
        return Ok(serde_json::json!({ "created": created.created, "listed": listed }));
    }
    delete_attachment(owned.document(), input.delete_key.as_deref().expect("a key"), &cancel)?;
    let listed =
        epdf_engine::features::files::attachments::list_attachments(owned.document(), &cancel)?;
    Ok(serde_json::json!({ "listed": listed }))
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
        let mine = run(case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
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
        assert!(error.is(expected), "{}: {} (got {error})", case.name, case.covers);
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
    let stamped = snapshot.last_modified.expect("a /LastModified");
    assert!(stamped.starts_with("2023-"), "the fixed clock did not reach the file: {stamped}");
}
