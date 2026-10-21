//! G11's first mutator gate: a created annotation is the same annotation.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-create
//!
//! # The read is the assertion
//!
//! Each case's draft is the v3 page-space draft the generator sent, replayed
//! as the wire request through [`Host::call`], with its bytes as payloads by
//! role. What is compared is the annotation the create answered, which a
//! create reads back as a listing reads it: anything PDFium normalized on the
//! way in shows up on both sides or on neither.
//!
//! The stamps are left out of the comparison, as the generator leaves them
//! out (`normalizeWriteStamps`): `modifiedAt` is `/M` from the clock (its
//! presence is asserted), `createdAt` is `/CreationDate` and a file's
//! creation date, and a create with no name of its own gets a fresh UUIDv7
//! `/NM`. Each reads `"<stamped>"`, and its shape is asserted here.

mod common;

use common::replay::Replay;
use epdf_engine::host::Host;
use epdf_engine::session::registry::SessionKey;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;
const STAMPED: &str = "<stamped>";
const DOC: &str = "doc-1";
/// The object numbers upstream's local engine reserves at open for the
/// creates its main thread names (`OBJECT_NUMBER_POOL_SIZE`): a create that
/// names none is numbered past them.
const OBJECT_NUMBER_POOL_SIZE: u32 = 64;

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
    draft: Value,
    /// The bytes beside the draft, by role (`appearance`, `file`).
    #[serde(default)]
    resources: std::collections::BTreeMap<String, Vec<u8>>,
    page_object_number: u32,
    #[serde(default)]
    created: Value,
    #[serde(default)]
    has_modified: bool,
    #[serde(default)]
    meta: Option<ExpectedMeta>,
    #[serde(default)]
    refused_with: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedMeta {
    changed: Value,
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

fn case<'a>(file: &'a VectorFile, name: &str) -> &'a Case {
    file.cases.iter().find(|c| c.name == name).unwrap_or_else(|| panic!("no case {name}"))
}

fn page_ref(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

/// The wire request for a draft on page `pon`, with `resources` as the
/// request's payloads, by role.
fn request(
    draft: &Value,
    pon: u32,
    resources: &std::collections::BTreeMap<String, Vec<u8>>,
) -> (Value, Vec<Vec<u8>>) {
    let mut payloads = Vec::new();
    let mut roles = serde_json::Map::new();
    for (role, bytes) in resources {
        roles.insert(role.clone(), json!({ "payload": payloads.len() }));
        payloads.push(bytes.clone());
    }
    let mut request = json!({ "kind": "annotations.create", "jobId": "<job>", "docId": DOC,
        "page": page_ref(pon), "draft": draft });
    if !roles.is_empty() {
        request["resources"] = Value::Object(roles);
    }
    (request, payloads)
}

/// What one create answered.
struct Created {
    outcome: Result<Value, Value>,
    event: Option<Value>,
}

/// The fixture open as upstream's `open.fatMem` opens it: a layer over the
/// bytes (`openBytesAsLayer`), whose new objects are numbered past the base
/// file's. This engine opens `open.fatMem` as a plain document until Task
/// 8.29, which numbers a new object from the file's first free number, so
/// the layer is opened by name here.
fn open(file: &VectorFile) -> Replay {
    let mut host = Host::new();
    let opened = host.call(
        &json!({ "kind": "open.layerMemBase", "effect": "open", "jobId": 1, "docId": DOC,
            "baseKey": "base", "layer": { "kind": "fresh" }, "password": null,
            "reserveObjectNumbers": OBJECT_NUMBER_POOL_SIZE })
        .to_string(),
        vec![test_support::fixture(&file.fixture)],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    Replay::adopt(host, DOC)
}

fn create_in(replay: &mut Replay, case: &Case) -> Created {
    create_draft(replay, &case.draft, case.page_object_number, &case.resources)
}

fn create_draft(
    replay: &mut Replay,
    draft: &Value,
    pon: u32,
    resources: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Created {
    let (request, payloads) = request(draft, pon, resources);
    let answer = replay.send(&request, payloads);
    // A write answers its result beside its sequence and its event.
    match answer.outcome {
        Ok(mut written) => Created {
            event: written.get_mut("event").map(Value::take),
            outcome: Ok(written["result"].take()),
        },
        Err(error) => Created { outcome: Err(error), event: None },
    }
}

/// `value` with its stamps as the generator records them: `createdAt` and a
/// UUID `nm` as `"<stamped>"` (`normalizeWriteStamps`).
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

/// A UUID's canonical form (`8-4-4-4-12` lowercase hex).
fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
}

#[test]
fn a_created_annotation_reads_back_as_the_typescript_engines_did() {
    let file = load();
    assert_eq!(file.cases.len(), 57, "the case list moved");
    for case in &file.cases {
        let mut replay = open(&file);
        let created = create_in(&mut replay, case);
        if let Some(code) = &case.refused_with {
            let error = created.outcome.expect_err(&case.name);
            assert_eq!(error["code"], json!(code), "{}: {error}", case.name);
            continue;
        }
        let result = created.outcome.unwrap_or_else(|e| panic!("{}: create: {e}", case.name));
        let mut mine =
            result["annotation"].as_object().unwrap_or_else(|| panic!("{result}")).clone();
        let modified = mine.remove("modifiedAt").expect("the record carries modifiedAt");
        assert_eq!(
            case.has_modified,
            modified.as_str().is_some_and(|m| !m.is_empty()),
            "{}: /M presence differs",
            case.name,
        );
        // The stamps, by shape: an ISO date, and a name the create made a
        // UUIDv7 when the draft gave none.
        let created_at = mine["createdAt"].as_str().expect("a creation date");
        assert!(created_at.len() >= 20 && created_at.contains('T'), "{}: {created_at}", case.name);
        if case.created["nm"] == STAMPED {
            let nm = mine["nm"].as_str().expect("a name");
            assert!(is_uuid(nm) && &nm[14..15] == "7", "{}: /NM {nm} is not a UUIDv7", case.name);
        }
        assert_eq!(
            test_support::canonical_numbers(&normalize_stamps(&Value::Object(mine))),
            test_support::canonical_numbers(&case.created),
            "{}: {}",
            case.name,
            case.covers,
        );
    }
}

#[test]
fn a_create_is_append_only_and_says_so() {
    // The meta of a one-op change: the page it touched, the annotation it
    // made, named by its object number, under the write's `opId`, and
    // undoable, since its record deletes what it made.
    let file = load();
    for case in file.cases.iter().filter(|c| c.refused_with.is_none()) {
        let mut replay = open(&file);
        let created = create_in(&mut replay, case);
        let result = created.outcome.unwrap_or_else(|e| panic!("{}: {e}", case.name));
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
        assert_eq!(meta["cacheDelta"], Value::Null, "{}", case.name);
        assert!(meta["opId"].as_str().is_some_and(|id| !id.is_empty()), "{}: {meta}", case.name);
        assert_eq!(meta["undoable"], json!(true), "{}: {meta}", case.name);
        assert!(meta.get("weakRefsInvalidated").is_none(), "{}: an old meta key", case.name);
        // The event is the item's: `annotations.created`, on the page.
        let event = created.event.expect("a create publishes");
        assert_eq!(event["type"], "annotations.created", "{}", case.name);
        assert_eq!(event["page"], page_ref(case.page_object_number), "{}", case.name);
        assert_eq!(event["annotation"], result["annotation"], "{}", case.name);
    }
}

#[test]
fn a_new_annotation_is_addressable() {
    // Born an indirect object, so named by its object number: an address
    // that survives a reordering.
    let file = load();
    for case in file.cases.iter().filter(|c| c.refused_with.is_none()) {
        let mut replay = open(&file);
        let result = create_in(&mut replay, case).outcome.expect("created");
        let reference = &result["annotation"]["ref"];
        assert_eq!(reference["kind"], "objectNumber", "{}", case.name);
        assert!(reference["objectNumber"].as_u64().is_some_and(|n| n > 0), "{}", case.name);
        assert_eq!(reference["page"], page_ref(case.page_object_number), "{}", case.name);
    }
}

/// A draft, the bytes beside it, what it gets wrong, and the field the
/// refusal names (when it names one).
type Refusal<'a> =
    (Value, &'a std::collections::BTreeMap<String, Vec<u8>>, &'a str, Option<&'a str>);

fn listed_count(replay: &mut Replay) -> usize {
    let listed = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": DOC }))
        .expect("the document lists");
    listed["annotations"].as_array().expect("annotations").len()
}

#[test]
fn a_draft_that_cannot_be_written_is_refused_before_anything_is_written() {
    // The apply boundary: each draft its kind's checks refuse is
    // `InvalidArg`, and leaves the document as it was, so the next create
    // is the only annotation the page gains.
    let file = load();
    let pon = file.cases[0].page_object_number;
    let png: Vec<u8> = case(&file, "stamp-png-contain").resources["appearance"].clone();
    let png_only: std::collections::BTreeMap<String, Vec<u8>> =
        [("appearance".to_owned(), png)].into_iter().collect();
    let text_bytes: std::collections::BTreeMap<String, Vec<u8>> =
        [("file".to_owned(), b"bytes".to_vec())].into_iter().collect();
    let not_a_format: std::collections::BTreeMap<String, Vec<u8>> =
        [("appearance".to_owned(), b"bytes".to_vec())].into_iter().collect();
    let none = std::collections::BTreeMap::new();
    let square = |extra: Value| {
        let mut draft =
            json!({ "subtype": "square", "box": { "x": 10, "y": 10, "width": 90, "height": 90 } });
        for (k, v) in extra.as_object().unwrap() {
            draft[k] = v.clone();
        }
        draft
    };
    let cases: Vec<Refusal<'_>> = vec![
        (
            json!({ "subtype": "highlight", "quadPoints": [] }),
            &none,
            "no quads",
            Some("quadPoints"),
        ),
        (
            json!({ "subtype": "highlight" }),
            &none,
            "a markup with no quads at all",
            Some("quadPoints"),
        ),
        (square(json!({ "color": "red" })), &none, "a colour not as #rrggbb", Some("color")),
        (square(json!({ "opacity": 2 })), &none, "an opacity outside 0..1", Some("opacity")),
        (
            square(json!({ "cloudyIntensity": 0 })),
            &none,
            "a cloudy border of no intensity",
            Some("cloudyIntensity"),
        ),
        (square(json!({ "strokeWidth": -1 })), &none, "a negative stroke", Some("strokeWidth")),
        (
            square(json!({ "borderStyle": "beveled" })),
            &none,
            "a border only widgets draw",
            Some("borderStyle"),
        ),
        (square(json!({ "quadPoints": [] })), &none, "a field its kind doesn't declare", None),
        (
            json!({ "subtype": "square" }),
            &none,
            "a box kind with neither box nor rect",
            Some("box"),
        ),
        (
            json!({ "subtype": "square", "rotation": 30,
                    "rect": { "x": 10, "y": 10, "width": 90, "height": 90 } }),
            &none,
            "a rect at an angle a box can't be told from",
            Some("rect"),
        ),
        (
            json!({ "subtype": "polygon", "vertices": [{ "x": 1, "y": 1 }, { "x": 9, "y": 9 }] }),
            &none,
            "a polygon of two points",
            Some("vertices"),
        ),
        (
            json!({ "subtype": "polyline", "vertices": [{ "x": 1, "y": 1 }] }),
            &none,
            "a polyline of one point",
            Some("vertices"),
        ),
        (
            json!({ "subtype": "ink", "inkList": [[{ "x": 1, "y": 1 }], []] }),
            &none,
            "an ink stroke with no points",
            Some("inkList"),
        ),
        (
            json!({ "subtype": "ink", "inkList": [[{ "x": 1, "y": 1 }]], "strokeWidth": 0 }),
            &none,
            "ink drawn with no width",
            Some("strokeWidth"),
        ),
        (
            json!({ "subtype": "text", "rect": { "x": 10, "y": 10, "width": 20, "height": 20 },
                    "state": "AwaitingCounsel" }),
            &none,
            "a custom review state with no model",
            Some("stateModel"),
        ),
        (
            json!({ "subtype": "free-text", "intent": "free-text",
                    "box": { "x": 10, "y": 10, "width": 90, "height": 50 },
                    "calloutLine": [{ "x": 0, "y": 0 }, { "x": 10, "y": 10 }] }),
            &none,
            "a leader on a plain text box",
            Some("intent"),
        ),
        (
            json!({ "subtype": "free-text", "box": { "x": 10, "y": 10, "width": 90, "height": 50 },
                    "fontSize": 0 }),
            &none,
            "text drawn at no size at all",
            Some("fontSize"),
        ),
        (
            json!({ "subtype": "free-text", "box": { "x": 10, "y": 10, "width": 90, "height": 50 },
                    "fontFamily": "a-font-nobody-registered" }),
            &none,
            "a font this host has no registry to resolve",
            None,
        ),
        (
            json!({ "subtype": "free-text", "box": { "x": 10, "y": 10, "width": 90, "height": 50 },
                    "contents": "one", "richText": { "paragraphs": [{ "runs": [{ "text": "two" }] }] } }),
            &none,
            "contents that disagree with the rich text",
            None,
        ),
        (
            json!({ "subtype": "file-attachment", "rect": { "x": 10, "y": 10, "width": 20, "height": 20 },
                    "file": { "name": "notes.txt" } }),
            &none,
            "a file whose bytes did not travel with the draft",
            None,
        ),
        (
            json!({ "subtype": "file-attachment", "rect": { "x": 10, "y": 10, "width": 20, "height": 20 } }),
            &text_bytes,
            "a file with no name",
            Some("file.name"),
        ),
        (
            json!({ "subtype": "stamp", "box": { "x": 10, "y": 10, "width": 90, "height": 50 } }),
            &text_bytes,
            "a stamp given a file instead of its appearance",
            None,
        ),
        (
            json!({ "subtype": "stamp", "box": { "x": 10, "y": 10, "width": 90, "height": 50 } }),
            &not_a_format,
            "an appearance that is not PNG, JPEG or PDF",
            None,
        ),
        (
            json!({ "subtype": "stamp", "box": { "x": 10, "y": 10, "width": 90, "height": 50 },
                    "name": "" }),
            &png_only,
            "an empty stamp name",
            None,
        ),
        (
            json!({ "subtype": "redact" }),
            &none,
            "a redaction with neither rect nor quads",
            Some("rect"),
        ),
        (
            json!({ "subtype": "widget", "box": { "x": 10, "y": 10, "width": 90, "height": 50 } }),
            &none,
            "a widget, which is the form's",
            Some("subtype"),
        ),
        (
            json!({ "subtype": "link", "rect": { "x": 10, "y": 10, "width": 90, "height": 20 },
                    "target": { "kind": "named", "name": "Print" } }),
            &none,
            "a named verb that isn't a page turn",
            Some("target"),
        ),
        (
            json!({ "subtype": "text", "rect": { "left": 10, "bottom": 10, "right": 30, "top": 30 } }),
            &none,
            "a rect given in the file's coordinates",
            Some("rect"),
        ),
    ];

    let mut replay = open(&file);
    let before = listed_count(&mut replay);
    for (draft, resources, what, field) in cases {
        let refused = create_draft(&mut replay, &draft, pon, resources)
            .outcome
            .expect_err(&format!("{what}: not refused"));
        assert_eq!(refused["code"], "InvalidArg", "{what}: {refused}");
        if let Some(field) = field {
            assert_eq!(refused["details"]["field"], field, "{what}: {refused}");
        }
        assert_eq!(listed_count(&mut replay), before, "{what}: the refused draft left something");
    }
    let valid = case(&file, "highlight-defaults");
    create_in(&mut replay, valid).outcome.expect("a valid draft still creates");
    assert_eq!(listed_count(&mut replay), before + 1);
}

/// The created annotation's appearances, rendered through the wire.
fn render(replay: &mut Replay, pon: u32) -> Vec<(Value, Vec<u8>)> {
    let answer = replay.send(
        &json!({ "kind": "annotations.renderAppearances", "jobId": "<job>", "docId": DOC,
            "page": page_ref(pon), "family": "annotations",
            "options": { "viewport": { "kind": "scale", "scale": 1 }, "modes": ["normal"] } }),
        Vec::new(),
    );
    let result = answer.outcome.expect("render");
    result["appearances"]
        .as_array()
        .expect("appearances")
        .iter()
        .cloned()
        .zip(answer.payloads)
        .collect()
}

fn painted(pixels: &[u8]) -> bool {
    pixels.chunks(4).any(|pixel| pixel[3] > 0)
}

#[test]
fn a_created_annotation_ships_with_an_appearance() {
    // An annotation's `/AP` is not in its record, so every comparison above
    // passes for a writer that never generated one. Rendering is the only
    // way a consumer would find out.
    let file = load();
    let case = case(&file, "square-decorated");
    let mut replay = open(&file);
    assert!(render(&mut replay, case.page_object_number).is_empty(), "the page starts bare");
    create_in(&mut replay, case).outcome.expect("created");
    let rendered = render(&mut replay, case.page_object_number);
    assert_eq!(rendered.len(), 1, "the new annotation draws nothing");
    let (appearance, pixels) = &rendered[0];
    assert_eq!(
        test_support::canonical_numbers(&appearance["rect"]),
        test_support::canonical_numbers(&case.created["rect"]),
    );
    assert!(painted(pixels), "the appearance stream is there and paints nothing");
}

#[test]
fn a_created_annotation_survives_a_save_and_reopen() {
    // Every comparison above reads the annotation back out of the session
    // that made it. Saving and reopening is what a caller does with it.
    let file = load();
    let case = case(&file, "square-decorated");
    let mut replay = open(&file);
    create_in(&mut replay, case).outcome.expect("created");
    let saved = replay.send(
        &json!({ "kind": "document.saveBuffer", "jobId": "<job>", "docId": DOC, "mode": "rewrite" }),
        Vec::new(),
    );
    saved.outcome.expect("saved");
    let bytes = saved.payloads.into_iter().next().expect("the saved bytes");

    let mut host = Host::new();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": DOC,
            "password": null })
        .to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let mut reopened = Replay::adopt(host, DOC);
    let listed = reopened
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": DOC }))
        .expect("listed");
    let annotations = listed["annotations"].as_array().expect("annotations");
    assert_eq!(annotations.len(), 1, "the annotation did not survive the save");
    let read = &annotations[0];
    assert_eq!(read["subtype"], "square");
    assert_eq!(
        test_support::canonical_numbers(&read["rect"]),
        test_support::canonical_numbers(&case.created["rect"]),
    );
    assert_eq!(read["interiorColor"], case.created["interiorColor"]);
    let rendered = render(&mut reopened, case.page_object_number);
    assert_eq!(rendered.len(), 1, "the /AP did not survive the save");
    assert!(painted(&rendered[0].1));
}

/// Create `name`'s draft, then hand its annotation, as the file holds it, to
/// `inspect`. A record can't show everything a writer decides: where the
/// reader normalizes, two different files read the same.
fn with_written<R>(
    file: &VectorFile,
    name: &str,
    inspect: impl FnOnce(&pdfium::Annotation<'_>) -> R,
) -> R {
    let case = case(file, name);
    let mut replay = open(file);
    let result = create_in(&mut replay, case).outcome.unwrap_or_else(|e| panic!("{name}: {e}"));
    let object_number = result["annotation"]["ref"]["objectNumber"].as_u64().unwrap() as u32;
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
        .expect("the created annotation");
    inspect(&annotation)
}

#[test]
fn a_review_state_is_written_in_the_spelling_the_pdf_uses() {
    // §12.5.6.3 names these with a capital, and a standard state brings its
    // own model when the draft names none.
    let file = load();
    for (name, model, state) in [
        ("text-reviewed", Some("Review"), Some("Accepted")),
        ("text-model-alone", Some("Marked"), None),
        // A model nobody standardized is written exactly as it arrived.
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

/// The bounds of the drawing a stamp's appearance places, in the
/// appearance form's own space.
fn drawing_bounds(written: &pdfium::Annotation<'_>) -> [f32; 4] {
    assert_eq!(written.object_count(), 1, "one drawing placed");
    let bounds = written.object_bounds(0).expect("the drawing's bounds");
    [bounds.left, bounds.bottom, bounds.right, bounds.top]
}

#[test]
fn a_stamp_fit_policy_decides_where_the_picture_lands() {
    // The record carries the box and never the placement inside it, so
    // three fits of the same bytes into the same box read back the same.
    // What differs is the appearance. The source is 4x2 in an 80x80 box:
    // `contain` fits the width and centres a 40-point band; `cover` fits
    // the height and overflows 40 points either side; `fill` takes the box
    // and distorts.
    let file = load();
    for (name, expected) in [
        ("stamp-png-contain", [0.0_f32, 20.0, 80.0, 60.0]),
        ("stamp-png-cover", [-40.0, 0.0, 120.0, 80.0]),
        ("stamp-png-fill", [0.0, 0.0, 80.0, 80.0]),
    ] {
        with_written(&file, name, |written| {
            let actual = drawing_bounds(written);
            for (got, want) in actual.iter().zip(expected.iter()) {
                assert!((got - want).abs() < 0.5, "{name}: at {actual:?}, expected {expected:?}");
            }
        });
    }
}

#[test]
fn a_rotated_stamp_is_authored_in_the_frame_it_is_turned_from() {
    // Authoring into the turned box bakes the letterbox bands in, and the
    // closing re-fit shrinks the picture a second time. The drawing must
    // fill the box it turns from: 90x60, with a 4:2 source, is a 90-point
    // band 45 points tall.
    let file = load();
    with_written(&file, "stamp-rotated", |written| {
        let [left, bottom, right, top] = drawing_bounds(written);
        let (width, height) = (right - left, top - bottom);
        assert!(
            (width - 90.0).abs() < 0.5 && (height - 45.0).abs() < 0.5,
            "the content is {width}x{height}, expected about 90x45 in the box",
        );
        assert_eq!(written.rotate(), Some(30.0), "Acrobat's /Rotate, counterclockwise");
    });
}

#[test]
fn a_redaction_gets_a_default_appearance_only_when_it_has_a_label() {
    // ISO 32000-2 requires a `/DA` beside an `/OverlayText`, and a redaction
    // with no label has nothing for one to describe. The record reads the
    // same defaults for a missing `/DA`, so this is checked in the file.
    let file = load();
    for (name, has_da, overlay) in [
        ("redact-text-with-label", true, Some("REDACTED")),
        ("redact-autofit-label", true, Some("gone")),
        ("redact-area", false, None),
        // An empty label is no label; it still names a label, so a `/DA`.
        ("redact-empty-label", true, None),
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
    // The record reads a turn of 0 as none, and never reports
    // `/SchemaVersion`. Both are what a later reader of the file depends on.
    // A turn is recorded in PDF's counterclockwise degrees with the box it
    // turns about: a box kind's box, a point kind's upright points' box.
    let file = load();
    // name -> (rotation, has an unrotated box, the dictionary exists)
    let expected: [(&str, Option<f32>, bool, bool); 4] = [
        ("square-rotated", Some(45.0), true, true),
        ("square-zero-rotation", None, false, false),
        ("ink-advisory-rotation", Some(45.0), true, true),
        ("polyline-zero-rotation", None, false, false),
    ];
    for (name, rotation, has_box, has_dictionary) in expected {
        with_written(&file, name, |written| {
            assert_eq!(written.has_embed_metadata(), has_dictionary, "{name}: /EMBD_Metadata");
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

// ── Ported from upstream's conformance and main tests ───────────────────────

const QUAD: &str = r#"[{ "upperLeft": { "x": 72, "y": 72 }, "upperRight": { "x": 200, "y": 72 },
    "lowerLeft": { "x": 72, "y": 92 }, "lowerRight": { "x": 200, "y": 92 } }]"#;

fn quad() -> Value {
    serde_json::from_str(QUAD).unwrap()
}

fn created(replay: &mut Replay, draft: Value) -> Value {
    let pon = load().cases[0].page_object_number;
    create_draft(replay, &draft, pon, &std::collections::BTreeMap::new())
        .outcome
        .unwrap_or_else(|e| panic!("{draft}: {e}"))
}

fn listed(replay: &mut Replay, reference: &Value) -> Value {
    let listed = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": DOC }))
        .expect("listed");
    listed["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["ref"] == *reference)
        .cloned()
        .expect("the annotation lists")
}

/// `runAnnotationMutationConformance`: "create a reply links /IRT and
/// defaults /RT to 'reply'" and "create a grouped subordinate writes /RT
/// /Group".
#[test]
fn a_reply_links_its_parent_and_a_group_member_says_so() {
    let file = load();
    let mut replay = open(&file);
    let parent = created(
        &mut replay,
        json!({ "subtype": "highlight", "contents": "reply parent", "quadPoints": quad() }),
    );
    assert_eq!(parent["annotation"]["reply"], Value::Null);
    let parent_ref = parent["annotation"]["ref"].clone();

    let reply = created(
        &mut replay,
        json!({ "subtype": "highlight", "contents": "a reply", "quadPoints": quad(),
                "reply": { "to": parent_ref } }),
    );
    assert_eq!(reply["annotation"]["reply"], json!({ "to": parent_ref, "type": "reply" }));
    // Only the reply was written.
    assert_eq!(reply["meta"]["changed"], json!([reply["annotation"]["ref"]]));
    let read = listed(&mut replay, &reply["annotation"]["ref"]);
    assert_eq!(read["reply"]["type"], "reply");

    let caret = created(
        &mut replay,
        json!({ "subtype": "caret", "contents": "", "color": "#000000", "opacity": 1,
                "box": { "x": 300, "y": 132, "width": 20, "height": 30 },
                "reply": { "to": parent_ref, "type": "group" } }),
    );
    assert_eq!(caret["annotation"]["reply"]["type"], "group");
    assert_eq!(listed(&mut replay, &caret["annotation"]["ref"])["reply"]["type"], "group");

    // A parent on another page is refused before anything is written.
    let elsewhere = json!({ "kind": "objectNumber",
        "page": { "kind": "objectNumber", "objectNumber": 999 }, "objectNumber": 1 });
    let refused = create_draft(
        &mut replay,
        &json!({ "subtype": "highlight", "quadPoints": quad(), "reply": { "to": elsewhere } }),
        file.cases[0].page_object_number,
        &std::collections::BTreeMap::new(),
    )
    .outcome
    .expect_err("a parent on another page");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
}

/// `runAnnotationMutationConformance`: "rich text: a draft with runs
/// round-trips the body, the runs and the plain projection".
#[test]
fn a_rich_text_draft_writes_its_body_and_runs() {
    let file = load();
    let mut replay = open(&file);
    let result = created(
        &mut replay,
        json!({ "subtype": "free-text", "intent": "free-text", "color": "#0000ff",
            "box": { "x": 72, "y": 300, "width": 200, "height": 80 },
            "richText": {
                "body": { "family": "Helvetica", "size": 18, "color": "#102030" },
                "paragraphs": [
                    { "runs": [{ "text": "Hello " }, { "text": "bold", "style": { "weight": 700 } },
                        { "text": " red", "style": { "color": "#FF0000" } }] },
                    { "align": "center", "runs": [{ "text": "H" },
                        { "text": "2", "style": { "script": "sub" } }] }
                ] } }),
    );
    let dto = &result["annotation"];
    assert_eq!(dto["contents"], "Hello bold red\rH2");
    assert_eq!(dto["fontFamily"], "helvetica");
    assert_eq!(test_support::canonical_numbers(&dto["fontSize"]), json!(18));
    assert_eq!(dto["color"], "#0000ff");
    assert_eq!(dto["fontColor"], "#102030");
    assert_eq!(dto["richText"]["body"]["color"], "#102030");
    assert_eq!(
        test_support::canonical_numbers(&dto["richText"]["paragraphs"][0]["runs"]),
        json!([{ "text": "Hello " }, { "text": "bold", "style": { "weight": 700 } },
            { "text": " red", "style": { "color": "#ff0000" } }])
    );
    assert_eq!(dto["richText"]["paragraphs"][1]["align"], "center");
    assert_eq!(
        dto["richText"]["paragraphs"][1]["runs"][1],
        json!({ "text": "2", "style": { "script": "sub" } })
    );
    let read = listed(&mut replay, &dto["ref"]);
    assert_eq!(
        test_support::canonical_numbers(&read["richText"]["paragraphs"][0]["runs"][1]),
        json!({ "text": "bold", "style": { "weight": 700 } })
    );
}

/// `annotations-stamp.test.ts`: "any non-empty stamp name is accepted; an
/// empty one rejects before any write". `/Name` is the stamp's identifier,
/// not an enum.
#[test]
fn any_non_empty_stamp_name_is_accepted() {
    let file = load();
    let png = case(&file, "stamp-png-contain").resources.clone();
    let pon = file.cases[0].page_object_number;
    let mut replay = open(&file);
    let custom = "#LBGiYhk8V_oAfmqAPENiwD";
    let result = create_draft(
        &mut replay,
        &json!({ "subtype": "stamp", "box": { "x": 20, "y": 100, "width": 60, "height": 30 },
            "name": custom, "contents": "before" }),
        pon,
        &png,
    )
    .outcome
    .expect("a custom name creates");
    assert_eq!(result["annotation"]["name"], custom);
}

/// `drawingFor`: placing the same artwork twice adds one drawing, which
/// both stamps place.
#[test]
fn two_stamps_of_one_artwork_place_one_drawing() {
    let file = load();
    let stamp = case(&file, "stamp-png-contain");
    let mut replay = open(&file);
    create_in(&mut replay, stamp).outcome.expect("first");
    create_in(&mut replay, stamp).outcome.expect("second");
    let session = replay.host.sessions().session(&SessionKey::document(DOC)).expect("open");
    let drawings = session.document().unwrap().document().stamp_drawings();
    assert_eq!(drawings.len(), 1, "{drawings:?}");
}

/// A create as an op of `document.apply`: in page space both ways, its
/// item and event the single verb's, and undone by `{ undoOf }`, which
/// deletes it while it reads as the create left it.
#[test]
fn a_create_op_applies_in_page_space_and_its_undo_deletes_it() {
    let file = load();
    let square = case(&file, "square-decorated");
    let mut replay = open(&file);
    let before = listed_count(&mut replay);
    let applied = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "create-1",
            "change": { "ops": [{ "type": "annotations.create",
                "page": page_ref(square.page_object_number), "data": square.draft }] } }))
        .expect("applied");
    let item = &applied["result"]["items"][0];
    assert_eq!(item["type"], "annotations.create");
    assert_eq!(item["page"], page_ref(square.page_object_number));
    assert_eq!(
        test_support::canonical_numbers(&item["annotation"]["box"]),
        test_support::canonical_numbers(&square.created["box"]),
        "the item answers page space"
    );
    assert_eq!(applied["events"][0]["type"], "annotations.created");
    assert_eq!(listed_count(&mut replay), before + 1);

    let undone = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "undo-1", "change": { "undoOf": "create-1" } }))
        .expect("undone");
    assert_eq!(undone["result"]["items"][0]["type"], "annotations.delete", "{undone}");
    assert_eq!(undone["result"]["items"][0]["meta"]["changed"][0], item["annotation"]["ref"]);
    assert_eq!(undone["events"][0]["type"], "annotations.deleted");
    assert_eq!(listed_count(&mut replay), before);
}

/// A change refused after its create ran is taken back whole: the
/// journal's remove step deletes what the create made.
#[test]
fn a_refused_change_takes_its_create_back() {
    let file = load();
    let square = case(&file, "square-decorated");
    let mut replay = open(&file);
    let before = listed_count(&mut replay);
    let refused = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "refused-1",
            "change": { "ops": [
                { "type": "annotations.create",
                  "page": page_ref(square.page_object_number), "data": square.draft },
                { "type": "annotations.create",
                  "page": page_ref(square.page_object_number),
                  "data": { "subtype": "square", "color": "red",
                            "box": { "x": 1, "y": 1, "width": 9, "height": 9 } } }
            ] } }))
        .expect_err("the second op is refused");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(listed_count(&mut replay), before, "the first create was left behind");
}

// ── Fix round 1 ─────────────────────────────────────────────────────────────

/// A measurement's scale, a shown caption or a leader needs 8.25's
/// writing: refused `NotImplemented`, and nothing is written.
#[test]
fn a_measured_create_is_refused_until_8_25_and_writes_nothing() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let line = case(&file, "line").draft.clone();
    let mut replay = open(&file);
    let before = listed_count(&mut replay);
    for (field, value) in [
        ("measure", json!({ "subtype": "rectilinear", "ratio": "1 in = 1 in" })),
        ("captionEnabled", json!(true)),
        ("captionPosition", json!("top")),
        ("captionOffset", json!({ "along": 1, "perpendicular": 2 })),
        ("leader", json!({ "length": 10 })),
    ] {
        let mut draft = line.clone();
        draft[field] = value;
        let refused = create_draft(&mut replay, &draft, pon, &std::collections::BTreeMap::new())
            .outcome
            .expect_err(field);
        assert_eq!(refused["code"], "NotImplemented", "{field}: {refused}");
        assert_eq!(refused["details"]["field"], field, "{field}: {refused}");
        assert_eq!(listed_count(&mut replay), before, "{field}: something was written");
    }
}

/// A line read sent back as a create: its unmeasured caption fields
/// (`captionPosition: "inline"`, `captionEnabled` null or false, no offset,
/// no leader, no scale) are accepted, as upstream accepts them.
#[test]
fn a_line_read_sent_back_as_a_create_is_accepted() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let read = case(&file, "line").created.clone();
    let mut replay = open(&file);
    let mut draft = read.clone();
    draft.as_object_mut().unwrap().remove("nm");
    let copy = create_draft(&mut replay, &draft, pon, &std::collections::BTreeMap::new())
        .outcome
        .expect("the read creates");
    assert_eq!(copy["annotation"]["captionEnabled"], Value::Null);
    assert_eq!(copy["annotation"]["captionPosition"], "inline");
    assert_eq!(
        test_support::canonical_numbers(&copy["annotation"]["linePoints"]),
        test_support::canonical_numbers(&read["linePoints"])
    );
    // A hidden caption is written as one.
    draft["captionEnabled"] = json!(false);
    let hidden = create_draft(&mut replay, &draft, pon, &std::collections::BTreeMap::new())
        .outcome
        .expect("a hidden caption creates");
    assert_eq!(hidden["annotation"]["captionEnabled"], json!(false));
}

/// `claimObjectNumber`: a number a change's earlier create took is
/// `taken` for a later one, before anything is written; one the session
/// doesn't hold is `not-held`; a number that isn't whole is not an object
/// number. (Checked on the session: the fork's numbered create needs the
/// layer transaction Task 8.30 adds, so a create naming a number can't make
/// its object here yet.)
#[test]
fn a_create_names_an_object_number_once_per_change() {
    let file = load();
    let mut replay = open(&file);
    let session = replay.host.sessions_mut().session_mut(&SessionKey::document(DOC)).expect("open");
    session.begin_transaction("claims").expect("a transaction");
    session.claim_object_number(6).expect("a held number");
    let taken = session.claim_object_number(6).expect_err("claimed twice");
    assert_eq!(taken.details.as_ref().unwrap()["reason"], "taken", "{taken}");
    let not_held = session.claim_object_number(70).expect_err("past the reserve");
    assert_eq!(not_held.details.as_ref().unwrap()["reason"], "not-held", "{not_held}");
    session.abort_transaction();
    session.begin_transaction("again").expect("a transaction");
    session.claim_object_number(6).expect("an aborted claim frees the number");
    session.abort_transaction();

    let square = case(&file, "square-defaults");
    let mut fractional = request(&square.draft, square.page_object_number, &Default::default()).0;
    fractional["objectNumber"] = json!(1.5);
    let refused = replay.replay(&fractional).expect_err("not whole");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "objectNumber", "{refused}");
    assert!(refused["message"].as_str().unwrap().contains("is not an object number"));
}

/// A popup's create in a refused change: taking it back puts the parent's
/// `/Popup` and `/Open` back as they were.
#[test]
fn a_refused_popup_create_gives_its_parent_back() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = open(&file);
    let note = created(
        &mut replay,
        json!({ "subtype": "text", "open": true,
                "rect": { "x": 72, "y": 172, "width": 20, "height": 20 } }),
    );
    let note_ref = note["annotation"]["ref"].clone();
    let before = listed(&mut replay, &note_ref);
    assert_eq!(before["popup"], Value::Null);
    assert_eq!(before["open"], json!(true));

    let refused = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "change": { "ops": [
                { "type": "annotations.create", "page": page_ref(pon),
                  "data": { "subtype": "popup", "parent": note_ref, "open": false,
                            "rect": { "x": 100, "y": 172, "width": 150, "height": 80 } } },
                { "type": "annotations.create", "page": page_ref(pon),
                  "data": { "subtype": "square", "color": "red",
                            "box": { "x": 1, "y": 1, "width": 9, "height": 9 } } }
            ] } }))
        .expect_err("the second op is refused");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    let after = listed(&mut replay, &note_ref);
    assert_eq!(after["popup"], Value::Null, "the parent still names the popup: {after}");
    assert_eq!(after["open"], json!(true), "the parent's /Open was not put back: {after}");
}
