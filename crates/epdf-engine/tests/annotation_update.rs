//! G11's update gate: the same patch leaves the same annotation.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-update
//!
//! Each case CREATES its subject from its v3 page-space draft and then sends
//! its v3 page-space patch as the wire request, through [`Host::call`], with
//! the bytes beside each as payloads by role. Every case says what it starts
//! from rather than depending on how some fixture annotation happened to be
//! written.
//!
//! The appearance verdict is compared alongside the read, because it is the
//! half a field comparison cannot see: preserving a foreign stream and
//! re-baking an identical-looking one produce the same dictionary and
//! different documents.
//!
//! The stamps are left out of the comparison, as the generator leaves them
//! out (`normalizeWriteStamps`): `modifiedAt` is `/M` from the clock (its
//! presence and its refresh are asserted), `createdAt` and a fresh UUIDv7
//! `/NM` read `"<stamped>"`.

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;

use common::replay::Replay;
use epdf_engine::clock::{Clock, Timestamp};
use epdf_engine::host::Host;
use epdf_engine::session::registry::SessionKey;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;
const STAMPED: &str = "<stamped>";
const DOC: &str = "doc-1";
/// The object numbers upstream's local engine reserves at open
/// (`OBJECT_NUMBER_POOL_SIZE`), as `annotation_create` opens its fixture.
const OBJECT_NUMBER_POOL_SIZE: u32 = 64;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VectorFile {
    schema_version: u32,
    kind: String,
    fixture: String,
    cases: Vec<Case>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    covers: String,
    draft: Value,
    patch: Value,
    /// The bytes beside the draft, by role.
    #[serde(default)]
    draft_resources: BTreeMap<String, Vec<u8>>,
    /// The bytes beside the patch, by role.
    #[serde(default)]
    patch_resources: BTreeMap<String, Vec<u8>>,
    page_object_number: u32,
    #[serde(default)]
    updated: Value,
    #[serde(default)]
    has_modified: bool,
    #[serde(default)]
    appearance: Value,
    #[serde(default)]
    meta: Option<ExpectedMeta>,
    #[serde(default)]
    refused_with: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMeta {
    changed: Value,
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

fn case<'a>(file: &'a VectorFile, name: &str) -> &'a Case {
    file.cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

/// A clock a day later on every read, so the `/M` an update writes differs
/// from the one its create wrote.
#[derive(Debug)]
struct SteppingClock(Cell<i64>);

impl Clock for SteppingClock {
    fn now(&self) -> Timestamp {
        let now = self.0.get();
        self.0.set(now + 86_400_000);
        Timestamp::utc(now)
    }
}

fn host() -> Host {
    Host::new().with_determinism(
        Box::new(SteppingClock(Cell::new(1_700_000_000_000))),
        Box::new(epdf_engine::ids::SeededIdSource::new(7)),
    )
}

/// `bytes` open through `open.fatMem`, a layer over the bytes whose new
/// objects are numbered past the base file's (`annotation_create`'s `open`).
fn open_bytes(bytes: Vec<u8>) -> Replay {
    let mut host = host();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": DOC,
            "password": null, "reserveObjectNumbers": OBJECT_NUMBER_POOL_SIZE })
        .to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, DOC)
}

fn open(file: &VectorFile) -> Replay {
    open_bytes(test_support::fixture(&file.fixture))
}

/// `resources` as a request's role map and its payloads.
fn roles(resources: &BTreeMap<String, Vec<u8>>) -> (Option<Value>, Vec<Vec<u8>>) {
    let mut payloads = Vec::new();
    let mut map = serde_json::Map::new();
    for (role, bytes) in resources {
        map.insert(role.clone(), json!({ "payload": payloads.len() }));
        payloads.push(bytes.clone());
    }
    ((!map.is_empty()).then_some(Value::Object(map)), payloads)
}

/// Create `draft` on page `pon`: the record it answered.
fn create(
    replay: &mut Replay,
    draft: &Value,
    pon: u32,
    resources: &BTreeMap<String, Vec<u8>>,
) -> Value {
    let (roles, payloads) = roles(resources);
    let mut request = json!({ "kind": "annotations.create", "jobId": "<job>", "docId": DOC,
        "opId": "<op>", "page": page_ref(pon), "draft": draft });
    if let Some(roles) = roles {
        request["resources"] = roles;
    }
    let answer = replay.send(&request, payloads);
    let mut written = answer.outcome.unwrap_or_else(|e| panic!("create: {e}"));
    written["result"]["annotation"].take()
}

/// What one update answered.
struct Updated {
    outcome: Result<Value, Value>,
    event: Option<Value>,
}

/// Update `reference` with `patch`.
fn update(
    replay: &mut Replay,
    reference: &Value,
    patch: &Value,
    resources: &BTreeMap<String, Vec<u8>>,
) -> Updated {
    let (roles, payloads) = roles(resources);
    let mut request = json!({ "kind": "annotations.update", "jobId": "<job>", "docId": DOC,
        "opId": "<op>", "ref": reference, "patch": patch });
    if let Some(roles) = roles {
        request["resources"] = roles;
    }
    match replay.send(&request, payloads).outcome {
        Ok(mut written) => Updated {
            event: written.get_mut("event").map(Value::take),
            outcome: Ok(written["result"].take()),
        },
        Err(error) => Updated { outcome: Err(error), event: None },
    }
}

/// Create the case's subject, then patch it: the record the create answered
/// and what the update did.
fn run(replay: &mut Replay, case: &Case) -> (Value, Updated) {
    let created = create(replay, &case.draft, case.page_object_number, &case.draft_resources);
    let updated = update(replay, &created["ref"], &case.patch, &case.patch_resources);
    (created, updated)
}

/// `value` with its stamps as the generator records them: `createdAt` and a
/// UUID `nm` as `"<stamped>"`.
fn normalize_stamps(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(normalize_stamps).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, entry)| {
                    let stamped = (key == "createdAt" && entry.is_string())
                        || (key == "nm" && entry.as_str().is_some_and(is_uuid));
                    if stamped {
                        (key.clone(), Value::from(STAMPED))
                    } else {
                        (key.clone(), normalize_stamps(entry))
                    }
                })
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups.iter().all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn listed(replay: &mut Replay) -> Vec<Value> {
    let listed = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": DOC }))
        .expect("the document lists");
    listed["annotations"].as_array().expect("annotations").clone()
}

#[test]
fn an_updated_annotation_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert_eq!(file.cases.len(), 41, "the case list moved");
    for case in &file.cases {
        let mut replay = open(&file);
        let (created, updated) = run(&mut replay, case);
        if let Some(code) = &case.refused_with {
            let error = updated.outcome.expect_err(&case.name);
            assert_eq!(error["code"], json!(code), "{}: {error}", case.name);
            // Refused before the first write: the annotation reads as created.
            let now = listed(&mut replay);
            assert_eq!(now.last(), Some(&created), "{}: the refusal wrote", case.name);
            continue;
        }
        let result = updated.outcome.unwrap_or_else(|e| panic!("{}: update: {e}", case.name));
        let mut mine =
            result["annotation"].as_object().unwrap_or_else(|| panic!("{result}")).clone();
        let modified = mine.remove("modifiedAt").expect("the record carries modifiedAt");
        assert_eq!(
            case.has_modified,
            modified.as_str().is_some_and(|m| !m.is_empty()),
            "{}: /M presence differs",
            case.name,
        );
        // Every update refreshes `/M`, whatever the patch touched.
        assert_ne!(modified, created["modifiedAt"], "{}: /M was not refreshed", case.name);
        assert_eq!(
            test_support::canonical_numbers(&normalize_stamps(&Value::Object(mine))),
            test_support::canonical_numbers(&case.updated),
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn the_appearance_verdict_agrees_with_the_typescript_engine() {
    // The half a field comparison cannot see. Every verdict has to occur in
    // the list, or a classifier answering one thing always would pass.
    let file = load();
    let mut seen = BTreeMap::<String, usize>::new();
    for case in file.cases.iter().filter(|c| c.refused_with.is_none()) {
        let mut replay = open(&file);
        let (_, updated) = run(&mut replay, case);
        let result = updated.outcome.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        assert_eq!(result["appearance"], case.appearance, "{}: {}", case.name, case.covers);
        *seen
            .entry(case.appearance["action"].as_str().unwrap_or_default().to_owned())
            .or_default() += 1;
    }
    for action in ["preserved", "regenerated", "generation-unavailable"] {
        assert!(seen.get(action).is_some_and(|n| *n > 0), "no case is {action}: {seen:?}");
    }
}

#[test]
fn an_update_is_not_structural() {
    // An update writes the annotation in place: the meta of a one-op change
    // on its page, naming it, undoable; its event `annotations.updated`; and
    // the annotation where it was, under its name.
    let file = load();
    for case in file.cases.iter().filter(|c| c.refused_with.is_none()) {
        let mut replay = open(&file);
        let (created, updated) = run(&mut replay, case);
        let result = updated.outcome.unwrap_or_else(|e| panic!("{}: {e}", case.name));
        let meta = &result["meta"];
        assert_eq!(
            test_support::canonical_numbers(&meta["changed"]),
            test_support::canonical_numbers(&case.meta.as_ref().expect("a meta").changed),
            "{}: the changed list",
            case.name,
        );
        assert_eq!(
            meta["affectedPages"],
            json!([page_ref(case.page_object_number)]),
            "{}",
            case.name
        );
        assert_eq!(meta["undoable"], json!(true), "{}: {meta}", case.name);
        assert!(meta.get("weakRefsInvalidated").is_none(), "{}: an old meta key", case.name);
        assert!(meta.get("shouldRefetch").is_none(), "{}: an old meta key", case.name);
        assert_eq!(result["annotation"]["ref"], created["ref"], "{}: renamed", case.name);
        let event = updated.event.expect("an update publishes");
        assert_eq!(event["type"], "annotations.updated", "{}", case.name);
        assert_eq!(event["page"], page_ref(case.page_object_number), "{}", case.name);
        assert_eq!(event["annotation"], result["annotation"], "{}", case.name);
        assert_eq!(event["appearance"], result["appearance"], "{}", case.name);
        let now = listed(&mut replay);
        assert_eq!(now.last().map(|a| &a["ref"]), Some(&created["ref"]), "{}: moved", case.name);
    }
}

#[test]
fn an_update_keeps_the_annotations_name() {
    // Upstream names an annotation by its object number, or by its place on
    // the page when it was born inline (`baseIndex`), and no write stamps an
    // `/NM` to name one (bc0057f3a). An inline annotation updated keeps its
    // name and gains no `/NM`.
    let mut replay = Replay::open("tooling/engine-conformance/fixtures/annotation_base.pdf", DOC);
    let inline = listed(&mut replay)
        .into_iter()
        .find(|a| a["ref"]["kind"] == "baseIndex")
        .expect("the fixture has an annotation born inline");
    let updated =
        update(&mut replay, &inline["ref"], &json!({ "contents": "touched" }), &BTreeMap::new());
    let result = updated.outcome.expect("update");
    assert_eq!(result["annotation"]["ref"], inline["ref"]);
    assert_eq!(result["annotation"]["nm"], inline["nm"], "an update named it");
    assert_eq!(result["annotation"]["contents"], "touched");
}

#[test]
fn a_patch_for_the_wrong_subtype_is_refused() {
    // Before anything is written. A square patch carries fields a highlight
    // has no dictionary entries for, and applying the ones they share would
    // half-write an annotation nobody asked for.
    let file = load();
    let colour = case(&file, "colour-change");
    let mut replay = open(&file);
    let created = create(&mut replay, &colour.draft, colour.page_object_number, &BTreeMap::new());
    let refused = update(
        &mut replay,
        &created["ref"],
        &json!({ "subtype": "square", "opacity": 0.5 }),
        &BTreeMap::new(),
    )
    .outcome
    .expect_err("a square patch on a highlight");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("subtype cannot change"), "{refused}");

    // An empty quad list is refused: a markup marks something.
    let refused =
        update(&mut replay, &created["ref"], &json!({ "quadPoints": [] }), &BTreeMap::new())
            .outcome
            .expect_err("an empty quad list");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "quadPoints");

    // A field the kind doesn't declare is refused, never ignored.
    let refused =
        update(&mut replay, &created["ref"], &json!({ "vertices": [] }), &BTreeMap::new())
            .outcome
            .expect_err("a foreign field");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(listed(&mut replay).last(), Some(&created), "a refusal wrote");

    // A shorter list replaces the quads (`replaceQuadPoints`): upstream
    // removes `/QuadPoints` and writes the new list, so a list can shrink.
    let two = json!({ "subtype": "highlight", "quadPoints": [
        { "upperLeft": { "x": 10, "y": 10 }, "upperRight": { "x": 50, "y": 10 },
          "lowerLeft": { "x": 10, "y": 20 }, "lowerRight": { "x": 50, "y": 20 } },
        { "upperLeft": { "x": 10, "y": 30 }, "upperRight": { "x": 50, "y": 30 },
          "lowerLeft": { "x": 10, "y": 40 }, "lowerRight": { "x": 50, "y": 40 } } ] });
    let created = create(&mut replay, &two, colour.page_object_number, &BTreeMap::new());
    let shrunk = update(
        &mut replay,
        &created["ref"],
        &json!({ "quadPoints": [two["quadPoints"][0].clone()] }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a shorter list");
    assert_eq!(shrunk["annotation"]["quadPoints"].as_array().map(Vec::len), Some(1));
}

/// Run `name`'s case and hand the patched annotation, as the file holds it,
/// to `inspect`. A read can't show everything a writer decides: where the
/// reader normalizes, two different files read the same.
fn with_patched<R>(
    file: &VectorFile,
    name: &str,
    inspect: impl FnOnce(&pdfium::Annotation<'_>) -> R,
) -> R {
    let case = case(file, name);
    let mut replay = open(file);
    let (created, updated) = run(&mut replay, case);
    updated.outcome.unwrap_or_else(|e| panic!("{name}: {e}"));
    let object_number = created["ref"]["objectNumber"].as_u64().unwrap() as u32;
    let session = replay.host.sessions().session(&SessionKey::document(DOC)).expect("open");
    let owned = session.document().expect("a document");
    let document = owned.document();
    let page_index = (0..document.page_count())
        .find(|i| document.page_object_number_by_index(*i) == case.page_object_number)
        .expect("the page");
    let count = document.raw_annotation_count(page_index).expect("a count");
    let annotation = (0..count)
        .filter_map(|i| document.raw_annotation(page_index, i))
        .find(|a| a.object_number() == object_number)
        .expect("the patched annotation");
    inspect(&annotation)
}

#[test]
fn a_patched_review_state_is_written_in_the_spelling_the_pdf_uses() {
    // §12.5.6.3 spells these with a capital, and the reader passes an
    // unrecognized spelling through verbatim, so a lowercase `/State`
    // round-trips through this engine looking correct and means nothing to
    // Acrobat. Only the dictionary can tell the two apart.
    let file = load();
    with_patched(&file, "text-state-alone", |written| {
        assert_eq!(written.string_value("State").unwrap().as_deref(), Some("Accepted"));
        assert_eq!(written.string_value("StateModel").unwrap().as_deref(), Some("Review"));
    });
    with_patched(&file, "text-clear-state", |written| {
        assert_eq!(
            written.string_value("State").unwrap(),
            None,
            "a cleared /State is absent rather than empty"
        );
    });
}

#[test]
fn a_moved_stamp_refits_the_picture_it_already_had() {
    // The read carries the annotation's place and never the placement of
    // the picture inside it, so a stamp that moved and did not re-fit reads
    // back the same and renders in the wrong place. The 4×2 picture was
    // contained in an 80×80 box, a band 80×40; the box widened to 160×80
    // re-fits the drawing it already had, contained again, so it fills the
    // new box (`EPDFAnnot_UpdateAppearanceToRect`).
    let file = load();
    with_patched(&file, "stamp-move-only", |written| {
        let rect = written.rect();
        let bounds = written.object_bounds(0).expect("the drawing's bounds");
        let (width, height) = (rect.right - rect.left, rect.top - rect.bottom);
        assert!((width - 160.0).abs() < 0.5 && (height - 80.0).abs() < 0.5, "{rect:?}");
        assert!(
            ((bounds.right - bounds.left) - width).abs() < 0.5
                && ((bounds.top - bounds.bottom) - height).abs() < 0.5,
            "the drawing {bounds:?} does not fill the moved box {rect:?}",
        );
    });
}

#[test]
fn an_updated_drawn_rect_is_the_engines() {
    // `rect` on a drawn kind is a command (9d8ebf857): the one read, sent
    // back, moves nothing; another one moves the shape, and the rect is
    // worked out again from it. With a new shape too it is refused on
    // `rect`.
    let file = load();
    let square = case(&file, "square-resize");
    let mut replay = open(&file);
    let created = create(&mut replay, &square.draft, square.page_object_number, &BTreeMap::new());
    let kept =
        update(&mut replay, &created["ref"], &json!({ "rect": created["rect"] }), &BTreeMap::new())
            .outcome
            .expect("the rect sent back");
    assert_eq!(kept["appearance"]["action"], "preserved", "{kept}");
    let mut moved_rect = created["rect"].clone();
    moved_rect["x"] = json!(moved_rect["x"].as_f64().unwrap() + 20.0);
    let moved =
        update(&mut replay, &created["ref"], &json!({ "rect": moved_rect }), &BTreeMap::new())
            .outcome
            .expect("a moved rect");
    let box_x = |a: &Value| a["box"]["x"].as_f64().unwrap();
    assert!((box_x(&moved["annotation"]) - box_x(&created) - 20.0).abs() < 0.01, "{moved}");
    assert_eq!(moved["appearance"]["action"], "preserved", "a move keeps the drawing");
    // The rect read, sent back beside a new box, is no command: the box
    // and its turn are written.
    let turned = update(
        &mut replay,
        &created["ref"],
        &json!({ "rect": moved["annotation"]["rect"].clone(),
            "box": { "x": 0, "y": 0, "width": 5, "height": 5 }, "rotation": 30 }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a rect sent back drops");
    assert_eq!(turned["annotation"]["rotation"], 30.0, "{turned}");
    let refused = update(
        &mut replay,
        &created["ref"],
        &json!({ "rect": { "x": 1, "y": 1, "width": 10, "height": 10 },
            "box": { "x": 0, "y": 0, "width": 5, "height": 5 } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect_err("a new rect with a new box");
    assert_eq!(refused["details"]["field"], "rect", "{refused}");
}

#[test]
fn a_box_kinds_rect_differences_follow_its_drawing() {
    // `/RD` is the drawing's (runtime 5e9241945): a cloudy square's
    // appearance reaches past its box and writes it, a move keeps it, and a
    // resize redraws it. No patch field writes it (2bdc74fbf).
    let file = load();
    let cloudy = case(&file, "square-clear-cloudy");
    let mut replay = open(&file);
    let created = create(&mut replay, &cloudy.draft, cloudy.page_object_number, &BTreeMap::new());
    let refused = update(
        &mut replay,
        &created["ref"],
        &json!({ "rectDifferences": { "left": 1, "bottom": 1, "right": 1, "top": 1 } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect_err("no /RD field");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let wider =
        |a: &Value| a["rect"]["width"].as_f64().unwrap() > a["box"]["width"].as_f64().unwrap();
    assert!(wider(&created), "the bumps reach past the box: {created}");
    let mut moved_box = created["box"].clone();
    moved_box["x"] = json!(moved_box["x"].as_f64().unwrap() + 10.0);
    let moved =
        update(&mut replay, &created["ref"], &json!({ "box": moved_box }), &BTreeMap::new())
            .outcome
            .expect("a move");
    assert!(wider(&moved["annotation"]), "a move kept /RD: {moved}");
    let plain =
        update(&mut replay, &created["ref"], &json!({ "cloudyIntensity": null }), &BTreeMap::new())
            .outcome
            .expect("no bumps");
    assert_eq!(
        test_support::canonical_numbers(&plain["annotation"]["rect"]),
        test_support::canonical_numbers(&plain["annotation"]["box"]),
        "a plain square's rect is its box once redrawn"
    );
}

/// A scale, a shown or placed caption and a leader are written by an
/// update, the caption merged with the one the line has
/// (`measurementFollows`, `writeMeasurementFields`).
#[test]
fn a_measured_update_writes_each_measurement_field() {
    let file = load();
    let line = case(&file, "line-endpoints");
    let mut replay = open(&file);
    let created = create(&mut replay, &line.draft, line.page_object_number, &BTreeMap::new());
    let updated = |replay: &mut Replay, patch: Value| {
        update(replay, &created["ref"], &patch, &BTreeMap::new())
            .outcome
            .unwrap_or_else(|e| panic!("{patch}: {e}"))["annotation"]
            .take()
    };
    let shown = updated(&mut replay, json!({ "captionEnabled": true }));
    assert_eq!(
        (&shown["captionEnabled"], &shown["captionPosition"]),
        (&json!(true), &json!("inline"))
    );
    let top = updated(&mut replay, json!({ "captionPosition": "top" }));
    assert_eq!((&top["captionEnabled"], &top["captionPosition"]), (&json!(true), &json!("top")));
    let offset =
        updated(&mut replay, json!({ "captionOffset": { "along": 1, "perpendicular": 2 } }));
    assert_eq!(
        test_support::canonical_numbers(&offset["captionOffset"]),
        test_support::canonical_numbers(&json!({ "along": 1, "perpendicular": 2 }))
    );
    assert_eq!(offset["captionPosition"], "top", "the position is kept: {offset}");
    let leader = updated(&mut replay, json!({ "leader": { "length": 10 } }));
    assert_eq!(leader["leader"]["length"], json!(10.0), "{leader}");
    let scaled = updated(
        &mut replay,
        json!({ "measure": { "subtype": "rectilinear", "ratio": "1 in = 1 in",
            "x": [{ "unit": "in", "conversion": 0.013888889 }],
            "distance": [{ "unit": "in", "conversion": 1 }], "area": [] } }),
    );
    assert_eq!(scaled["measure"]["ratio"], "1 in = 1 in", "{scaled}");
    let cleared = updated(&mut replay, json!({ "measure": null, "leader": null }));
    assert_eq!(cleared["measure"], Value::Null, "{cleared}");
    assert_eq!(listed(&mut replay).last(), Some(&cleared), "the read is the listing's");
}

// ── `runColorConformance`'s update cases (color.test.ts) ───────────────────

fn pdf_of(objects: &[&str]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// `COLOR_FIXTURE_PDF`: a square whose border the file stores as gray
/// (`/C [0.5]`) and whose fill as CMYK (`/IC [0 0 0 1]`).
fn color_fixture() -> Replay {
    let mut host = host();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": DOC,
            "password": null })
        .to_string(),
        vec![pdf_of(&[
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>",
            "<< /Type /Annot /Subtype /Square /Rect [20 20 120 120] /F 4 /C [0.5] /IC [0 0 0 1] \
             /NM (colored) >>",
        ])],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, DOC)
}

/// The numbers of the last `/Key [ … ]` array in a saved file's text.
fn last_array(text: &str, key: &str) -> Option<Vec<f64>> {
    let marker = format!("/{key}");
    let mut found = None;
    let mut rest = text;
    while let Some(at) = rest.find(&marker) {
        let after = rest[at + marker.len()..].trim_start();
        if let Some(body) = after.strip_prefix('[')
            && let Some(end) = body.find(']')
        {
            found = Some(body[..end].split_whitespace().filter_map(|n| n.parse().ok()).collect());
        }
        rest = &rest[at + marker.len()..];
    }
    found
}

fn saved_text(replay: &mut Replay) -> String {
    let saved = replay.send(
        &json!({ "kind": "document.saveBuffer", "jobId": "<job>", "docId": DOC, "mode": "rewrite" }),
        Vec::new(),
    );
    saved.outcome.expect("save");
    String::from_utf8_lossy(saved.payloads.first().expect("the saved bytes")).into_owned()
}

#[test]
fn a_color_sent_back_unchanged_stays_as_the_file_stores_it() {
    let mut replay = color_fixture();
    let square = listed(&mut replay).into_iter().find(|a| a["nm"] == "colored").expect("square");
    // The whole read sent back, the colours in the other case, one change.
    let mut patch = square.clone();
    patch["color"] = json!(square["color"].as_str().unwrap().to_uppercase());
    patch["interiorColor"] = json!(square["interiorColor"].as_str().unwrap().to_uppercase());
    patch["contents"] = json!("kept");
    update(&mut replay, &square["ref"], &patch, &BTreeMap::new()).outcome.expect("update");
    let text = saved_text(&mut replay);
    assert_eq!(last_array(&text, "C"), Some(vec![0.5]));
    assert_eq!(last_array(&text, "IC"), Some(vec![0.0, 0.0, 0.0, 1.0]));
    let after = listed(&mut replay).into_iter().find(|a| a["nm"] == "colored").unwrap();
    assert_eq!(after["contents"], "kept");
    assert_eq!(after["color"], square["color"]);
}

#[test]
fn a_new_color_is_written_as_given_and_reads_back_lowercase() {
    let mut replay = color_fixture();
    let square = listed(&mut replay).into_iter().find(|a| a["nm"] == "colored").expect("square");
    // A patch that leaves out its subtype is its target's.
    let result =
        update(&mut replay, &square["ref"], &json!({ "color": "#1A2B3C" }), &BTreeMap::new())
            .outcome
            .expect("update");
    assert_eq!(result["annotation"]["color"], "#1a2b3c");
    assert_eq!(last_array(&saved_text(&mut replay), "C").map(|c| c.len()), Some(3));
}

// ── annotations-text-preservation.test.ts ──────────────────────────────────

const TEXT: &str = "Keep this text";

/// A free text with `/Contents` and `/DA` and no `/RC` (`legacy`), or a page
/// with none, on a 500×500 page.
fn text_fixture(legacy: bool, callout: bool) -> Vec<u8> {
    let annots = if legacy { "6 0 R" } else { "" };
    let intent =
        if callout { "FreeTextCallout /CL [90 190 120 240] /RD [40 20 0 0]" } else { "FreeText" };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /Resources << /Font << /Helv 5 0 R >> >> /Contents 4 0 R /Annots [{annots}] >>"
    );
    let annotation = format!(
        "<< /Type /Annot /Subtype /FreeText /Rect [80 180 380 280] /F 4 /Contents ({TEXT}) /DA (/Helv 18 Tf 0 0 0 rg) /Q 0 /IT /{intent} >>"
    );
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &page,
        "<< /Length 0 >>\nstream\n\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        &annotation,
    ])
}

fn expect_text(annotation: &Value, what: &str) {
    assert_eq!(annotation["contents"], TEXT, "{what}: {annotation}");
    let runs: String = annotation["richText"]["paragraphs"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|p| {
            p["runs"].as_array().unwrap().iter().map(|r| r["text"].as_str().unwrap().to_owned())
        })
        .collect();
    assert_eq!(runs, TEXT, "{what}: {annotation}");
}

#[test]
fn free_text_partial_updates_keep_its_text() {
    let rect = json!({ "x": 80, "y": 220, "width": 300, "height": 100 });
    for callout in [false, true] {
        for source in ["plain", "rich", "legacy"] {
            let mut replay = open_bytes(text_fixture(source == "legacy", callout));
            if source != "legacy" {
                let mut draft = json!({ "subtype": "free-text",
                    "intent": if callout { "free-text-callout" } else { "free-text" },
                    "box": rect, "fontFamily": "helvetica", "fontSize": 18, "textAlign": "left" });
                if callout {
                    draft["calloutLine"] = json!([{ "x": 90, "y": 310 }, { "x": 120, "y": 260 }]);
                }
                if source == "rich" {
                    draft["richText"] = json!({ "paragraphs": [{ "runs": [
                        { "text": "Keep " }, { "text": "this", "style": { "weight": 700 } },
                        { "text": " text" }] }] });
                } else {
                    draft["contents"] = json!(TEXT);
                }
                create(&mut replay, &draft, 3, &BTreeMap::new());
            }
            let mut patches = vec![
                json!({ "color": "#14283c" }),
                json!({ "fontSize": 20, "fontColor": "#002878" }),
                json!({ "fontFamily": "times-roman", "textAlign": "center" }),
                json!({ "interiorColor": "#f0f5fa", "opacity": 0.9 }),
                json!({ "box": { "x": 80, "y": 220, "width": 330, "height": 100 } }),
                json!({ "subject": "metadata-only" }),
            ];
            if callout {
                patches.push(
                    json!({ "calloutLine": [{ "x": 85, "y": 315 }, { "x": 120, "y": 260 }] }),
                );
            }
            for patch in patches {
                let reference = listed(&mut replay)[0]["ref"].clone();
                let what = format!("{source}, callout={callout}, {patch}");
                let result = update(&mut replay, &reference, &patch, &BTreeMap::new())
                    .outcome
                    .unwrap_or_else(|e| panic!("{what}: {e}"));
                expect_text(&result["annotation"], &what);
                expect_text(&listed(&mut replay)[0], &what);
            }
        }
        // An explicit empty `contents` still clears the text.
        let mut replay = open_bytes(text_fixture(true, callout));
        let reference = listed(&mut replay)[0]["ref"].clone();
        let result = update(
            &mut replay,
            &reference,
            &json!({ "subtype": "free-text", "contents": "" }),
            &BTreeMap::new(),
        )
        .outcome
        .expect("clear");
        assert_eq!(result["annotation"]["contents"], "", "callout={callout}");
    }
}

// ── annotations-rotation.test.ts and runAnnotationRotationConformance ──────

#[test]
fn a_rotated_box_moved_keeps_its_appearance_and_a_cleared_turn_reads_upright() {
    let file = load();
    let mut replay = open(&file);
    let page = file.cases[0].page_object_number;
    // A move of a turned box by its own turn is a translation: the drawing
    // is kept as it is.
    let circle = create(
        &mut replay,
        &json!({ "subtype": "circle", "box": { "x": 100, "y": 100, "width": 200, "height": 200 },
            "rotation": 45, "interiorColor": "#ffd500", "color": "#e5484d", "strokeWidth": 6 }),
        page,
        &BTreeMap::new(),
    );
    let moved = update(
        &mut replay,
        &circle["ref"],
        &json!({ "subtype": "circle", "box": { "x": 80, "y": 100, "width": 200, "height": 200 },
            "rotation": 45 }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a move");
    assert_eq!(moved["appearance"], json!({ "action": "preserved", "changed": false }));
    assert_eq!(moved["annotation"]["rotation"], 45.0);
    assert!((moved["annotation"]["box"]["x"].as_f64().unwrap() - 80.0).abs() < 0.01, "{moved}");

    // `rotation: null` clears the turn: the box reads upright where it was.
    let square = create(
        &mut replay,
        &json!({ "subtype": "square", "box": { "x": 60, "y": 60, "width": 100, "height": 100 },
            "rotation": 90 }),
        page,
        &BTreeMap::new(),
    );
    let cleared = update(
        &mut replay,
        &square["ref"],
        &json!({ "subtype": "square", "box": { "x": 60, "y": 60, "width": 100, "height": 100 },
            "rotation": null }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a cleared turn");
    assert_eq!(cleared["annotation"]["rotation"], Value::Null);
    assert_eq!(
        test_support::canonical_numbers(&cleared["annotation"]["box"]),
        test_support::canonical_numbers(&json!({ "x": 60, "y": 60, "width": 100, "height": 100 }))
    );
}

#[test]
fn a_rect_resizes_a_box_at_a_quarter_turn_along_its_own_sides() {
    // `runAnnotationRotationConformance`: a box turned a quarter stands
    // with its sides swapped, and a taller rect lengthens its own width.
    let file = load();
    let mut replay = open(&file);
    let page = file.cases[0].page_object_number;
    let created = create(
        &mut replay,
        &json!({ "subtype": "square", "box": { "x": 100, "y": 100, "width": 100, "height": 50 },
            "rotation": 90 }),
        page,
        &BTreeMap::new(),
    );
    let mut rect = created["rect"].clone();
    rect["height"] = json!(rect["height"].as_f64().unwrap() * 2.0);
    let resized = update(&mut replay, &created["ref"], &json!({ "rect": rect }), &BTreeMap::new())
        .outcome
        .expect("a resize");
    let content = &resized["annotation"]["box"];
    assert!((content["width"].as_f64().unwrap() - 200.0).abs() < 0.5, "{resized}");
    assert!((content["height"].as_f64().unwrap() - 50.0).abs() < 0.5, "{resized}");
    assert_eq!(resized["annotation"]["rotation"], 90.0);
}

#[test]
fn turning_a_drawn_kind_redraws_it() {
    // 6153c181e: a point kind's points are upright and `rotation` turns
    // them, so a new turn is a new drawing (`ink-advisory-rotation`).
    let file = load();
    let ink = case(&file, "ink-intent");
    let mut replay = open(&file);
    let created = create(&mut replay, &ink.draft, ink.page_object_number, &BTreeMap::new());
    let turned = update(&mut replay, &created["ref"], &json!({ "rotation": 45 }), &BTreeMap::new())
        .outcome
        .expect("a turn");
    assert_eq!(turned["appearance"]["action"], "regenerated");
    assert_eq!(turned["annotation"]["rotation"], 45.0);
    // The points read back upright, as they were sent, within what a turn
    // through `f32` keeps.
    let points = |a: &Value| -> Vec<f64> {
        a["inkList"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| s.as_array().unwrap().iter())
            .flat_map(|p| [p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap()])
            .collect()
    };
    for (got, sent) in points(&turned["annotation"]).into_iter().zip(points(&created)) {
        assert!((got - sent).abs() < 1e-3, "the points read back upright: {turned}");
    }
}

// ── The change op ───────────────────────────────────────────────────────────

#[test]
fn an_update_op_applies_in_page_space_and_its_undo_puts_back_what_still_shows_it() {
    let file = load();
    let colour = case(&file, "colour-change");
    let mut replay = open(&file);
    let created = create(&mut replay, &colour.draft, colour.page_object_number, &BTreeMap::new());
    let applied = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-a",
            "change": { "ops": [{ "type": "annotations.update", "ref": created["ref"],
                "patch": { "color": "#0000ff", "contents": "edited" } }] } }))
        .expect("apply");
    let item = &applied["result"]["items"][0];
    assert_eq!(item["type"], "annotations.update", "{applied}");
    assert_eq!(item["annotation"]["color"], "#0000ff");
    assert!(item["annotation"]["rect"]["width"].is_number(), "page space: {item}");
    assert_eq!(item["appearance"]["action"], "regenerated");

    // Someone changes the colour again: the undo puts back only the
    // contents, which still show the update, and reports the colour.
    update(&mut replay, &created["ref"], &json!({ "color": "#00ff00" }), &BTreeMap::new())
        .outcome
        .expect("a later change");
    let undone = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-b",
            "change": { "undoOf": "op-a" } }))
        .expect("undo");
    let item = &undone["result"]["items"][0];
    assert_eq!(item["type"], "annotations.update", "{undone}");
    assert_eq!(item["skipped"], json!(["color"]), "{undone}");
    assert_eq!(item["annotation"]["contents"], created["contents"]);
    assert_eq!(item["annotation"]["color"], "#00ff00");
}

#[test]
fn an_update_op_on_a_widget_or_a_wrong_expectation_is_refused() {
    let file = load();
    let colour = case(&file, "colour-change");
    let mut replay = open(&file);
    let created = create(&mut replay, &colour.draft, colour.page_object_number, &BTreeMap::new());
    let conflict = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-c",
            "change": { "ops": [{ "type": "annotations.update", "ref": created["ref"],
                "patch": { "color": "#0000ff" }, "expect": { "color": "#123456" } }] } }))
        .expect_err("a wrong expectation");
    assert_eq!(conflict["code"], "ChangeConflict", "{conflict}");
    assert_eq!(listed(&mut replay).last(), Some(&created), "a refused change wrote");
}

// ── annotations-stamp-resize.test.ts ───────────────────────────────────────

const BANDS: &str =
    "1 0 0 rg 0 0 50 100 re f 0 1 0 rg 50 0 100 100 re f 0 0 1 rg 150 0 50 100 re f";

fn stream(dict: &str, content: &str) -> String {
    format!("<< {dict} /Length {} >>\nstream\n{content}\nendstream", content.len())
}

/// Two stamps whose drawing is three vertical bands (red, green, blue, the
/// green twice as wide), so a wrong crop shows. `shared` gives the second
/// stamp the first's stream, `/AP` dictionary or states; `vertical` turns the
/// drawing a quarter inside its form.
fn stamp_fixture(shared: Option<&str>, vertical: bool) -> Vec<u8> {
    let size = if vertical { "100 200" } else { "200 100" };
    let content =
        if vertical { format!("q 0 1 -1 0 100 0 cm {BANDS} Q") } else { BANDS.to_owned() };
    let ap =
        if matches!(shared, Some("dictionary" | "states")) { "8 0 R" } else { "<< /N 6 0 R >>" };
    let rect = if vertical { "110 210" } else { "210 110" };
    let second = if shared.is_some() { "7 0 R" } else { "" };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 600] /Resources << >> /Contents 4 0 R /Annots [5 0 R {second}] >>"
    );
    let first =
        format!("<< /Type /Annot /Subtype /Stamp /Rect [10 10 {rect}] /F 4 /AS /On /AP {ap} >>");
    let form = stream(
        &format!("/Type /XObject /Subtype /Form /BBox [0 0 {size}] /Resources << >>"),
        &content,
    );
    let other =
        format!("<< /Type /Annot /Subtype /Stamp /Rect [300 10 500 110] /F 4 /AS /On /AP {ap} >>");
    let states = if shared == Some("states") {
        "<< /N 9 0 R /R 6 0 R >>"
    } else {
        "<< /N 6 0 R /R 6 0 R >>"
    };
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        &page,
        &stream("", ""),
        &first,
        &form,
        &other,
        states,
        "<< /On 6 0 R /Off 6 0 R >>",
        &form,
    ])
}

/// The `index`-th annotation's look at rest, in the state it shows:
/// `(width, height, stride, pixels)`.
fn stamp_appearance(replay: &mut Replay, index: usize) -> (usize, usize, usize, Vec<u8>) {
    let annotation = listed(replay)[index].clone();
    let answer = replay.send(
        &json!({ "kind": "annotations.renderAppearances", "effect": "read", "jobId": "<job>",
            "docId": DOC, "page": page_ref(3), "family": "annotations" }),
        Vec::new(),
    );
    let result = answer.outcome.expect("the appearances render");
    let appearances = result["result"]["appearances"].as_array().expect("appearances");
    let (at, shown) = appearances
        .iter()
        .enumerate()
        .find(|(_, a)| {
            a["ref"] == annotation["ref"]
                && a["mode"] == "normal"
                && (a["state"].is_null() || a["state"] == annotation["appearanceState"])
        })
        .unwrap_or_else(|| panic!("no appearance shown for annotation {index}: {result}"));
    let raster = &shown["raster"];
    let number = |key: &str| raster[key].as_u64().expect(key) as usize;
    (number("width"), number("height"), number("stride"), answer.payloads[at].clone())
}

fn pixel(raster: &(usize, usize, usize, Vec<u8>), x: f64, y: f64) -> [u8; 4] {
    let (width, height, stride, data) = raster;
    let at =
        (y * *height as f64).floor() as usize * stride + (x * *width as f64).floor() as usize * 4;
    [data[at], data[at + 1], data[at + 2], data[at + 3]]
}

#[test]
fn a_cover_fit_centres_the_crop() {
    for vertical in [false, true] {
        let mut replay = open_bytes(stamp_fixture(None, vertical));
        let reference = listed(&mut replay)[0]["ref"].clone();
        update(
            &mut replay,
            &reference,
            &json!({ "subtype": "stamp", "fit": "cover",
                "box": { "x": 10, "y": 10, "width": 100, "height": 100 } }),
            &BTreeMap::new(),
        )
        .outcome
        .expect("a cover fit");
        let raster = stamp_appearance(&mut replay, 0);
        assert_eq!((raster.0, raster.1), (100, 100), "vertical={vertical}");
        for (x, y) in [(0.1, 0.5), (0.9, 0.5), (0.5, 0.1), (0.5, 0.9)] {
            assert_eq!(pixel(&raster, x, y), [0, 255, 0, 255], "vertical={vertical} at {x},{y}");
        }
    }
}

#[test]
fn resizing_a_stamp_keeps_a_sibling_sharing_its_drawing() {
    for shared in ["stream", "dictionary", "states"] {
        let mut replay = open_bytes(stamp_fixture(Some(shared), false));
        let reference = listed(&mut replay)[0]["ref"].clone();
        let before = stamp_appearance(&mut replay, 1);
        update(
            &mut replay,
            &reference,
            &json!({ "subtype": "stamp", "fit": "contain",
                "box": { "x": 10, "y": 10, "width": 100, "height": 100 } }),
            &BTreeMap::new(),
        )
        .outcome
        .expect("a resize");
        let sibling = stamp_appearance(&mut replay, 1);
        assert_eq!(sibling, before, "{shared}: the sibling changed");
        let resized = stamp_appearance(&mut replay, 0);
        assert_eq!((resized.0, resized.1), (100, 100), "{shared}");
        assert_eq!(pixel(&resized, 0.5, 0.1)[3], 0, "{shared}: contained, the top is empty");
        assert_eq!(pixel(&resized, 0.5, 0.5), [0, 255, 0, 255], "{shared}");
    }
}

// ── runAnnotationRotationConformance: what another app turned ──────────────

fn close_to(a: &Value, b: &Value, within: f64) -> bool {
    ["x", "y", "width", "height"]
        .iter()
        .all(|k| (a[*k].as_f64().unwrap() - b[*k].as_f64().unwrap()).abs() <= within)
}

#[test]
fn a_change_to_a_text_box_acrobat_turned_keeps_it_turned() {
    let mut replay =
        Replay::open("packages/engine/main/test/fixtures/freetext-rotated-acrobat.pdf", DOC);
    let turned = listed(&mut replay)
        .into_iter()
        .find(|a| a["contents"] == "test 123")
        .expect("the turned text box");
    let result = update(
        &mut replay,
        &turned["ref"],
        &json!({ "subtype": "free-text", "color": "#c8e6ff" }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a recolour");
    let updated = &result["annotation"];
    assert_eq!(updated["rotation"], 270.0, "{updated}");
    assert!(close_to(&updated["box"], &turned["box"], 0.01), "{updated}");
    assert!(close_to(&updated["rect"], &turned["rect"], 0.01), "{updated}");
}

#[test]
fn moving_a_stamp_acrobat_turned_turns_it_once() {
    let mut replay =
        Replay::open("packages/engine/main/test/fixtures/stamp-rotated-acrobat.pdf", DOC);
    let stamp = listed(&mut replay)
        .into_iter()
        .find(|a| a["subtype"] == "stamp")
        .expect("the turned stamp");
    let mut moved = stamp["box"].clone();
    moved["x"] = json!(moved["x"].as_f64().unwrap() + 40.0);
    let result = update(
        &mut replay,
        &stamp["ref"],
        &json!({ "subtype": "stamp", "box": moved, "rotation": stamp["rotation"] }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a move");
    let updated = &result["annotation"];
    assert_eq!(updated["rotation"], 36.0, "{updated}");
    assert!(close_to(&updated["box"], &moved, 0.02), "{updated}");
    let mut rect = stamp["rect"].clone();
    rect["x"] = json!(rect["x"].as_f64().unwrap() + 40.0);
    assert!(close_to(&updated["rect"], &rect, 0.02), "{updated}");
}

// ── runAnnotationMutationConformance's update cases ────────────────────────

fn free_text(replay: &mut Replay, extra: Value) -> Value {
    let mut draft = json!({ "subtype": "free-text", "intent": "free-text",
        "box": { "x": 72, "y": 72, "width": 200, "height": 60 }, "fontFamily": "helvetica",
        "fontSize": 14, "textAlign": "left", "color": "#000000" });
    for (k, v) in extra.as_object().unwrap() {
        draft[k] = v.clone();
    }
    let page = load().cases[0].page_object_number;
    create(replay, &draft, page, &BTreeMap::new())
}

#[test]
fn a_paragraphs_only_patch_keeps_the_body_and_contents_rewrite_body_style_paragraphs() {
    let mut replay = open(&load());
    let created = free_text(
        &mut replay,
        json!({ "textAlign": "center", "richText": { "body": { "family": "Helvetica", "size": 14 },
            "paragraphs": [{ "runs": [{ "text": "a" }, { "text": "b", "style": { "weight": 700 } }] }] } }),
    );
    // The editor's commit: paragraphs only, no body. The body, its
    // alignment with it, stays what the annotation had.
    let restyled = update(
        &mut replay,
        &created["ref"],
        &json!({ "richText": { "paragraphs": [{ "runs": [
            { "text": "ab", "style": { "italic": true } }, { "text": "c" }] }] } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a paragraphs-only patch")["annotation"]
        .clone();
    assert_eq!(restyled["contents"], "abc");
    assert_eq!(restyled["richText"]["body"]["size"], 14.0);
    assert_eq!(restyled["richText"]["body"]["align"], "center");
    assert_eq!(restyled["textAlign"], "center");
    assert_eq!(
        restyled["richText"]["paragraphs"][0]["runs"][0],
        json!({ "text": "ab", "style": { "italic": true } })
    );
    // A plain-text client's rewrite: body-style paragraphs, one per line
    // break, run formatting gone by design.
    let rewritten =
        update(&mut replay, &created["ref"], &json!({ "contents": "one\rtwo" }), &BTreeMap::new())
            .outcome
            .expect("a contents rewrite")["annotation"]
            .clone();
    assert_eq!(rewritten["contents"], "one\rtwo");
    assert_eq!(
        rewritten["richText"]["paragraphs"],
        json!([{ "runs": [{ "text": "one" }] }, { "runs": [{ "text": "two" }] }])
    );
    assert_eq!(rewritten["richText"]["body"]["size"], 14.0);
}

#[test]
fn a_text_style_patch_moves_the_body_while_runs_keep_their_deltas() {
    let mut replay = open(&load());
    let created = free_text(
        &mut replay,
        json!({ "richText": { "body": { "family": "Helvetica", "size": 14 },
            "paragraphs": [{ "runs": [{ "text": "a" }, { "text": "b", "style": { "size": 30 } }] }] } }),
    );
    let updated = update(
        &mut replay,
        &created["ref"],
        &json!({ "fontSize": 20, "fontColor": "#ff0000", "fontFamily": "times-bold" }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a style patch")["annotation"]
        .clone();
    assert_eq!(updated["fontSize"], 20.0);
    assert_eq!(updated["fontFamily"], "times-bold");
    assert_eq!(updated["richText"]["body"]["size"], 20.0);
    assert_eq!(updated["richText"]["body"]["color"], "#ff0000");
    assert_eq!(updated["richText"]["body"]["family"], "Times");
    assert_eq!(updated["richText"]["body"]["weight"], 700);
    // A run's own size is a delta over the body: it survives the move.
    assert_eq!(
        updated["richText"]["paragraphs"][0]["runs"][1],
        json!({ "text": "b", "style": { "size": 30 } })
    );
}

#[test]
fn contents_and_rich_text_that_disagree_are_refused() {
    let mut replay = open(&load());
    let created = free_text(&mut replay, json!({ "contents": "x" }));
    let refused = update(
        &mut replay,
        &created["ref"],
        &json!({ "contents": "nope", "richText": { "paragraphs": [{ "runs": [{ "text": "yes" }] }] } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect_err("a disagreement");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let agreed = update(
        &mut replay,
        &created["ref"],
        &json!({ "contents": "yes", "richText": { "paragraphs": [{ "runs": [{ "text": "yes" }] }] } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("an agreement");
    assert_eq!(agreed["annotation"]["contents"], "yes");
}

#[test]
fn a_partial_default_appearance_patch_keeps_the_members_it_leaves_out() {
    // `/DA` packs the font, size and colour into one string: a size-only
    // patch keeps the font and the border colour, a colour-only one the
    // font and the size.
    let mut replay = open(&load());
    let created = free_text(
        &mut replay,
        json!({ "contents": "DA read-modify-write", "fontFamily": "times-roman", "color": "#c80000" }),
    );
    assert_eq!(created["fontFamily"], "times-roman");
    let sized = update(&mut replay, &created["ref"], &json!({ "fontSize": 18 }), &BTreeMap::new())
        .outcome
        .expect("a size")["annotation"]
        .clone();
    assert_eq!(sized["fontFamily"], "times-roman");
    assert_eq!(sized["fontSize"], 18.0);
    assert_eq!(sized["color"], "#c80000");
    let recoloured =
        update(&mut replay, &created["ref"], &json!({ "color": "#0000c8" }), &BTreeMap::new())
            .outcome
            .expect("a colour")["annotation"]
            .clone();
    assert_eq!(recoloured["fontFamily"], "times-roman");
    assert_eq!(recoloured["fontSize"], 18.0);
    assert_eq!(recoloured["color"], "#0000c8");
}

#[test]
fn a_reply_patched_to_null_is_top_level_again() {
    let file = load();
    let page = file.cases[0].page_object_number;
    let quad = case(&file, "colour-change").draft["quadPoints"].clone();
    let mut replay = open(&file);
    let parent = create(
        &mut replay,
        &json!({ "subtype": "highlight", "quadPoints": quad }),
        page,
        &BTreeMap::new(),
    );
    let reply = create(
        &mut replay,
        &json!({ "subtype": "highlight", "quadPoints": quad, "reply": { "to": parent["ref"] } }),
        page,
        &BTreeMap::new(),
    );
    assert_eq!(reply["reply"]["type"], "reply");
    let cleared = update(&mut replay, &reply["ref"], &json!({ "reply": null }), &BTreeMap::new())
        .outcome
        .expect("a cleared reply");
    assert_eq!(cleared["annotation"]["reply"], Value::Null);
    // And linked again, as a group member.
    let grouped = update(
        &mut replay,
        &reply["ref"],
        &json!({ "reply": { "to": parent["ref"], "type": "group" } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a relink");
    assert_eq!(grouped["annotation"]["reply"], json!({ "to": parent["ref"], "type": "group" }));
}

#[test]
fn a_note_and_its_popup_share_open_and_the_change_names_both() {
    let file = load();
    let page = file.cases[0].page_object_number;
    let mut replay = open(&file);
    let note = create(
        &mut replay,
        &json!({ "subtype": "text", "rect": { "x": 72, "y": 72, "width": 20, "height": 20 },
            "open": true }),
        page,
        &BTreeMap::new(),
    );
    let popup = create(
        &mut replay,
        &json!({ "subtype": "popup", "rect": { "x": 100, "y": 72, "width": 120, "height": 80 },
            "parent": note["ref"] }),
        page,
        &BTreeMap::new(),
    );
    let closed = update(&mut replay, &note["ref"], &json!({ "open": false }), &BTreeMap::new())
        .outcome
        .expect("a closed note");
    assert_eq!(closed["annotation"]["open"], false);
    assert_eq!(closed["meta"]["changed"], json!([note["ref"], popup["ref"]]), "{closed}");
    let shown = listed(&mut replay).into_iter().find(|a| a["ref"] == popup["ref"]).unwrap();
    assert_eq!(shown["open"], false, "the popup's /Open follows its note's");
    // Opened from the popup, the note opens too.
    let opened = update(&mut replay, &popup["ref"], &json!({ "open": true }), &BTreeMap::new())
        .outcome
        .expect("an opened popup");
    assert_eq!(opened["meta"]["changed"], json!([popup["ref"], note["ref"]]), "{opened}");
    let shown = listed(&mut replay).into_iter().find(|a| a["ref"] == note["ref"]).unwrap();
    assert_eq!(shown["open"], true);
}

// ── Fix round 1 ─────────────────────────────────────────────────────────────

#[test]
fn an_update_ops_expectation_compares_values_not_spellings() {
    // `valuesEqual`: the read's `1.0` is the expectation's `1`, and a `box`
    // the read gave, converted back, is the box the annotation has.
    let file = load();
    let square = case(&file, "square-resize");
    let mut replay = open(&file);
    let created = create(&mut replay, &square.draft, square.page_object_number, &BTreeMap::new());
    let applied = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-e",
            "change": { "ops": [{ "type": "annotations.update", "ref": created["ref"],
                "patch": { "color": "#0000ff" },
                "expect": { "opacity": 1, "strokeWidth": 1, "box": created["box"] } }] } }))
        .expect("the expectation holds");
    assert_eq!(applied["result"]["items"][0]["annotation"]["color"], "#0000ff");
}

#[test]
fn a_file_sent_back_renamed_is_renamed_and_its_undo_restores_the_annotation() {
    // `AttachmentFileWriteSchema` ignores what a read adds (size, checksum,
    // dates), so the whole `file` a read gives can be sent back changed.
    let file = load();
    let attachment = case(&file, "file-attachment-icon");
    let mut replay = open(&file);
    let created = create(
        &mut replay,
        &attachment.draft,
        attachment.page_object_number,
        &attachment.draft_resources,
    );
    assert_eq!(created["file"]["name"], "notes.txt", "{created}");
    let mut renamed = created["file"].clone();
    renamed["name"] = json!("renamed.txt");
    renamed["description"] = json!("described");
    let applied = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-f",
            "change": { "ops": [{ "type": "annotations.update", "ref": created["ref"],
                "patch": { "file": renamed } }] } }))
        .expect("a rename");
    let item = &applied["result"]["items"][0]["annotation"];
    assert_eq!(item["file"]["name"], "renamed.txt", "{item}");
    assert_eq!(item["file"]["description"], "described", "{item}");
    assert_eq!(item["file"]["size"], created["file"]["size"], "the bytes stay: {item}");

    // The undo puts the annotation's dictionary back from its capture
    // (`revertObjects`). The rename was written into the file
    // specification, which the layer already held a version of; the fork's
    // import puts back only the objects under `/AP` and `/FS` the layer holds
    // no version of (`EPDFDoc_ImportDictRaw`), so the specification keeps
    // the rename, as upstream's undo leaves it.
    let undone = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC, "opId": "op-g",
            "change": { "undoOf": "op-f" } }))
        .expect("the undo");
    let item = &undone["result"]["items"][0];
    assert_eq!(item["type"], "annotations.update", "{undone}");
    assert_eq!(item["appearance"], json!({ "action": "restored", "changed": false }), "{undone}");
    assert_eq!(item["annotation"]["file"]["name"], "renamed.txt", "{undone}");
}

#[test]
fn an_undone_update_puts_the_dictionary_back_exactly_and_says_restored() {
    // `revertAnnotation`: while the annotation reads as the update left it,
    // its dictionaries come back from their captures, and the item says
    // `restored`. Its redo is the update again, from the captures the undo
    // took.
    let file = load();
    let square = case(&file, "square-resize");
    let mut replay = open(&file);
    let created = create(&mut replay, &square.draft, square.page_object_number, &BTreeMap::new());
    let apply = |replay: &mut common::replay::Replay, op_id: &str, change: Value| {
        replay
            .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
                "opId": op_id, "change": change }))
            .unwrap_or_else(|e| panic!("{op_id}: {e}"))
    };
    let updated = apply(
        &mut replay,
        "op-u",
        json!({ "ops": [{ "type": "annotations.update", "ref": created["ref"],
            "patch": { "color": "#0000ff", "contents": "changed" } }] }),
    );
    assert_eq!(updated["result"]["items"][0]["annotation"]["color"], "#0000ff");

    let undone = apply(&mut replay, "op-v", json!({ "undoOf": "op-u" }));
    let item = &undone["result"]["items"][0];
    assert_eq!(item["appearance"]["action"], "restored", "{undone}");
    assert_eq!(item["appearance"]["changed"], true, "the update redrew it: {undone}");
    let mut back = item["annotation"].clone();
    let mut before = created.clone();
    for value in [&mut back, &mut before] {
        value.as_object_mut().unwrap().remove("ref");
    }
    assert_eq!(back, before, "the annotation did not come back as it was");

    let redone = apply(&mut replay, "op-w", json!({ "undoOf": "op-v" }));
    let item = &redone["result"]["items"][0];
    assert_eq!(item["appearance"]["action"], "restored", "{redone}");
    assert_eq!(item["annotation"]["color"], "#0000ff", "{redone}");
    assert_eq!(item["annotation"]["contents"], "changed", "{redone}");
}

#[test]
fn an_update_changed_since_is_undone_by_value_for_what_still_shows_it() {
    // When someone changed the annotation since, the dictionaries no longer
    // read as the update left them: the undo puts back, by value, the fields
    // that still hold what the update set, and names the rest skipped.
    let file = load();
    let square = case(&file, "square-resize");
    let mut replay = open(&file);
    let created = create(&mut replay, &square.draft, square.page_object_number, &BTreeMap::new());
    let apply = |replay: &mut common::replay::Replay, op_id: &str, change: Value| {
        replay
            .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
                "opId": op_id, "change": change }))
            .unwrap_or_else(|e| panic!("{op_id}: {e}"))
    };
    apply(
        &mut replay,
        "op-u",
        json!({ "ops": [{ "type": "annotations.update", "ref": created["ref"],
            "patch": { "color": "#0000ff", "contents": "changed" } }] }),
    );
    apply(
        &mut replay,
        "op-x",
        json!({ "ops": [{ "type": "annotations.update", "ref": created["ref"],
            "patch": { "contents": "someone else" } }] }),
    );
    let undone = apply(&mut replay, "op-v", json!({ "undoOf": "op-u" }));
    let item = &undone["result"]["items"][0];
    assert_ne!(item["appearance"]["action"], "restored", "{undone}");
    assert_eq!(item["annotation"]["color"], created["color"], "{undone}");
    assert_eq!(item["annotation"]["contents"], "someone else", "{undone}");
    assert_eq!(item["skipped"], json!(["contents"]), "{undone}");
}

#[test]
fn a_patch_without_its_subtype_converts_its_places_by_name() {
    // A patch that leaves out its kind converts each field by what its name
    // holds in every kind (`ANNOTATION_FIELD_SPACES_BY_NAME`); a box in the
    // file's shape is no page-space place (`notInPageSpace`).
    let file = load();
    let square = case(&file, "square-rect-translation");
    let mut replay = open(&file);
    let created = create(&mut replay, &square.draft, square.page_object_number, &BTreeMap::new());
    let moved = update(
        &mut replay,
        &created["ref"],
        &json!({ "box": { "x": 330, "y": 147, "width": 100, "height": 60 } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect("a subtype-less move");
    assert_eq!(
        test_support::canonical_numbers(&moved["annotation"]["box"]),
        test_support::canonical_numbers(&json!({ "x": 330, "y": 147, "width": 100, "height": 60 }))
    );
    let refused = update(
        &mut replay,
        &created["ref"],
        &json!({ "box": { "left": 0, "bottom": 0, "right": 10, "top": 10 } }),
        &BTreeMap::new(),
    )
    .outcome
    .expect_err("a box in the file's shape");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "box", "{refused}");
}
