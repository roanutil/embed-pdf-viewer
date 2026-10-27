//! G11's delete gate: the same annotations go, with their thread, and the
//! same names come back.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-delete
//!
//! Every case deletes from `annotation_base.pdf`, the fixture with one
//! annotation of every identity shape: objects, inline annotations with and
//! without an `/NM`, and a thread (a reply, a grouped part and a review
//! state on the first annotation). Each case replays the request the
//! generator sent (`request`) through `Host::call`.
//!
//! What is compared is the meta's `changed` list AND the `/NM` of every
//! annotation left on the page in order. That last one is what catches a
//! delete that removed the wrong entry: the names would still look right.
//!
//! The fixture opens through `open.fatMem`, a layer over the bytes
//! (`openBytesAsLayer`): only a layer records where an inline annotation was
//! born when a delete promotes its page, which is what keeps naming it by
//! `baseIndex`, and only a layer can capture what a delete removes.

mod common;

use common::page_ref;
use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;
const DOC: &str = "delete";

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
    page_object_number: u32,
    #[serde(rename = "ref")]
    annotation_ref: Value,
    /// The `annotations.delete` request the generator sent.
    request: Value,
    meta: ExpectedMeta,
    /// Every surviving annotation's `/NM`, in order.
    remaining: Vec<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct ExpectedMeta {
    changed: Vec<Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("delete.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("delete.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.delete");
    file
}

/// The page's annotations as `annotations.list` answers them.
fn listed(replay: &mut Replay, pon: u32) -> Vec<Value> {
    let listing = replay
        .replay(&json!({ "kind": "annotations.list", "jobId": "<job>", "docId": DOC,
            "pages": [page_ref(pon)] }))
        .expect("list");
    listing["annotations"].as_array().cloned().unwrap_or_default()
}

fn names(annotations: &[Value]) -> Vec<Option<String>> {
    annotations.iter().map(|a| a["nm"].as_str().map(str::to_owned)).collect()
}

fn refs(annotations: &[Value]) -> Vec<Value> {
    annotations.iter().map(|a| a["ref"].clone()).collect()
}

/// A `document.apply` of `change` under `op_id`.
fn apply(replay: &mut Replay, op_id: &str, change: Value) -> Result<Value, Value> {
    replay.replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
        "opId": op_id, "change": change }))
}

#[test]
fn a_delete_removes_the_annotation_the_typescript_engine_removed() {
    let file = load();
    assert!(file.cases.len() >= 5, "the case list shrank");
    let mut threads = 0usize;

    for case in &file.cases {
        let mut replay = Replay::open(&file.fixture, DOC);
        let answer =
            replay.replay(&case.request).unwrap_or_else(|e| panic!("{}: delete: {e}", case.name));
        let result = &answer["result"];
        // Nothing exists after a delete: the result is its meta only.
        assert_eq!(
            result.as_object().map(|r| r.keys().cloned().collect::<Vec<_>>()),
            Some(vec!["meta".to_owned()]),
            "{}: {result}",
            case.name
        );
        assert_eq!(
            result["meta"]["changed"].as_array().cloned().unwrap_or_default(),
            case.meta.changed,
            "{}: what went: {}",
            case.name,
            case.covers,
        );
        assert_eq!(
            result["meta"]["changed"][0], case.annotation_ref,
            "{}: the target first",
            case.name
        );
        assert_eq!(result["meta"]["affectedPages"], json!([page_ref(case.page_object_number)]));
        if case.meta.changed.len() > 1 {
            threads += 1;
        }

        // The event names what went, for listeners that didn't delete it.
        let event = &answer["event"];
        assert_eq!(event["type"], "annotations.deleted", "{}: {event}", case.name);
        assert_eq!(event["deleted"], result["meta"]["changed"], "{}", case.name);
        assert_eq!(event["page"], page_ref(case.page_object_number), "{}", case.name);

        // The part a name comparison cannot see.
        assert_eq!(
            names(&listed(&mut replay, case.page_object_number)),
            case.remaining,
            "{}: the wrong annotations were removed: {}",
            case.name,
            case.covers,
        );
    }
    assert!(threads > 0, "no case deletes a thread");
}

/// The name is historical: upstream answers a ref that names nothing
/// `NotFound` (`resolveAnnotIndexRaw`), probed before anything is removed.
#[test]
fn a_reference_that_names_nothing_is_an_invalid_reference() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    for (reference, what) in [
        (
            json!({ "kind": "objectNumber", "page": page_ref(pon), "objectNumber": 9_999 }),
            "an object number nothing has",
        ),
        (
            json!({ "kind": "baseIndex", "page": page_ref(pon), "baseIndex": 99 }),
            "a birth past the end",
        ),
        // An object, so no inline annotation was born at its position.
        (
            json!({ "kind": "baseIndex", "page": page_ref(pon), "baseIndex": 0 }),
            "a birth of an object",
        ),
    ] {
        let refused = replay
            .replay(&json!({ "kind": "annotations.delete", "jobId": "<job>", "opId": "<op>",
                "docId": DOC, "ref": reference }))
            .expect_err(what);
        assert_eq!(refused["code"], "NotFound", "{what}: {refused}");
    }
    // The wire's old names are not names any more.
    let refused = replay
        .replay(&json!({ "kind": "annotations.delete", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "ref": { "kind": "nm", "page": page_ref(pon), "nm": "the-anchor" } }))
        .expect_err("an nm ref");
    assert_eq!(refused["code"], "WireFormat", "{refused}");
    assert_eq!(listed(&mut replay, pon), before, "something was removed");
}

/// A widget is the form's (1233d1349): every widget is refused, naming the
/// form op that removes it, and nothing is removed.
#[test]
fn an_attached_widget_is_not_deleted_through_the_annotation_plane() {
    let fixture = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
    let mut replay = Replay::open(fixture, DOC);
    let pages = replay
        .replay(&json!({ "kind": "pages.list", "jobId": "<job>", "docId": DOC }))
        .expect("pages");
    let pon = pages["pages"][0]["ref"]["objectNumber"].as_u64().expect("a page") as u32;
    let widgets = replay
        .replay(&json!({ "kind": "forms.list", "jobId": "<job>", "docId": DOC }))
        .expect("forms");
    let count = |replay: &mut Replay| {
        replay
            .replay(&json!({ "kind": "forms.list", "jobId": "<job>", "docId": DOC }))
            .expect("forms")["widgets"]
            .as_array()
            .map_or(0, Vec::len)
    };
    let before = count(&mut replay);
    let mut refused_kinds = std::collections::BTreeSet::new();
    for widget in widgets["widgets"].as_array().expect("widgets") {
        let reference = widget["ref"].clone();
        if reference["page"]["objectNumber"] != json!(pon) {
            continue;
        }
        let refused = replay
            .replay(&json!({ "kind": "annotations.delete", "jobId": "<job>", "opId": "<op>",
                "docId": DOC, "ref": reference }))
            .expect_err("a widget");
        assert_eq!(refused["code"], "InvalidArg", "{refused}");
        let message = refused["message"].as_str().unwrap_or_default().to_owned();
        assert!(message.contains("widget"), "{message}");
        refused_kinds.insert(message);
    }
    // The fixture's field widget is its field's own dictionary.
    assert!(refused_kinds.iter().any(|m| m.contains("doc.forms.delete")), "{refused_kinds:?}");
    assert!(
        refused_kinds.iter().any(|m| m.contains("isn't deleted as an annotation")),
        "{refused_kinds:?}"
    );
    assert_eq!(count(&mut replay), before, "a widget was removed");
}

/// A delete's undo brings back everything it removed, at the same numbers
/// and positions, under the same names; undoing the undo deletes them
/// again.
#[test]
fn a_delete_takes_its_thread_and_its_undo_brings_it_back_where_it_was() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "by-object-number").expect("the thread case");
    let pon = case.page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);

    let applied = apply(
        &mut replay,
        "delete-1",
        json!({ "ops": [{ "type": "annotations.delete", "ref": case.annotation_ref }] }),
    )
    .expect("applied");
    let item = &applied["result"]["items"][0];
    assert_eq!(item["type"], "annotations.delete", "{item}");
    assert_eq!(item["meta"]["changed"].as_array().unwrap(), &case.meta.changed);
    assert_eq!(applied["result"]["meta"]["undoable"], true, "{applied}");
    assert_eq!(names(&listed(&mut replay, pon)), case.remaining);

    let undone = apply(&mut replay, "undo-1", json!({ "undoOf": "delete-1" })).expect("undone");
    let restored = &undone["result"]["items"][0];
    assert_eq!(restored["type"], "annotations.restore", "{undone}");
    assert_eq!(restored["meta"]["changed"].as_array().unwrap(), &case.meta.changed);
    assert_eq!(undone["events"][0]["type"], "annotations.restored", "{undone}");
    let after = listed(&mut replay, pon);
    assert_eq!(refs(&after), refs(&before), "the names and their order");
    assert_eq!(after, before, "what came back reads as it did");
    // The item answers what came back, in page space, as a listing does.
    for annotation in restored["annotations"].as_array().expect("annotations") {
        assert!(after.contains(annotation), "{annotation}");
    }

    let redone = apply(&mut replay, "redo-1", json!({ "undoOf": "undo-1" })).expect("redone");
    assert_eq!(redone["result"]["items"][0]["type"], "annotations.delete", "{redone}");
    assert_eq!(names(&listed(&mut replay, pon)), case.remaining);
}

/// An undone create can be redone: the delete its undo runs captures what
/// it removes, and the restore brings the annotation back under its number.
#[test]
fn an_undone_create_can_be_redone() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let created = apply(
        &mut replay,
        "create-1",
        json!({ "ops": [{ "type": "annotations.create", "page": page_ref(pon),
            "data": { "subtype": "square", "box": { "x": 10, "y": 10, "width": 40, "height": 30 },
                "contents": "to be redone" } }] }),
    )
    .expect("created");
    let made = created["result"]["items"][0]["annotation"].clone();
    let with_it = listed(&mut replay, pon);
    assert_eq!(with_it.len(), before.len() + 1);

    let undone = apply(&mut replay, "undo-1", json!({ "undoOf": "create-1" })).expect("undone");
    assert_eq!(undone["result"]["items"][0]["type"], "annotations.delete", "{undone}");
    assert_eq!(undone["result"]["meta"]["undoable"], true, "an undone create has a redo");
    assert_eq!(listed(&mut replay, pon), before);

    let redone = apply(&mut replay, "redo-1", json!({ "undoOf": "undo-1" })).expect("redone");
    let item = &redone["result"]["items"][0];
    assert_eq!(item["type"], "annotations.restore", "{redone}");
    assert_eq!(item["annotations"][0]["ref"], made["ref"], "the same number");
    assert_eq!(listed(&mut replay, pon), with_it);
}

/// A change refused after its delete ran is taken back whole: the
/// journal's restore step brings back what the delete removed.
#[test]
fn a_refused_change_takes_its_delete_back() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "by-object-number").expect("the thread case");
    let pon = case.page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let refused = apply(
        &mut replay,
        "refused-1",
        json!({ "ops": [
            { "type": "annotations.delete", "ref": case.annotation_ref },
            { "type": "annotations.delete",
              "ref": { "kind": "objectNumber", "page": page_ref(pon), "objectNumber": 9_999 } },
        ] }),
    )
    .expect_err("the second op is refused");
    assert_eq!(refused["code"], "NotFound", "{refused}");
    assert_eq!(listed(&mut replay, pon), before, "the first delete was left behind");
}

/// `expect` is what the op assumes the annotation reads now; a delete that
/// finds otherwise is refused whole, `ChangeConflict`.
#[test]
fn a_delete_that_expects_otherwise_is_refused() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "the-last-annotation").expect("a case");
    let pon = case.page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let target = before.iter().find(|a| a["ref"] == case.annotation_ref).expect("the target");
    let refused = apply(
        &mut replay,
        "expect-1",
        json!({ "ops": [{ "type": "annotations.delete", "ref": case.annotation_ref,
            "expect": { "contents": "not what it says" } }] }),
    )
    .expect_err("a wrong expectation");
    assert_eq!(refused["code"], "ChangeConflict", "{refused}");
    assert_eq!(listed(&mut replay, pon), before);
    apply(
        &mut replay,
        "expect-2",
        json!({ "ops": [{ "type": "annotations.delete", "ref": case.annotation_ref,
            "expect": { "contents": target["contents"] } }] }),
    )
    .expect("the right expectation");
    assert_eq!(names(&listed(&mut replay, pon)), case.remaining);
}

/// A popup deleted without the note it shows leaves the note, which stops
/// naming it; the delete's undo brings the popup back and the note names
/// it again. Deleting the note takes its popup with it.
#[test]
fn a_popup_deleted_alone_unlinks_its_note_and_its_undo_links_it_again() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let created = apply(
        &mut replay,
        "create-1",
        json!({ "ops": [{ "type": "annotations.create", "page": page_ref(pon),
            "data": { "subtype": "text", "rect": { "x": 72, "y": 172, "width": 20, "height": 20 } } }] }),
    )
    .expect("the note");
    let note = created["result"]["items"][0]["annotation"]["ref"].clone();
    let created = apply(
        &mut replay,
        "create-2",
        json!({ "ops": [{ "type": "annotations.create", "page": page_ref(pon),
            "data": { "subtype": "popup", "parent": note, "open": false,
                "rect": { "x": 100, "y": 172, "width": 150, "height": 80 } } }] }),
    )
    .expect("the popup");
    let popup = created["result"]["items"][0]["annotation"]["ref"].clone();
    let read = |replay: &mut Replay, reference: &Value| {
        listed(replay, pon).into_iter().find(|a| a["ref"] == *reference)
    };
    assert_eq!(read(&mut replay, &note).expect("the note")["popup"], popup);

    let deleted = apply(
        &mut replay,
        "delete-1",
        json!({ "ops": [{ "type": "annotations.delete", "ref": popup }] }),
    )
    .expect("deleted");
    assert_eq!(deleted["result"]["items"][0]["meta"]["changed"], json!([popup]));
    assert_eq!(read(&mut replay, &note).expect("the note stays")["popup"], Value::Null);

    apply(&mut replay, "undo-1", json!({ "undoOf": "delete-1" })).expect("undone");
    assert_eq!(read(&mut replay, &note).expect("the note")["popup"], popup);
    assert_eq!(read(&mut replay, &popup).expect("the popup")["parent"], note);

    let deleted = apply(
        &mut replay,
        "delete-2",
        json!({ "ops": [{ "type": "annotations.delete", "ref": note }] }),
    )
    .expect("deleted");
    assert_eq!(deleted["result"]["items"][0]["meta"]["changed"], json!([note, popup]));
    assert!(read(&mut replay, &popup).is_none());
}

/// A delete beside other ops on an `open.fatMem` session is all or none:
/// a later op's refusal takes the delete back, and the inline annotations
/// its promotion turned into objects still answer to their `baseIndex`
/// names through their births. The undo of a two-create change deletes
/// both.
#[test]
fn a_fat_memory_change_that_deletes_beside_other_ops_is_all_or_none() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let inline: Vec<Value> =
        refs(&before).into_iter().filter(|r| r["kind"] == "baseIndex").collect();
    assert!(inline.len() > 1, "the fixture has inline annotations");
    let refused = apply(
        &mut replay,
        "refused-1",
        json!({ "ops": [
            { "type": "annotations.delete", "ref": inline[0] },
            { "type": "annotations.delete",
              "ref": { "kind": "objectNumber", "page": page_ref(pon), "objectNumber": 9_999 } },
        ] }),
    )
    .expect_err("the second op is refused");
    assert_eq!(refused["code"], "NotFound", "{refused}");
    assert_eq!(listed(&mut replay, pon), before, "the delete was left behind");
    for (at, reference) in inline.iter().enumerate() {
        replay
            .replay(&json!({ "kind": "annotations.update", "jobId": "<job>", "opId": "<op>",
                "docId": DOC, "ref": reference, "patch": { "contents": format!("still {at}") } }))
            .unwrap_or_else(|e| panic!("{reference} no longer resolves: {e}"));
    }

    let square = |x: f64| {
        json!({ "type": "annotations.create", "page": page_ref(pon),
        "data": { "subtype": "square", "box": { "x": x, "y": 10, "width": 20, "height": 20 } } })
    };
    let without_them = listed(&mut replay, pon);
    apply(&mut replay, "create-2", json!({ "ops": [square(10.0), square(40.0)] }))
        .expect("created");
    assert_eq!(listed(&mut replay, pon).len(), without_them.len() + 2);
    apply(&mut replay, "undo-2", json!({ "undoOf": "create-2" })).expect("undone");
    assert_eq!(listed(&mut replay, pon), without_them, "the undo left a square behind");
}

// ── Ported from `runAnnotationMutationConformance` ─────────────────────────

/// "delete by objectNumber removes the annotation and reports a stable id".
#[test]
fn a_created_annotation_deleted_by_its_number_is_named_and_gone() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let created = replay
        .replay(&json!({ "kind": "annotations.create", "jobId": "<job>", "docId": DOC,
            "page": page_ref(pon), "draft": { "subtype": "highlight",
            "contents": "mutation conformance: to-delete",
            "quadPoints": [{ "upperLeft": { "x": 72, "y": 72 }, "upperRight": { "x": 200, "y": 72 },
                "lowerLeft": { "x": 72, "y": 92 }, "lowerRight": { "x": 200, "y": 92 } }] } }))
        .expect("created");
    let reference = created["result"]["annotation"]["ref"].clone();
    let before = listed(&mut replay, pon);
    let deleted = replay
        .replay(&json!({ "kind": "annotations.delete", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "ref": reference }))
        .expect("deleted");
    assert_eq!(deleted["event"]["deleted"], json!([reference]));
    let meta = &deleted["result"]["meta"];
    assert_eq!(meta["affectedPages"], json!([page_ref(pon)]));
    assert!(meta.get("cacheDelta").is_some(), "{meta}");
    assert_eq!(meta["changed"], json!([reference]));
    assert_eq!(listed(&mut replay, pon).len(), before.len() - 1);
}

/// "delete of an annotation born inline reports its baseIndex name; the
/// others keep theirs".
#[test]
fn an_inline_annotation_deleted_is_named_by_its_birth_and_the_others_keep_theirs() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let inline = before.iter().find(|a| a["ref"]["kind"] == "baseIndex").expect("an inline one");
    let deleted = replay
        .replay(&json!({ "kind": "annotations.delete", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "ref": inline["ref"] }))
        .expect("deleted");
    let changed = deleted["result"]["meta"]["changed"].as_array().cloned().unwrap_or_default();
    assert_eq!(changed[0], inline["ref"]);
    let after = refs(&listed(&mut replay, pon));
    let expected: Vec<Value> = refs(&before).into_iter().filter(|r| !changed.contains(r)).collect();
    assert_eq!(after, expected, "every other annotation keeps its name");

    // The name is never reused: it now names nothing.
    let again = replay
        .replay(&json!({ "kind": "annotations.update", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "ref": inline["ref"], "patch": { "contents": "gone" } }))
        .expect_err("a deleted name");
    assert_eq!(again["code"], "NotFound", "{again}");
}

/// A server can send the record an undo runs (`document.applyChanges`'s
/// `record`). One whose restore step carries no capture of what the delete
/// removed can't bring it back, and is refused rather than claimed.
#[test]
fn a_server_record_with_no_capture_is_refused() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "the-last-annotation").expect("a case");
    let pon = case.page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let authority = json!({ "identity": {}, "grants": null });
    let deleted = replay
        .replay(&json!({ "kind": "document.applyChanges", "jobId": "<job>", "docId": DOC,
            "changes": [{ "opId": "s1", "authority": authority,
                "change": { "ops": [{ "type": "annotations.delete", "ref": case.annotation_ref }] } }] }))
        .expect("the job runs");
    let mut record = deleted["outcomes"][0]["record"].clone();
    let step = &mut record["steps"][0];
    assert!(step["capture"].is_string(), "the delete captured: {record}");
    step["capture"] = Value::Null;

    let undone = replay
        .replay(&json!({ "kind": "document.applyChanges", "jobId": "<job>", "docId": DOC,
            "changes": [{ "opId": "s2", "authority": authority, "change": { "undoOf": "s9" },
                "record": record }] }))
        .expect("the job runs");
    let outcome = &undone["outcomes"][0];
    assert_eq!(outcome["status"], "refused", "{undone}");
    assert_eq!(outcome["error"]["code"], "InvalidArg", "{undone}");
    assert_eq!(
        names(&listed(&mut replay, pon)),
        case.remaining,
        "the refused undo wrote something"
    );
}

/// The document as an incremental save writes it.
fn saved(replay: &mut Replay) -> Vec<u8> {
    replay
        .send(
            &json!({ "kind": "document.saveBuffer", "jobId": "<job>", "docId": DOC,
                "mode": "incremental" }),
            Vec::new(),
        )
        .payloads
        .remove(0)
}

/// A delete promotes the page's inline annotations before it checks who may
/// delete what; a refusal there aborts the write's layer transaction, which
/// takes the promotion back with it.
#[test]
fn a_delete_refused_after_its_promotion_leaves_the_page_as_it_was() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let bytes_before = saved(&mut replay);
    let inline = refs(&before).into_iter().find(|r| r["kind"] == "baseIndex").expect("inline");
    // May delete only its own annotations; the inline one is no one's.
    let bits = json!({ "bit3": true, "bit4": true, "bit5": true, "bit6": true, "bit9": true,
        "bit10": true, "bit11": true, "bit12": true });
    let refused = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "refused", "change": { "ops": [{ "type": "annotations.delete", "ref": inline }] },
            "authority": { "identity": { "userId": "someone" },
                "grants": { "scope": ["doc.open", "doc.annotate.modify", "annotations:delete:self"],
                    "pdfBits": bits } } }))
        .expect_err("not the caller's to delete");
    assert_eq!(refused["code"], "Forbidden", "{refused}");
    assert_eq!(listed(&mut replay, pon), before, "the refused delete changed the listing");
    assert_eq!(saved(&mut replay), bytes_before, "the promotion outlived the refusal");
}
