//! G11's first mutator gate: a created annotation is the same annotation.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-create
//!
//! # The read is the assertion
//!
//! What is compared is the DTO the create RETURNED, and a create obtains that
//! by reading the new annotation back through the same reader a listing uses.
//! So this compares two documents rather than two descriptions of an intent:
//! anything PDFium normalized on the way in shows up on both sides or on
//! neither, and a writer that wrote nothing at all would be caught by the
//! read rather than by believing its own report.
//!
//! `modified` is dropped from the comparison: it is `/M`, stamped from the
//! clock, and the two engines ran at different instants. Its presence is
//! asserted separately, because a writer that skipped it would otherwise pass.

use epdf_engine::access::DocumentAccess;
use epdf_engine::cancel::CancelToken;
use epdf_engine::clock::FixedClock;
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::annotation::write::context::AnnotationWriteContext;
use epdf_engine::features::annotation::write::create::create_annotation;
use epdf_engine::features::annotation::write::draft::{AnnotationActor, AnnotationDraft};
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::session::authority::SessionAuthority;
use epdf_engine::session::document::DocumentSession;
use serde::Deserialize;

const SCHEMA_VERSION: u32 = 1;
const SESSION: &str = "<session>";

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
    draft: serde_json::Value,
    /// The binaries this draft names by key. A draft is JSON and a file is
    /// not, so the bytes travel beside it exactly as they do in a real
    /// mutation.
    #[serde(default)]
    resources: std::collections::BTreeMap<String, Vec<u8>>,
    page_object_number: u32,
    created: serde_json::Value,
    has_modified: bool,
    meta: ExpectedMeta,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMeta {
    changed: serde_json::Value,
    weak_refs_invalidated: bool,
    should_refetch: serde_json::Value,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("create.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("create.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.create");
    file
}

#[derive(Debug)]
struct Created {
    dto: serde_json::Value,
    meta: serde_json::Value,
}

/// PDFium stamps an attachment's `/Params /CreationDate` from the clock when
/// the bytes are written, so the two engines record different instants for
/// the same file. The generator writes the token; this puts the Rust side on
/// the same footing, and the PRESENCE of a real date is asserted separately.
const STAMPED: &str = "<stamped>";

/// Borrow the case's payloads as the write context takes them.
fn write_resources(
    bytes: &std::collections::BTreeMap<String, Vec<u8>>,
) -> epdf_engine::features::annotation::write::context::WriteResourceMap<'_> {
    bytes
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                epdf_engine::features::annotation::write::context::WriteResource {
                    bytes: value.as_slice(),
                    mime_type: None,
                },
            )
        })
        .collect()
}

fn normalize_stamps(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(normalize_stamps).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(key, entry)| {
                    if key == "creationDate" && entry.is_string() {
                        (key.clone(), serde_json::Value::from(STAMPED))
                    } else {
                        (key.clone(), normalize_stamps(entry))
                    }
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// Open the fixture, create one annotation, and report what came back.
fn create(file: &VectorFile, case: &Case) -> Result<Created, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{}: read {}: {e}", case.name, file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");

    let page_object_number =
        PageObjectNumber::new(case.page_object_number).expect("a page object number");
    // The listing is what populates the registry, and the page state the
    // impact classifier needs a known weak flag from.
    let annotations = epdf_engine::features::annotation::read::page::read_page_annotations(
        session.document().expect("open").document(),
        0,
        page_object_number,
        &epdf_engine::revision::RevisionToken::new(SESSION, page_object_number, 0),
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists");
    let state = annotations.page_state;

    let draft: AnnotationDraft = serde_json::from_value(case.draft.clone())
        .unwrap_or_else(|e| panic!("{}: the draft does not decode: {e}", case.name));
    let resources = write_resources(&case.resources);

    let owned = session.document().expect("open");
    let page = owned.document().page(0).expect("page");
    let result = create_annotation(
        owned.document(),
        &page,
        page_object_number,
        &state,
        &state,
        &draft,
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty().with_resources(&resources),
        // A fixed clock, because `/M` is dropped from the comparison and a
        // wall clock would make this test's own output depend on when it ran.
        &FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )?;

    let dto = serde_json::to_value(&result.created).expect("serialize");
    let meta = serde_json::to_value(&result.meta).expect("serialize");
    Ok(Created { dto, meta })
}

#[test]
fn a_created_annotation_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 10, "the case list shrank");

    for case in &file.cases {
        let created = create(&file, case).unwrap_or_else(|e| panic!("{}: create: {e}", case.name));

        let mut mine = created.dto.as_object().expect("an object").clone();
        let modified = mine.remove("modified").expect("the DTO carries a modified key");
        assert_eq!(
            case.has_modified,
            modified.as_str().is_some_and(|m| !m.is_empty()),
            "{}: /M presence differs",
            case.name,
        );

        // An attachment's creation date is PDFium's own stamp, so it is
        // replaced by the token the generator wrote and its shape is
        // asserted here instead: a PDF date, not an empty string.
        if let Some(date) = mine
            .get("file")
            .and_then(|file| file.get("creationDate"))
            .and_then(serde_json::Value::as_str)
        {
            assert!(date.starts_with("D:"), "{}: /Params /CreationDate is {date}", case.name);
        }

        assert_eq!(
            test_support::canonical_numbers(&normalize_stamps(&serde_json::Value::Object(mine))),
            test_support::canonical_numbers(&case.created),
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn a_create_is_append_only_and_says_so() {
    // The envelope, which is the impact classifier's verdict rather than the
    // writer's. A create appends, so no existing index moves and no client
    // holding one has to re-read, whatever else is on the page.
    let file = load();
    for case in &file.cases {
        let created = create(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let meta = created.meta.as_object().expect("an object");

        assert_eq!(
            test_support::canonical_numbers(&meta["changed"]),
            test_support::canonical_numbers(&case.meta.changed),
            "{}: the changed list",
            case.name,
        );
        assert_eq!(
            meta["weakRefsInvalidated"].as_bool(),
            Some(case.meta.weak_refs_invalidated),
            "{}: the weak flag",
            case.name,
        );
        assert_eq!(meta["shouldRefetch"], case.meta.should_refetch, "{}", case.name);
        assert!(meta["shouldRefetch"].is_null(), "{}: a create asked for a refetch", case.name);
    }
}

#[test]
fn a_new_annotation_is_addressable() {
    // Creation goes through the fork's indirect helper, so the result has an
    // object number and therefore an address that survives a reordering. An
    // annotation created weak would be one nobody can refer to.
    let file = load();
    for case in &file.cases {
        let created = create(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(
            created.dto["identityQuality"].as_str(),
            Some("durable"),
            "{}: a created annotation must be durable",
            case.name,
        );
        assert_eq!(
            created.dto["ref"]["kind"].as_str(),
            Some("objectNumber"),
            "{}: and addressed by its object number",
            case.name,
        );
    }
}

#[test]
fn a_draft_that_cannot_be_written_is_refused_before_anything_is_written() {
    // The apply boundary. A markup with no quads has no `/Rect` to derive,
    // a vertex path of one point draws nothing, an ink stroke with no points
    // is a pen that never touched the page,
    // and a shape with an inside-out rect describes no region; both are the
    // caller's mistake, and both have to be caught before an annotation
    // exists in the file, because nothing here can take one back out.
    let file = load();
    let case = file.cases.first().expect("a case to borrow the fixture from");

    for (draft, what) in [
        (serde_json::json!({ "subtype": "highlight", "quadPoints": [] }), "no quads"),
        (
            serde_json::json!({
                "subtype": "square",
                "rect": { "left": 100.0, "bottom": 100.0, "right": 10.0, "top": 200.0 },
            }),
            "an inside-out rect",
        ),
        (
            serde_json::json!({
                "subtype": "square",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
                "opacity": 2.0,
            }),
            "an opacity outside 0..=1",
        ),
        (
            serde_json::json!({
                "subtype": "polygon",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
                "vertices": [{ "x": 10.0, "y": 10.0 }],
            }),
            "a vertex path of one point",
        ),
        (
            serde_json::json!({
                "subtype": "ink",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
                "inkList": [[{ "x": 10.0, "y": 10.0 }], []],
            }),
            "an ink stroke with no points",
        ),
        (
            serde_json::json!({
                "subtype": "square",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 },
                "rotation": 30.0,
            }),
            "a rotation with no logical box to turn",
        ),
        (
            serde_json::json!({
                "subtype": "text",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 30.0, "top": 30.0 },
                "state": "accepted",
            }),
            "a review state belonging to no model",
        ),
        (
            serde_json::json!({
                "subtype": "free-text",
                "intent": "free-text-callout",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "fontFamily": "helvetica",
                "fontSize": 12.0,
                "textAlign": "left",
            }),
            "a callout with no leader to call anything out with",
        ),
        (
            serde_json::json!({
                "subtype": "free-text",
                "intent": "free-text",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "fontFamily": "helvetica",
                "fontSize": 12.0,
                "textAlign": "left",
                "calloutLine": [{ "x": 0.0, "y": 0.0 }, { "x": 10.0, "y": 10.0 }],
            }),
            "a leader on a plain text box",
        ),
        (
            serde_json::json!({
                "subtype": "free-text",
                "intent": "free-text",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "fontFamily": "helvetica",
                "fontSize": 0.0,
                "textAlign": "left",
            }),
            "text drawn at no size at all",
        ),
        (
            serde_json::json!({
                "subtype": "free-text",
                "intent": "free-text",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "fontFamily": "a-font-nobody-registered",
                "fontSize": 12.0,
                "textAlign": "left",
            }),
            "a font this host has no registry to resolve",
        ),
        (
            serde_json::json!({
                "subtype": "file-attachment",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 30.0, "top": 30.0 },
                "file": { "resource": "never-arrived", "name": "notes.txt" },
            }),
            "a file whose bytes did not travel with the draft",
        ),
        (
            serde_json::json!({
                "subtype": "file-attachment",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 30.0, "top": 30.0 },
                "file": { "resource": "r0", "name": "" },
            }),
            "a filespec that names nothing",
        ),
        (
            serde_json::json!({
                "subtype": "link",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 30.0 },
                "target": { "kind": "uri", "uri": "" },
            }),
            "a link to an empty uri",
        ),
        (
            serde_json::json!({
                "subtype": "stamp",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "source": { "resource": "not-a-format" },
            }),
            "a stamp whose source is not a format this build places",
        ),
        (
            serde_json::json!({
                "subtype": "stamp",
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 60.0 },
                "source": { "resource": "r0" },
                "name": "QuarterlyReviewed",
            }),
            "a stamp label outside the enumerated set",
        ),
    ] {
        let mut refused_case = Case {
            name: format!("refusal: {what}"),
            // Two payloads, so a refusal that is about something OTHER
            // than the bytes gets bytes that pass the sniffer: with only
            // the unusable payload, every stamp refusal would be the same
            // refusal and the later checks would never run.
            resources: [
                (
                    String::from("r0"),
                    vec![
                        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 4,
                        0, 0, 0, 2, 8, 2, 0, 0, 0, 240, 202, 234, 52, 0, 0, 0, 16, 73, 68, 65, 84,
                        120, 156, 99, 184, 163, 161, 1, 71, 12, 200, 28, 0, 127, 154, 9, 97, 122,
                        180, 22, 59, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
                    ],
                ),
                (String::from("not-a-format"), b"bytes".to_vec()),
            ]
            .into_iter()
            .collect(),
            covers: what.to_string(),
            draft,
            page_object_number: case.page_object_number,
            created: serde_json::Value::Null,
            has_modified: false,
            meta: ExpectedMeta {
                changed: serde_json::Value::Null,
                weak_refs_invalidated: false,
                should_refetch: serde_json::Value::Null,
            },
        };
        // Both creates go through ONE document. Running them through two
        // would make the second assertion vacuous: a fresh document has an
        // empty page whatever the first draft did to its own copy.
        let (error, after) = refuse_then_create(
            &file,
            &refused_case,
            &serde_json::json!({
                "subtype": "highlight",
                "quadPoints": [{
                    "p1": { "x": 0.0, "y": 10.0 }, "p2": { "x": 10.0, "y": 10.0 },
                    "p3": { "x": 0.0, "y": 0.0 }, "p4": { "x": 10.0, "y": 0.0 }
                }],
            }),
        );
        assert!(error.is(EngineErrorCode::InvalidArg), "{what}: {error}");
        assert_eq!(
            after["index"].as_u64(),
            Some(0),
            "{what}: the refused draft left something on the page",
        );
        let _ = &mut refused_case;
    }
}

/// Refuse one draft and create another, against the SAME document.
///
/// The point of the pair is the index the second create reports. A refusal
/// that happened after the first native write leaves an annotation behind,
/// and the only way to see it is to ask the same page again: nothing can
/// take an annotation back out of a document this layer has written to.
fn refuse_then_create(
    file: &VectorFile,
    refused: &Case,
    valid: &serde_json::Value,
) -> (epdf_engine::error::EngineError, serde_json::Value) {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let page_object_number =
        PageObjectNumber::new(refused.page_object_number).expect("a page object number");
    let state = epdf_engine::features::annotation::read::page::read_page_annotations(
        session.document().expect("open").document(),
        0,
        page_object_number,
        &epdf_engine::revision::RevisionToken::new(SESSION, page_object_number, 0),
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists")
    .page_state;
    let resources = write_resources(&refused.resources);
    let owned = session.document().expect("open");
    let page = owned.document().page(0).expect("page");
    let clock = FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000));

    let run = |draft: &serde_json::Value| {
        let draft: AnnotationDraft = serde_json::from_value(draft.clone())
            .unwrap_or_else(|e| panic!("{}: the draft does not decode: {e}", refused.name));
        create_annotation(
            owned.document(),
            &page,
            page_object_number,
            &state,
            &state,
            &draft,
            &AnnotationActor::default(),
            &AnnotationWriteContext::empty().with_resources(&resources),
            &clock,
            &CancelToken::never(),
        )
    };

    let error =
        run(&refused.draft).err().unwrap_or_else(|| panic!("{}: not refused", refused.name));
    let created =
        run(valid).unwrap_or_else(|e| panic!("{}: a valid draft still creates: {e}", refused.name));
    (error, serde_json::to_value(&created.created).expect("serialize"))
}

#[test]
fn a_created_annotation_ships_with_an_appearance() {
    // The one thing the DTO cannot show. An annotation's `/AP` is not on the
    // wire, so every comparison in this file passes for a writer that never
    // generated one, and the result would be an annotation that is in the
    // document and draws nothing.
    //
    // Checked by rendering, which is also the only way a consumer would find
    // out.
    use epdf_engine::features::annotation::appearance::{
        AppearanceRenderOptions, render_page_appearances,
    };

    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "square-decorated")
        .expect("a case with something to draw");

    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture)).expect("read");
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let page_object_number =
        PageObjectNumber::new(case.page_object_number).expect("a page object number");
    let revision = epdf_engine::revision::RevisionToken::new(SESSION, page_object_number, 0);

    let before = epdf_engine::features::annotation::read::page::read_page_annotations(
        session.document().expect("open").document(),
        0,
        page_object_number,
        &revision,
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");
    assert!(before.annotations.is_empty(), "the fixture starts with no annotations");

    let draft: AnnotationDraft = serde_json::from_value(case.draft.clone()).expect("draft");
    let resources = write_resources(&case.resources);
    let owned = session.document().expect("open");
    let page = owned.document().page(0).expect("page");
    create_annotation(
        owned.document(),
        &page,
        page_object_number,
        &before.page_state,
        &before.page_state,
        &draft,
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty().with_resources(&resources),
        &FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .expect("create");

    // The appearance reader needs a page it can enumerate annotations on,
    // which is the same page handle the create just wrote through.
    let owned_page = owned.page(page_object_number).expect("page");
    let rendered = render_page_appearances(
        &owned_page,
        page_object_number,
        &revision,
        &AppearanceRenderOptions::default(),
        &CancelToken::never(),
    )
    .expect("render");

    assert_eq!(
        rendered.appearances.len(),
        1,
        "the new annotation has no /AP, so it is in the document and draws nothing",
    );
    let drawn = &rendered.appearances[0];
    assert_eq!(drawn.rect.left, 300.0);
    assert!(drawn.raster.width > 0 && drawn.raster.height > 0);

    // And it actually painted: the fixture's page is blank, so any opaque
    // pixel in the appearance came from the annotation.
    let painted = (0..drawn.raster.height).any(|y| {
        (0..drawn.raster.width).any(|x| drawn.raster.pixel(x, y).is_some_and(|p| p.3 > 0))
    });
    assert!(painted, "the appearance stream is there and paints nothing");
}

#[test]
fn a_created_annotation_survives_a_save_and_reopen() {
    // The other half of "it is really in the document". Every comparison
    // above reads the annotation back out of the session that created it,
    // where an appearance stream the page does not reference still renders.
    // Saving and reopening is what makes the page's own content stream
    // matter, and it is also what a caller actually does with the result.
    use epdf_engine::features::annotation::appearance::{
        AppearanceRenderOptions, render_page_appearances,
    };
    use epdf_engine::features::save::{SaveMode, save_to_buffer};

    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "square-decorated")
        .expect("a case with something to draw");

    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture)).expect("read");
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let page_object_number =
        PageObjectNumber::new(case.page_object_number).expect("a page object number");
    let revision = epdf_engine::revision::RevisionToken::new(SESSION, page_object_number, 0);

    let before = epdf_engine::features::annotation::read::page::read_page_annotations(
        session.document().expect("open").document(),
        0,
        page_object_number,
        &revision,
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");

    let draft: AnnotationDraft = serde_json::from_value(case.draft.clone()).expect("draft");
    {
        let owned = session.document().expect("open");
        let page = owned.document().page(0).expect("page");
        create_annotation(
            owned.document(),
            &page,
            page_object_number,
            &before.page_state,
            &before.page_state,
            &draft,
            &AnnotationActor::default(),
            &AnnotationWriteContext::empty(),
            &FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000)),
            &CancelToken::never(),
        )
        .expect("create");
    }

    let saved = save_to_buffer(&session, SaveMode::Full, &CancelToken::never()).expect("save");

    let mut reopened = DocumentSession::new();
    reopened.open_with(saved, None, SessionAuthority::unrestricted()).expect("reopen");
    let listing = epdf_engine::features::annotation::read::page::read_page_annotations(
        reopened.document().expect("open").document(),
        0,
        page_object_number,
        &revision,
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");

    assert_eq!(listing.annotations.len(), 1, "the annotation did not survive the save");
    let dto = serde_json::to_value(&listing.annotations[0]).expect("serialize");
    assert_eq!(dto["subtype"].as_str(), Some("square"));
    assert_eq!(
        test_support::canonical_numbers(&dto["rect"]),
        test_support::canonical_numbers(&case.created["rect"])
    );
    assert_eq!(dto["interiorColor"], case.created["interiorColor"]);

    // And it still draws, which is what the page's regenerated content
    // stream is for: an appearance the page does not reference is in the
    // file and invisible.
    let owned_page = reopened.document().expect("open").page(page_object_number).expect("page");
    let rendered = render_page_appearances(
        &owned_page,
        page_object_number,
        &revision,
        &AppearanceRenderOptions::default(),
        &CancelToken::never(),
    )
    .expect("render");
    assert_eq!(rendered.appearances.len(), 1, "the /AP did not survive the save");
}

/// Open the fixture, create one annotation, and hand the caller the written
/// annotation to inspect directly.
///
/// The DTO comparison cannot see everything a writer decides. Where the
/// reader normalizes on the way back out, two different files read as the
/// same DTO, and the one that is wrong is wrong for every OTHER reader.
fn with_written<R>(
    file: &VectorFile,
    name: &str,
    inspect: impl FnOnce(&pdfium::Annotation<'_>) -> R,
) -> R {
    let case =
        file.cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"));
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("{name}: read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");
    let page_object_number =
        PageObjectNumber::new(case.page_object_number).expect("a page object number");
    let annotations = epdf_engine::features::annotation::read::page::read_page_annotations(
        session.document().expect("open").document(),
        0,
        page_object_number,
        &epdf_engine::revision::RevisionToken::new(SESSION, page_object_number, 0),
        &mut epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("the page lists");
    let state = annotations.page_state;
    let draft: AnnotationDraft =
        serde_json::from_value(case.draft.clone()).expect("the draft decodes");
    let resources = write_resources(&case.resources);

    let owned = session.document().expect("open");
    let page = owned.document().page(0).expect("page");
    create_annotation(
        owned.document(),
        &page,
        page_object_number,
        &state,
        &state,
        &draft,
        &AnnotationActor::default(),
        &AnnotationWriteContext::empty().with_resources(&resources),
        &FixedClock(epdf_engine::clock::Timestamp::utc(1_700_000_000_000)),
        &CancelToken::never(),
    )
    .unwrap_or_else(|e| panic!("{name}: create: {e}"));

    // The fixture page starts empty, so the created annotation is the only
    // one on it.
    inspect(&page.annotation(0).expect("the created annotation"))
}

#[test]
fn a_review_state_is_written_in_the_spelling_the_pdf_uses() {
    // §12.5.6.3 names these with a capital. The reader passes an unknown
    // spelling through verbatim, which is what makes a lowercase `/State`
    // invisible on a round trip through this engine: `accepted` goes in,
    // fails to match `Accepted` in the table on the way out, and is handed
    // back as `accepted`. It reads correctly here and means nothing to
    // Acrobat, so the assertion is against the dictionary.
    let file = load();
    for (name, model, state) in [
        ("text-reviewed", Some("Review"), Some("Accepted")),
        ("text-model-alone", Some("Marked"), None),
        // A model nobody standardized is written exactly as it arrived.
        // Normalizing it would be inventing a spelling for somebody else's
        // vocabulary.
        ("text-foreign-state", Some("LegalReview"), Some("AwaitingCounsel")),
        ("text-defaults", None, None),
    ] {
        with_written(&file, name, |written| {
            assert_eq!(
                written.string_value("StateModel").expect("a dictionary read").as_deref(),
                model,
                "{name}: /StateModel",
            );
            assert_eq!(
                written.string_value("State").expect("a dictionary read").as_deref(),
                state,
                "{name}: /State",
            );
        });
    }
}

#[test]
fn a_stamp_fit_policy_decides_where_the_picture_lands() {
    // The DTO carries the annotation's `/Rect` and never the placement
    // inside it, so three fits of the same bytes into the same box read
    // back identically. What differs is the appearance, and that is what
    // this asserts.
    //
    // The source is 4x2 in an 80x80 box, and the bounds are in the
    // appearance form's own space, which the closing re-fit anchors at the
    // box's origin. `contain` fits the width and centres the 40-point band;
    // `cover` fits the height and overflows 40 points either side, which is
    // the crop; `fill` takes the box exactly and distorts.
    let file = load();
    for (name, expected) in [
        ("stamp-png-contain", [0.0_f32, 20.0, 80.0, 60.0]),
        ("stamp-png-cover", [-40.0, 0.0, 120.0, 80.0]),
        ("stamp-png-fill", [0.0, 0.0, 80.0, 80.0]),
    ] {
        with_written(&file, name, |written| {
            assert_eq!(written.object_count(), 1, "{name}: appearance objects");
            let bounds = written.object_bounds(0).expect("the image object's bounds");
            let actual = [bounds.left, bounds.bottom, bounds.right, bounds.top];
            for (got, want) in actual.iter().zip(expected.iter()) {
                assert!(
                    (got - want).abs() < 0.5,
                    "{name}: placed at {actual:?}, expected about {expected:?}",
                );
            }
        });
    }
}

#[test]
fn a_rotated_stamp_is_authored_in_the_frame_it_is_turned_from() {
    // Authoring into the ROTATED box bakes the letterbox bands in, and the
    // closing re-fit then shrinks the picture by the aspect ratio a second
    // time. The symptom is a small image adrift in white space, and it only
    // appears when rotated, because at zero degrees the two frames coincide.
    //
    // So the assertion is that the content fills the LOGICAL box: 90x60,
    // with a 4:2 source, is a 90-point band 45 points tall.
    let file = load();
    with_written(&file, "stamp-rotated", |written| {
        let bounds = written.object_bounds(0).expect("the image object's bounds");
        let width = bounds.right - bounds.left;
        let height = bounds.top - bounds.bottom;
        assert!(
            (width - 90.0).abs() < 0.5 && (height - 45.0).abs() < 0.5,
            "the content is {width}x{height}, expected about 90x45 in the logical box",
        );
    });
}

#[test]
fn a_redaction_gets_a_default_appearance_only_when_it_has_a_label() {
    // ISO 32000-2 requires a `/DA` alongside an `/OverlayText`, and a
    // redaction with no label has nothing for one to describe. The DTO
    // cannot tell the two apart: the reader reports the SAME defaults for a
    // missing `/DA` that a writer would have put in one, so an engine that
    // emitted a `/DA` on every redaction passes every vector while leaving
    // an entry in the file that says nothing.
    let file = load();
    for (name, has_da, overlay) in [
        ("redact-text-with-label", true, Some("REDACTED")),
        ("redact-autofit-label", true, Some("gone")),
        ("redact-area", false, None),
        // An empty string is not a label, so it obliges no `/DA` either.
        ("redact-empty-label", false, None),
    ] {
        with_written(&file, name, |written| {
            assert_eq!(written.default_appearance().is_some(), has_da, "{name}: /DA");
            assert_eq!(
                written.overlay_text().expect("a dictionary read").as_deref(),
                overlay,
                "{name}: /OverlayText",
            );
        });
    }
}

#[test]
fn the_transform_keys_land_in_the_dictionary_and_not_only_in_the_dto() {
    // The reader normalizes a zero rotation back to absent, so the DTO
    // cannot tell a cleared `/Rotation` from one written as zero, and it
    // never reports `/SchemaVersion` at all. Both are things a later reader
    // of the FILE depends on, so they are asserted against the dictionary.
    let file = load();
    // name -> (rotation, has an unrotated box, the dictionary exists)
    let expected: [(&str, Option<f32>, bool, bool); 4] = [
        ("square-rotated", Some(45.0), true, true),
        ("square-zero-rotation", None, true, true),
        ("ink-advisory-rotation", Some(45.0), false, true),
        ("polyline-zero-rotation", None, false, false),
    ];

    for (name, rotation, has_box, has_dictionary) in expected {
        with_written(&file, name, |written| {
            assert_eq!(
                written.has_embed_metadata(),
                has_dictionary,
                "{name}: the /EMBD_Metadata dictionary",
            );
            assert_eq!(
                written.embed_metadata_number("Rotation").expect("a dictionary read"),
                rotation,
                "{name}: /Rotation",
            );
            assert_eq!(
                written.embed_metadata_rect("UnrotatedRect").expect("a dictionary read").is_some(),
                has_box,
                "{name}: /UnrotatedRect",
            );
            if has_dictionary {
                assert_eq!(
                    written.embed_metadata_number("SchemaVersion").expect("a dictionary read"),
                    Some(1.0),
                    "{name}: /SchemaVersion is seeded with the first transform key",
                );
            }
        });
    }
}
