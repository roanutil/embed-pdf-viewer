//! G11's update gate: the same patch leaves the same annotation.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-update
//!
//! Each case CREATES its subject from a draft and then patches it, so every
//! case says what it starts from rather than depending on how some fixture
//! annotation happened to be written.
//!
//! The appearance verdict is compared alongside the DTO, because it is the
//! half a field comparison cannot see: preserving a foreign stream and
//! re-baking an identical-looking one produce the same dictionary and
//! different documents.

use epdf_engine::cancel::CancelToken;
use epdf_engine::clock::{FixedClock, Timestamp};
use epdf_engine::error::EngineErrorCode;
use epdf_engine::features::actions::vocabulary::ActionReadBudgetTracker;
use epdf_engine::features::annotation::read::page::read_page_annotations;
use epdf_engine::features::annotation::write::create::create_annotation;
use epdf_engine::features::annotation::write::draft::{AnnotationActor, AnnotationDraft};
use epdf_engine::features::annotation::write::patch::AnnotationPatch;
use epdf_engine::features::annotation::write::update::update_annotation;
use epdf_engine::identity::PageObjectNumber;
use epdf_engine::ids::SeededIdSource;
use epdf_engine::revision::{PageState, RevisionToken, WeakAnnotationState};
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
    patch: serde_json::Value,
    page_object_number: u32,
    updated: serde_json::Value,
    has_modified: bool,
    appearance: serde_json::Value,
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
    let path = test_support::corpus::vectors_dir("annotation").join("update.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("update.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.update");
    file
}

/// See `annotation_reader.rs`.
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

fn states(page: PageObjectNumber, weak: bool) -> (PageState, PageState) {
    // An update is non-structural, so the two states are the same: no bump.
    //
    // `weak` is what makes the envelope's verdict a test rather than a
    // coincidence. On a page with a weak annotation, a DELETE would ask for
    // a refetch and an update must not, so a mutation reported as the wrong
    // kind is visible; on a fully durable page the two agree.
    let state = PageState {
        page_object_number: page,
        revision: RevisionToken::new(SESSION, page, 0),
        weak_annotation_state: WeakAnnotationState::Known { has_any_weak_annotations: weak },
    };
    (state.clone(), state)
}

#[derive(Debug)]
struct Updated {
    /// The `/M` the CREATE stamped, so a refresh is visible as a change.
    created_modified: String,
    dto: serde_json::Value,
    appearance: serde_json::Value,
    meta: serde_json::Value,
}

/// Create the case's subject, then patch it.
fn run(file: &VectorFile, case: &Case) -> Result<Updated, epdf_engine::error::EngineError> {
    let bytes = std::fs::read(test_support::workspace_root().join(&file.fixture))
        .unwrap_or_else(|e| panic!("read {}: {e}", file.fixture));
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");

    let page = PageObjectNumber::new(case.page_object_number).expect("page number");
    let (before, after) = states(page, true);
    let created_at = FixedClock(Timestamp::utc(1_700_000_000_000));
    // A LATER instant for the update, so a refresh of `/M` is visible as a
    // different value rather than as the same one written twice.
    let updated_at = FixedClock(Timestamp::utc(1_800_000_000_000));
    // Seeded, so a stamped `/NM` is the same string on every run. Nothing in
    // this case list stamps one, and the seed is here so the day one does,
    // the vector does not start moving.
    let ids = SeededIdSource::new(7);

    let draft: AnnotationDraft = serde_json::from_value(case.draft.clone())
        .unwrap_or_else(|e| panic!("{}: the draft does not decode: {e}", case.name));
    let patch: AnnotationPatch = serde_json::from_value(case.patch.clone())
        .unwrap_or_else(|e| panic!("{}: the patch does not decode: {e}", case.name));

    let owned = session.document().expect("open");
    let view = owned.document().page(0).expect("page");
    let created = create_annotation(
        owned.document(),
        &view,
        page,
        &before,
        &after,
        &draft,
        &AnnotationActor::default(),
        &created_at,
        &CancelToken::never(),
    )
    .unwrap_or_else(|e| panic!("{}: create: {e}", case.name));
    let created_modified = serde_json::to_value(&created.created).expect("serialize")["modified"]
        .as_str()
        .expect("a created /M")
        .to_owned();

    let result = update_annotation(
        owned.document(),
        &view,
        0,
        page,
        &created.created.base.annotation_ref,
        &patch,
        &before,
        &after,
        &updated_at,
        &ids,
        &CancelToken::never(),
    )?;

    Ok(Updated {
        created_modified,
        dto: serde_json::to_value(&result.updated).expect("serialize"),
        appearance: serde_json::to_value(result.appearance).expect("serialize"),
        meta: serde_json::to_value(&result.meta).expect("serialize"),
    })
}

#[test]
fn an_updated_annotation_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert!(file.cases.len() >= 12, "the case list shrank");

    for case in &file.cases {
        let result = run(&file, case).unwrap_or_else(|e| panic!("{}: update: {e}", case.name));

        let mut mine = result.dto.as_object().expect("an object").clone();
        let modified = mine.remove("modified").expect("the DTO carries a modified key");
        assert_eq!(
            case.has_modified,
            modified.as_str().is_some_and(|m| !m.is_empty()),
            "{}: /M presence differs",
            case.name,
        );
        // Every update refreshes `/M`, whatever the patch touched. The
        // create stamped an earlier instant, so an update that did not
        // refresh would still report a non-empty value and the wrong one.
        assert_ne!(
            modified.as_str(),
            Some(result.created_modified.as_str()),
            "{}: /M was not refreshed",
            case.name,
        );

        assert_eq!(
            canonical_numbers(&serde_json::Value::Object(mine)),
            canonical_numbers(&case.updated),
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn the_appearance_verdict_agrees_with_the_typescript_engine() {
    // The half a field comparison cannot see. Both verdicts have to occur in
    // the list, or a classifier answering one thing always would pass.
    let file = load();
    let mut preserved = 0usize;
    let mut regenerated = 0usize;

    for case in &file.cases {
        let result = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(result.appearance, case.appearance, "{}: {}", case.name, case.covers);
        match case.appearance["action"].as_str() {
            Some("preserved") => preserved += 1,
            Some("regenerated") => regenerated += 1,
            other => panic!("{}: an appearance action of {other:?}", case.name),
        }
    }

    assert!(preserved > 0, "no case preserves an appearance");
    assert!(regenerated > 0, "no case re-bakes one");
}

#[test]
fn an_update_is_not_structural() {
    // No index moves, so no revision bump and no client is asked to re-read,
    // whatever the patch did.
    let file = load();
    for case in &file.cases {
        let result = run(&file, case).unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let meta = result.meta.as_object().expect("an object");
        assert_eq!(meta["changed"], case.meta.changed, "{}: the changed list", case.name);
        assert_eq!(
            meta["weakRefsInvalidated"].as_bool(),
            Some(case.meta.weak_refs_invalidated),
            "{}",
            case.name,
        );
        assert_eq!(meta["shouldRefetch"], case.meta.should_refetch, "{}", case.name);
        assert!(meta["shouldRefetch"].is_null(), "{}: an update asked for a refetch", case.name);
    }
}

#[test]
fn a_weak_annotation_is_strengthened_by_its_own_update() {
    // The one identity effect an update can have. A weak annotation has no
    // address that survives a reordering, so the engine stamps an `/NM`
    // while it has the annotation open, and the caller gets a stronger
    // reference back than the one it passed in.
    //
    // `annotation_base.pdf` is the fixture, because it is the only one with
    // an annotation that has neither an object number nor a name.
    let bytes = std::fs::read(
        test_support::workspace_root()
            .join("tooling/engine-conformance/fixtures/annotation_base.pdf"),
    )
    .expect("read");
    let mut session = DocumentSession::new();
    session.open_with(bytes, None, SessionAuthority::unrestricted()).expect("open");

    let page = PageObjectNumber::new(
        session.document().expect("open").document().page_object_number_by_index(0),
    )
    .expect("page number");
    let (before, after) = states(page, true);

    let listing = read_page_annotations(
        session.document().expect("open").document(),
        0,
        page,
        &before.revision,
        &mut ActionReadBudgetTracker::default(),
        &CancelToken::never(),
    )
    .expect("list");
    let weak = listing
        .annotations
        .iter()
        .find(|a| a.base.identity_quality == epdf_engine::identity::AnnotationIdentityQuality::Weak)
        .expect("the fixture has a weak annotation");
    let weak_ref = weak.base.annotation_ref.clone();
    let weak_index = weak.base.index;

    // The weak one is a square, so a square patch is what applies to it.
    let patch: AnnotationPatch = serde_json::from_value(serde_json::json!({
        "subtype": "square",
        "contents": "touched",
    }))
    .expect("patch");

    let owned = session.document().expect("open");
    let view = owned.document().page(0).expect("page");
    let result = update_annotation(
        owned.document(),
        &view,
        0,
        page,
        &weak_ref,
        &patch,
        &before,
        &after,
        &FixedClock(Timestamp::utc(1_700_000_000_000)),
        &SeededIdSource::new(7),
        &CancelToken::never(),
    )
    .expect("update");

    let dto = serde_json::to_value(&result.updated).expect("serialize");
    assert_eq!(
        dto["identityQuality"].as_str(),
        Some("durable"),
        "the weak annotation was not strengthened",
    );
    assert_eq!(dto["ref"]["kind"].as_str(), Some("nm"));
    let stamped = dto["ref"]["nm"].as_str().expect("a stamped name");
    assert_eq!(stamped.len(), 36, "a UUID v4 is 36 characters: {stamped}");
    assert_eq!(dto["nm"].as_str(), Some(stamped));

    // It did not MOVE: strengthening is an identity change, not a structural
    // one, so the annotation is still at the index it was.
    assert_eq!(dto["index"].as_u64(), Some(u64::from(weak_index)));
    assert!(result.meta.should_refetch.is_none());
}

#[test]
fn a_patch_for_the_wrong_subtype_is_refused() {
    // Before anything is written. A square patch carries fields a highlight
    // has no dictionary entries for, and applying the ones they share would
    // half-write an annotation nobody asked for.
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "colour-change").expect("a highlight case");
    let mut wrong = Case {
        name: "a square patch on a highlight".into(),
        covers: String::new(),
        draft: case.draft.clone(),
        patch: serde_json::json!({ "subtype": "square", "opacity": 0.5 }),
        page_object_number: case.page_object_number,
        updated: serde_json::Value::Null,
        has_modified: false,
        appearance: serde_json::Value::Null,
        meta: ExpectedMeta {
            changed: serde_json::Value::Null,
            weak_refs_invalidated: false,
            should_refetch: serde_json::Value::Null,
        },
    };
    let refused = run(&file, &wrong).expect_err("a square patch on a highlight");
    assert!(refused.is(EngineErrorCode::InvalidArg), "{refused}");

    // And a text-markup patch that would shrink the quad list, which PDFium
    // cannot do: the extra quads would stay and the annotation would mark
    // text nobody selected.
    wrong.draft = serde_json::json!({
        "subtype": "highlight",
        "quadPoints": [
            { "p1": { "x": 0.0, "y": 10.0 }, "p2": { "x": 10.0, "y": 10.0 },
              "p3": { "x": 0.0, "y": 0.0 }, "p4": { "x": 10.0, "y": 0.0 } },
            { "p1": { "x": 0.0, "y": 30.0 }, "p2": { "x": 10.0, "y": 30.0 },
              "p3": { "x": 0.0, "y": 20.0 }, "p4": { "x": 10.0, "y": 20.0 } }
        ],
    });
    wrong.patch = serde_json::json!({
        "subtype": "highlight",
        "quadPoints": [
            { "p1": { "x": 0.0, "y": 10.0 }, "p2": { "x": 10.0, "y": 10.0 },
              "p3": { "x": 0.0, "y": 0.0 }, "p4": { "x": 10.0, "y": 0.0 } }
        ],
    });
    let refused = run(&file, &wrong).expect_err("a shrinking quad list");
    assert!(refused.is(EngineErrorCode::InvalidArg), "{refused}");

    // An empty list is refused for the same reason and a different one: it
    // would read as deleting the attachment points.
    wrong.patch = serde_json::json!({ "subtype": "highlight", "quadPoints": [] });
    let refused = run(&file, &wrong).expect_err("an empty quad list");
    assert!(refused.is(EngineErrorCode::InvalidArg), "{refused}");
}
