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

/// See `annotation_reader.rs`: JSON has one number type and the two
/// serializers spell a whole one differently.
fn canonical_numbers(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9e15 => {
                serde_json::Value::from(float as i64)
            }
            _ => value.clone(),
        },
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_numbers).collect())
        }
        serde_json::Value::Object(fields) => serde_json::Value::Object(
            fields.iter().map(|(key, entry)| (key.clone(), canonical_numbers(entry))).collect(),
        ),
        _ => value.clone(),
    }
}

#[derive(Debug)]
struct Created {
    dto: serde_json::Value,
    meta: serde_json::Value,
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

        assert_eq!(
            canonical_numbers(&serde_json::Value::Object(mine)),
            canonical_numbers(&case.created),
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
            canonical_numbers(&meta["changed"]),
            canonical_numbers(&case.meta.changed),
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
    ] {
        let mut refused_case = Case {
            name: format!("refusal: {what}"),
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
        let error = create(&file, &refused_case).expect_err(what);
        assert!(error.is(EngineErrorCode::InvalidArg), "{what}: {error}");

        // And nothing was added to the page: the refusal happened before the
        // first write.
        refused_case.draft = serde_json::json!({
            "subtype": "highlight",
            "quadPoints": [{
                "p1": { "x": 0.0, "y": 10.0 }, "p2": { "x": 10.0, "y": 10.0 },
                "p3": { "x": 0.0, "y": 0.0 }, "p4": { "x": 10.0, "y": 0.0 }
            }],
        });
        let after = create(&file, &refused_case).expect("a valid draft still creates");
        assert_eq!(
            after.dto["index"].as_u64(),
            Some(0),
            "{what}: the refused draft left something on the page",
        );
    }
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
    assert_eq!(canonical_numbers(&dto["rect"]), canonical_numbers(&case.created["rect"]));
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
