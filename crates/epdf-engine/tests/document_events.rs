//! The document events the host publishes, pinned against the payloads
//! `packages/engine/main` publishes for the same calls
//! (`core/src/events/DocumentEvent.ts`): what the event adds beside the
//! result, and when there is no event at all.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture, page_ref};

const RICH: &str = "tooling/engine-conformance/fixtures/rich_metadata.pdf";
const ROTATED: &str = "tooling/engine-conformance/fixtures/rotated_pages.pdf";
const KINDS: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";
const APPEARANCES: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
const FORMS: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";

/// Send `request` and answer the whole envelope, for the answers whose
/// event rides beside the result rather than inside it.
fn envelope(host: &mut Host, mut request: Value, payloads: Vec<Vec<u8>>) -> Value {
    request["jobId"] = json!(5);
    let response = host.call(&request.to_string(), payloads);
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{request} failed: {envelope}");
    envelope
}

fn page_refs(host: &mut Host, doc: &str) -> Vec<Value> {
    let list = call(host, json!({ "kind": "pages.list", "docId": doc }));
    list["pages"].as_array().expect("pages").iter().map(|p| p["ref"].clone()).collect()
}

/// The event carries every key of the result, unchanged, beside what it
/// adds.
fn assert_result_rides(answer: &Value) {
    let event = &answer["event"];
    for (key, value) in answer["result"].as_object().expect("an object result") {
        assert_eq!(&event[key], value, "the event changed '{key}': {answer}");
    }
}

#[test]
fn every_annotation_write_names_its_page() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, RICH);
    let page = page_ref(pon);

    let created = call(
        &mut host,
        json!({ "kind": "annotations.create", "docId": doc, "page": page,
                "draft": { "subtype": "square",
                           "box": { "x": 10.0, "y": 10.0, "width": 40.0, "height": 40.0 } } }),
    );
    // A create is a one-op change: its event is the item's
    // (`changeEvents`), on the page the create named.
    assert_eq!(created["event"]["type"], json!("annotations.created"), "{created}");
    assert_eq!(created["event"]["page"], page, "{created}");
    assert!(created["result"].get("page").is_none(), "the page leaked into the result");
    assert_result_rides(&created);
    let annotation = created["result"]["annotation"]["ref"].clone();

    let updated = call(
        &mut host,
        json!({ "kind": "annotations.update", "docId": doc, "ref": annotation,
                "patch": { "subtype": "square", "contents": "moved on" } }),
    );
    // An update is a one-op change too (Task 8.15): `annotations.updated`.
    assert_eq!(updated["event"]["type"], json!("annotations.updated"), "{updated}");
    assert_eq!(updated["event"]["page"], page, "{updated}");
    assert!(updated["result"].get("page").is_none(), "the page leaked into the result");
    assert_result_rides(&updated);
    let annotation = updated["result"]["annotation"]["ref"].clone();

    // A reorder and a delete are one-op changes too (Task 8.16).
    let moved = call(
        &mut host,
        json!({ "kind": "annotations.reorder", "docId": doc, "page": page,
                "refs": [annotation], "position": "start" }),
    );
    assert_eq!(moved["event"]["type"], json!("annotations.reordered"), "{moved}");
    assert_eq!(moved["event"]["page"], page, "{moved}");
    assert!(moved["result"].get("page").is_none(), "the page leaked into the result");
    assert_result_rides(&moved);

    let deleted =
        call(&mut host, json!({ "kind": "annotations.delete", "docId": doc, "ref": annotation }));
    assert_eq!(deleted["event"]["type"], json!("annotations.deleted"), "{deleted}");
    assert_eq!(deleted["event"]["page"], page, "{deleted}");
    // `deletedAnnotationsOf`: what went, beside the result.
    assert_eq!(deleted["event"]["deleted"], json!([annotation]), "{deleted}");
    assert_result_rides(&deleted);
}

/// One page op as a `document.apply` change under `op_id`, answering its
/// one item and the one event the change publishes for it.
fn page_op(host: &mut Host, doc: &str, op_id: &str, op: Value) -> (Value, Value) {
    let answer = call(
        host,
        json!({ "kind": "document.apply", "docId": doc, "opId": op_id,
                "change": { "ops": [op] } }),
    );
    let items = answer["result"]["items"].as_array().expect("items");
    let events = answer["events"].as_array().expect("events");
    assert_eq!((items.len(), events.len()), (1, 1), "{answer}");
    assert_eq!(events[0]["origin"]["tx"], json!({ "id": op_id, "index": 0, "count": 1 }));
    (items[0].clone(), events[0].clone())
}

/// The event carries the item, less its `type`, unchanged
/// (`changeEvents`' `itemEvents`).
fn assert_item_rides(item: &Value, event: &Value) {
    for (key, value) in item.as_object().expect("an object item") {
        if key != "type" {
            assert_eq!(&event[key], value, "the event changed '{key}': {event}");
        }
    }
}

#[test]
fn a_reorder_names_the_block_in_the_callers_order() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    assert!(pages.len() >= 2, "the fixture needs two pages");
    // The caller's order, not the document's.
    let block = json!([pages[1], pages[0]]);
    let (item, event) = page_op(
        &mut host,
        &doc,
        "op-reorder",
        json!({ "type": "pages.reorder", "pages": block, "position": "start" }),
    );
    assert_eq!(event["type"], json!("pages.reordered"), "{event}");
    assert_eq!(item["pages"], block);
    assert_eq!(item["meta"]["affectedPages"], json!([]));
    assert!(event.get("destIndex").is_none(), "{event}");
    assert_item_rides(&item, &event);
}

#[test]
fn a_rotation_names_its_pages_and_its_rotation() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    let (item, event) = page_op(
        &mut host,
        &doc,
        "op-rotate",
        json!({ "type": "pages.rotate", "pages": [pages[0]], "rotation": 90 }),
    );
    assert_eq!(event["type"], json!("pages.rotated"), "{event}");
    assert_eq!(event["pages"], json!([pages[0]]));
    assert_eq!(event["rotation"], json!(90));
    assert_eq!(event["layout"]["pages"][0]["rotation"], json!(90));
    assert_item_rides(&item, &event);
}

#[test]
fn a_delete_names_the_retired_pages() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    let (item, event) = page_op(
        &mut host,
        &doc,
        "op-delete",
        json!({ "type": "pages.delete", "pages": [pages[0]] }),
    );
    assert_eq!(event["type"], json!("pages.deleted"), "{event}");
    assert_eq!(event["pages"], json!([pages[0]]));
    assert_eq!(event["layout"]["pageCount"], json!(pages.len() - 1));
    assert_item_rides(&item, &event);
}

#[test]
fn an_insert_names_the_new_pages_and_no_insertion_point() {
    // `destIndex` is gone with the neighbour positions (1233d1349): the
    // event is the result, `{insertedPages, layout, meta}`, whatever the
    // position said.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let size = json!({ "width": 200.0, "height": 300.0 });
    let (placed, event) = page_op(
        &mut host,
        &doc,
        "op-placed",
        json!({ "type": "pages.insertBlank", "spec": { "size": size }, "position": "start" }),
    );
    assert_eq!(event["type"], json!("pages.inserted"), "{event}");
    assert!(event.get("destIndex").is_none(), "{event}");
    assert_eq!(event["insertedPages"], json!([event["layout"]["pages"][0]["ref"]]));
    assert_item_rides(&placed, &event);

    let (_, appended) = page_op(
        &mut host,
        &doc,
        "op-appended",
        json!({ "type": "pages.insertBlank", "spec": { "size": size } }),
    );
    assert_eq!(appended["type"], json!("pages.inserted"), "{appended}");
    let pages = appended["layout"]["pages"].as_array().expect("pages");
    assert_eq!(appended["insertedPages"], json!([pages.last().expect("a page")["ref"]]));
}

#[test]
fn a_metadata_write_answers_its_meta_and_publishes_one_fact_of_its_write() {
    // `MetadataUpdateResult`: the re-read metadata and the write's meta,
    // which names no page and has no cache delta locally. The event is one
    // fact of the write, under its `opId` (`publishWrite`).
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let updated = call(
        &mut host,
        json!({ "kind": "metadata.update", "docId": doc, "opId": "meta-1",
                "patch": { "title": "x" } }),
    );
    assert_eq!(updated["event"]["type"], json!("metadata.updated"), "{updated}");
    assert_eq!(
        updated["result"]["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "meta-1", "undoable": true }),
        "{updated}"
    );
    assert!(updated["result"].get("cache").is_none(), "{updated}");
    assert_eq!(
        updated["event"]["origin"]["tx"],
        json!({ "id": "meta-1", "index": 0, "count": 1 }),
        "{updated}"
    );
    assert_result_rides(&updated);

    let custom = call(
        &mut host,
        json!({ "kind": "metadata.updateCustom", "docId": doc, "patch": { "Project": "x" } }),
    );
    assert_eq!(custom["event"]["type"], json!("metadata.customUpdated"), "{custom}");
    assert_eq!(custom["result"]["custom"]["Project"], json!("x"), "{custom}");
    assert_result_rides(&custom);
}

#[test]
fn a_piece_info_write_publishes_nothing() {
    // `LocalPieceInfoService` publishes no event. This one used to go out as
    // `metadata.updated` with no `metadata`, which the metadata plugin then
    // published as the document's metadata.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let written = call(
        &mut host,
        json!({ "kind": "pieceInfo.update", "docId": doc, "application": "Epdf",
                "opId": "op-update", "patch": { "Flag": true } }),
    );
    assert!(written.get("event").is_none(), "{written}");
    assert_eq!(written["mutationSequence"], json!(1), "the write is still a mutation");
    // `PieceInfoUpdateResult`: the application's data read back, and `meta`.
    assert_eq!(
        written["result"]["pieceInfo"]["entries"]["Flag"],
        json!({ "type": "boolean", "value": true }),
        "{written}"
    );
    assert_eq!(
        written["result"]["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "op-update",
                "undoable": false }),
        "{written}"
    );

    // `pieceInfo.delete` (a9434c0e2): only `meta`.
    let deleted = call(
        &mut host,
        json!({ "kind": "pieceInfo.delete", "docId": doc, "application": "Epdf",
                "opId": "op-delete" }),
    );
    assert!(deleted.get("event").is_none(), "{deleted}");
    assert_eq!(deleted["mutationSequence"], json!(2), "{deleted}");
    assert_eq!(
        deleted["result"],
        json!({ "meta": { "affectedPages": [], "cacheDelta": null, "opId": "op-delete",
                          "undoable": false } }),
    );
    let read =
        call(&mut host, json!({ "kind": "pieceInfo.read", "docId": doc, "application": "Epdf" }));
    assert_eq!(read, Value::Null, "the application is gone");
}

#[test]
fn attachment_writes_publish_the_typescript_events() {
    // `document.apply` answers each item's events beside its result, as
    // `LocalDocumentChanges.apply` publishes them (`changeEvents`).
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let created = envelope(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "op-create",
                "change": { "ops": [{ "type": "attachments.create",
                    "file": { "name": "notes.txt", "mimeType": "text/plain" },
                    "resources": { "file": { "payload": 0 } } }] } }),
        vec![b"hello".to_vec()],
    )["result"]
        .clone();
    let item = &created["result"]["items"][0];
    assert_eq!(item["type"], json!("attachments.create"), "{created}");
    assert_eq!(item["attachment"]["ref"], json!({ "kind": "key", "key": "notes.txt" }));
    assert_eq!(item["meta"]["changed"], json!([{ "kind": "key", "key": "notes.txt" }]));
    let event = &created["events"][0];
    assert_eq!(event["type"], json!("attachments.created"), "{created}");
    assert_eq!(event["attachment"], item["attachment"], "{created}");
    assert_eq!(event["meta"], item["meta"], "{created}");

    let deleted = envelope(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "op-delete",
                "change": { "ops": [{ "type": "attachments.delete",
                    "ref": { "kind": "key", "key": "notes.txt" } }] } }),
        Vec::new(),
    )["result"]
        .clone();
    let item = &deleted["result"]["items"][0];
    assert_eq!(item["type"], json!("attachments.delete"), "{deleted}");
    assert!(item.get("attachment").is_none(), "{deleted}");
    let event = &deleted["events"][0];
    assert_eq!(event["type"], json!("attachments.deleted"), "{deleted}");
    // `deletedAttachmentOf`: what the delete removed.
    assert_eq!(event["deleted"], json!({ "kind": "key", "key": "notes.txt" }), "{deleted}");
    assert_eq!(event["meta"], item["meta"], "{deleted}");
}

#[test]
fn a_form_change_publishes_each_ops_fact() {
    // `changeEvents`: a value write is `forms.valueSet`, a script's display
    // effect `forms.updated`, each the op's item without its `type`, one
    // fact of the change each, under its `opId`.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let applied = call(
        &mut host,
        json!({ "kind": "document.apply", "docId": doc, "opId": "script-1", "change": { "ops": [
            { "type": "forms.setValue", "field": text, "value": { "value": "Ada" } },
            { "type": "forms.setDisplay", "field": text, "display": "hidden" },
        ] } }),
    );
    let events = applied["events"].as_array().expect("events");
    let types: Vec<&Value> = events.iter().map(|e| &e["type"]).collect();
    assert_eq!(types, [&json!("forms.valueSet"), &json!("forms.updated")], "{applied}");
    for (index, (event, item)) in
        events.iter().zip(applied["result"]["items"].as_array().unwrap()).enumerate()
    {
        assert_eq!(
            event["origin"]["tx"],
            json!({ "id": "script-1", "index": index, "count": 2 }),
            "{applied}"
        );
        assert_item_rides(item, event);
    }
}

#[test]
fn a_page_flatten_carries_meta_and_publishes_only_a_change() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, APPEARANCES);
    let flattened = call(
        &mut host,
        json!({ "kind": "pages.flatten", "docId": doc, "pages": [page_ref(pon)], "usage": "display" }),
    );
    assert_eq!(flattened["event"]["type"], json!("pages.flattened"), "{flattened}");
    assert_eq!(flattened["result"]["results"][0]["status"], json!("applied"));
    let meta = &flattened["result"]["meta"];
    assert_eq!(meta["affectedPages"], json!([page_ref(pon)]), "{flattened}");
    assert_eq!(meta["cacheDelta"], Value::Null);
    assert_eq!(meta["undoable"], json!(false), "a flatten is final");
    // The finish notes the mutation once, as `WorkerHost.finishMutation`
    // does.
    assert_eq!(flattened["mutationSequence"], json!(1), "{flattened}");
    assert_eq!(flattened["wrote"], json!(true));
    assert_result_rides(&flattened);

    // Flat already: unchanged, which is no mutation.
    let again = call(
        &mut host,
        json!({ "kind": "pages.flatten", "docId": doc, "pages": [page_ref(pon)], "usage": "display" }),
    );
    assert_eq!(again["result"]["results"][0]["status"], json!("unchanged"), "{again}");
    assert!(again.get("event").is_none(), "{again}");
    assert_eq!(again["result"]["meta"]["affectedPages"], json!([]), "{again}");
    assert_eq!(again["wrote"], json!(false));
    assert_eq!(again["mutationSequence"], json!(1), "{again}");
}

#[test]
fn a_redaction_carries_meta_and_publishes_only_a_change() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, KINDS);
    let scope = json!({ "pages": [page_ref(pon)] });
    let applied =
        call(&mut host, json!({ "kind": "redaction.apply", "docId": doc, "scope": scope }));
    assert_eq!(applied["event"]["type"], json!("redaction.applied"), "{applied}");
    assert_eq!(applied["result"]["scope"], scope, "{applied}");
    assert_eq!(applied["result"]["results"][0]["status"], json!("applied"));
    let meta = &applied["result"]["meta"];
    assert_eq!(meta["affectedPages"], json!([page_ref(pon)]), "{applied}");
    assert_eq!(meta["cacheDelta"], Value::Null);
    assert_eq!(meta["undoable"], json!(false), "a redaction is final");
    // The finish notes the mutation once, as `WorkerHost.finishMutation`
    // does.
    assert_eq!(applied["mutationSequence"], json!(1), "{applied}");
    assert_eq!(applied["wrote"], json!(true));
    assert_result_rides(&applied);

    // Nothing left to apply: unchanged, which is no mutation. The answer
    // still carries the sequence it found (a kept divergence, 87b61b533).
    let again = call(&mut host, json!({ "kind": "redaction.apply", "docId": doc, "scope": scope }));
    assert_eq!(again["result"]["results"][0]["status"], json!("unchanged"), "{again}");
    assert!(again.get("event").is_none(), "{again}");
    assert_eq!(again["result"]["meta"]["affectedPages"], json!([]), "{again}");
    assert_eq!(again["wrote"], json!(false));
    assert_eq!(again["mutationSequence"], json!(1), "{again}");
}

const SIGNING: &str = "packages/engine/main/test/fixtures/unsigned_sigfield.pdf";

#[test]
fn a_parked_signing_publishes_prepared_and_its_cancel_publishes_cancelled() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SIGNING);
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["snapshot"]
        ["signatures"][0]["field"]
        .clone();
    let prepared = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    let signing_id = prepared["result"]["result"]["signingId"].clone();
    assert!(signing_id.is_string(), "{prepared}");
    // Beside the answer, which stays `{ result: SignaturePrepared }`.
    assert!(prepared["result"].get("event").is_none(), "{prepared}");
    let event = &prepared["event"];
    assert_eq!(event["type"], json!("signatures.prepared"), "{prepared}");
    assert_eq!(event["signingId"], signing_id);
    assert_eq!(event["field"], field);
    // `{ field, ...SignaturePrepared }`: the candidate as the call answered it.
    for (key, value) in prepared["result"]["result"].as_object().expect("the candidate") {
        assert_eq!(&event[key], value, "{key}: {prepared}");
    }
    assert_eq!(event["origin"]["kind"], json!("local"));
    // The one fact of its write (`publishWrite`).
    assert!(event["origin"]["tx"]["id"].is_string(), "{prepared}");
    assert_eq!(event["origin"]["tx"]["index"], json!(0));
    assert_eq!(event["origin"]["tx"]["count"], json!(1));

    // `signatures.cancel` (7bccc4f35) publishes `signatures.cancelled`.
    let aborted = envelope(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": doc, "signingId": signing_id }),
        Vec::new(),
    );
    assert_eq!(aborted["result"]["result"]["status"], json!("cancelled"), "{aborted}");
    assert_eq!(
        aborted["event"].as_object().map(|e| {
            let mut keys: Vec<_> = e.keys().cloned().collect();
            keys.sort();
            keys
        }),
        Some(vec!["origin".to_string(), "signingId".to_string(), "type".to_string()]),
        "{aborted}",
    );
    assert_eq!(aborted["event"]["type"], json!("signatures.cancelled"));
    assert_eq!(aborted["event"]["origin"]["tx"]["count"], json!(1), "{aborted}");
    assert_eq!(aborted["event"]["signingId"], signing_id);

    // Nothing parked any more: not a cancel, so no notice.
    let nothing = envelope(
        &mut host,
        json!({ "kind": "signatures.cancel", "docId": doc, "signingId": signing_id }),
        Vec::new(),
    );
    assert_eq!(nothing["result"]["result"]["status"], json!("unknown"), "{nothing}");
    assert!(nothing.get("event").is_none(), "{nothing}");
}

/// Answer a form write, pinning what every one of them shares: the
/// event that spreads the result beside its type, and a sequence that moved
/// past `last`.
fn form_write(
    host: &mut Host,
    last: &mut u64,
    request: Value,
    payloads: Vec<Vec<u8>>,
    event: &str,
) -> Value {
    let answer = envelope(host, request.clone(), payloads)["result"].clone();
    assert_eq!(answer["event"]["type"], json!(event), "{answer}");
    assert_result_rides(&answer);
    let sequence = answer["mutationSequence"].as_u64().expect("a mutation sequence");
    assert!(sequence > *last, "{request} did not move the sequence past {last}: {answer}");
    *last = sequence;
    answer
}

#[test]
fn every_form_write_carries_meta() {
    // `FormMutationMeta` for the value writes: the write's pages, `opId`
    // and whether it can be undone, beside the fields and widgets it
    // changed. A repair's is `emptyMeta`: no page, and no undo.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let mut last = 0;
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let widget = json!({ "ref": { "kind": "objectNumber", "page": page_ref(3u64),
        "objectNumber": 4 }, "objectNumber": 4, "page": page_ref(3u64) });

    let set = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.setValue", "docId": doc, "opId": "fill-1", "ref": text,
                "value": { "value": "Ada" } }),
        Vec::new(),
        "forms.valueSet",
    );
    assert_eq!(
        set["result"]["meta"],
        json!({ "affectedPages": [page_ref(3u64)], "cacheDelta": null, "opId": "fill-1",
                "undoable": true, "changedFields": [text], "changedWidgets": [widget],
                "redrawnWidgets": [widget] }),
        "{set}"
    );
    assert_eq!(set["result"]["field"]["value"], json!("Ada"), "{set}");
    assert_eq!(set["result"]["widgets"][0]["ref"], widget["ref"], "{set}");
    assert_eq!(set["event"]["origin"]["tx"], json!({ "id": "fill-1", "index": 0, "count": 1 }));

    // A reset publishes one `forms.valueSet` per field it changed
    // (`formResetFacts`), as `events`.
    let reset = envelope(
        &mut host,
        json!({ "kind": "forms.reset", "docId": doc, "opId": "reset-1", "refs": [text] }),
        Vec::new(),
    )["result"]
        .clone();
    assert_eq!(reset["result"]["fields"][0]["value"], json!("unset"), "{reset}");
    assert_eq!(reset["result"]["meta"]["changedFields"], json!([text]), "{reset}");
    let events = reset["events"].as_array().expect("events");
    assert_eq!(events.len(), 1, "{reset}");
    assert_eq!(events[0]["type"], json!("forms.valueSet"), "{reset}");
    assert_eq!(events[0]["field"], reset["result"]["fields"][0], "{reset}");
    assert_eq!(events[0]["meta"]["changedFields"], json!([text]), "{reset}");
    let sequence = reset["mutationSequence"].as_u64().expect("a sequence");
    assert!(sequence > last, "{reset}");
    last = sequence;

    let repaired = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.repair", "docId": doc, "opId": "repair-1" }),
        Vec::new(),
        "forms.repaired",
    );
    assert_eq!(
        repaired["result"]["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "repair-1", "undoable": false }),
        "{repaired}"
    );
    assert_eq!(repaired["result"]["needsAppearancesCleared"], json!(false), "{repaired}");
    assert_eq!(
        repaired["event"]["origin"]["tx"],
        json!({ "id": "repair-1", "index": 0, "count": 1 }),
        "{repaired}"
    );
}

#[test]
fn every_field_edit_carries_meta() {
    // The field and widget verbs are one-op changes (2445caba7, 1233d1349):
    // each answers its item with `FormMutationMeta` (its `opId`, and that it
    // can be undone) and publishes its one event.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let mut last = 0;
    let text = json!({ "kind": "objectNumber", "objectNumber": 4 });
    let check = |answer: &Value, op_id: &str| {
        assert_eq!(answer["result"]["meta"]["opId"], json!(op_id), "{answer}");
        assert_eq!(answer["result"]["meta"]["undoable"], json!(true), "{answer}");
        assert_eq!(answer["event"]["origin"]["tx"], json!({ "id": op_id, "index": 0, "count": 1 }));
    };
    let created = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.createField", "docId": doc, "opId": "create-1",
                "draft": { "family": "text", "name": "new.plain" } }),
        Vec::new(),
        "forms.created",
    );
    check(&created, "create-1");
    assert_eq!(created["result"]["field"]["name"], json!("new.plain"), "{created}");
    let plain = created["result"]["field"]["ref"].clone();
    assert_eq!(created["result"]["meta"]["changedFields"], json!([plain]), "{created}");
    let updated = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.updateField", "docId": doc, "opId": "update-1", "ref": text,
                "patch": { "family": "text", "readOnly": true } }),
        Vec::new(),
        "forms.updated",
    );
    check(&updated, "update-1");
    assert_eq!(updated["result"]["field"]["readOnly"], json!(true), "{updated}");
    assert_eq!(updated["result"]["meta"]["changedFields"], json!([text]), "{updated}");

    let radio = json!({ "kind": "objectNumber", "objectNumber": 9 });
    let widget = |annot: u64| json!({ "kind": "objectNumber", "page": page_ref(3u64), "objectNumber": annot });
    let detached = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.detachWidget", "docId": doc, "opId": "detach-1", "ref": radio,
                "widget": widget(7) }),
        Vec::new(),
        "forms.widgetRemoved",
    );
    check(&detached, "detach-1");
    assert_eq!(detached["result"]["widgets"][0]["field"], Value::Null, "{detached}");
    let added = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.addWidget", "docId": doc, "opId": "add-1", "ref": radio,
                "placement": { "page": page_ref(3u64), "exportValue": "Green",
                    "rect": { "x": 300, "y": 20, "width": 20, "height": 20 } } }),
        Vec::new(),
        "forms.widgetAdded",
    );
    check(&added, "add-1");
    let new_widget = added["result"]["meta"]["changedWidgets"][0]["ref"].clone();
    assert_eq!(added["result"]["widgets"][0]["ref"], new_widget, "{added}");
    let restyled = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.updateWidget", "docId": doc, "opId": "style-1",
                "widget": new_widget, "patch": { "interiorColor": "#ffeeaa" } }),
        Vec::new(),
        "forms.widgetUpdated",
    );
    check(&restyled, "style-1");
    assert_eq!(restyled["result"]["widget"]["interiorColor"], json!("#ffeeaa"), "{restyled}");
    let reordered = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.reorderWidgets", "docId": doc, "opId": "order-1",
                "page": page_ref(3u64), "widgets": [new_widget], "position": "start" }),
        Vec::new(),
        "forms.widgetsReordered",
    );
    check(&reordered, "order-1");
    assert_eq!(reordered["result"]["order"][0], new_widget, "{reordered}");
    let deleted = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.deleteWidget", "docId": doc, "opId": "drop-1",
                "widget": new_widget }),
        Vec::new(),
        "forms.widgetDeleted",
    );
    check(&deleted, "drop-1");
    assert_eq!(deleted["result"]["field"]["ref"], radio, "{deleted}");
}

#[test]
fn a_field_delete_removes_its_widgets_and_names_them() {
    // `forms.delete` takes the field's widgets off their pages in the same
    // write, and its meta names them, which the render plugin reads to
    // repaint; its event names the field it removed (`deletedFieldOf`).
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let before = on_page(&mut host, &doc);
    assert!(before.contains(&7) && before.contains(&8), "the fixture's radio widgets: {before:?}");
    let mut last = 0;
    let radio = json!({ "kind": "objectNumber", "objectNumber": 9 });
    let deleted = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.deleteField", "docId": doc, "ref": radio }),
        Vec::new(),
        "forms.deleted",
    );
    let result = deleted["result"].as_object().expect("a result");
    assert_eq!(result.keys().collect::<Vec<_>>(), vec!["meta"], "{deleted}");
    assert_eq!(deleted["result"]["meta"]["changedFields"], json!([radio]), "{deleted}");
    let removed = deleted["result"]["meta"]["changedWidgets"].as_array().expect("widgets");
    let removed: Vec<_> = removed.iter().map(|w| w["objectNumber"].clone()).collect();
    assert_eq!(removed, vec![json!(7), json!(8)], "{deleted}");
    assert_eq!(deleted["event"]["deleted"], radio, "{deleted}");

    let after = on_page(&mut host, &doc);
    assert!(!after.contains(&7) && !after.contains(&8), "the widgets are still there: {after:?}");
}

/// The object numbers of page 3's annotations and widgets: the widgets are
/// the form's (`forms.list`), the rest `annotations.list`'s (1233d1349).
fn on_page(host: &mut Host, doc: &str) -> Vec<u64> {
    let list =
        call(host, json!({ "kind": "annotations.list", "docId": doc, "pages": [page_ref(3u64)] }));
    let form = call(host, json!({ "kind": "forms.list", "docId": doc }));
    let widgets = form["widgets"].as_array().unwrap_or_else(|| panic!("widgets: {form}"));
    list["annotations"]
        .as_array()
        .unwrap_or_else(|| panic!("annotations: {list}"))
        .iter()
        .chain(widgets.iter().filter(|w| w["page"] == page_ref(3u64)))
        .filter_map(|a| a["ref"]["objectNumber"].as_u64())
        .collect()
}

/// `flatten-schema.test.ts`: a flatten's result keeps its request beside
/// its outcome, for audit replay, and reads back as it was written.
#[test]
fn a_flatten_result_keeps_its_request_for_replay() {
    use epdf_engine::features::pages_write::FlattenResult;
    let flattened = json!({ "pages": [page_ref(12u64), page_ref(18u64)], "usage": "print",
        "results": [{ "page": page_ref(12u64), "status": "applied" },
                    { "page": page_ref(18u64), "status": "unchanged" }],
        "meta": { "affectedPages": [], "cacheDelta": null, "opId": "op-1", "undoable": false } });
    let read: FlattenResult = serde_json::from_value(flattened.clone()).expect("a TS flatten");
    assert_eq!(serde_json::to_value(&read).unwrap(), flattened);
    let failed = json!({ "pages": [page_ref(3u64)], "usage": "display",
        "results": [{ "page": page_ref(3u64), "status": "failed" }],
        "meta": { "affectedPages": [], "cacheDelta": null, "opId": "op-1", "undoable": false } });
    assert!(serde_json::from_value::<FlattenResult>(failed).is_err(), "no page status 'failed'");
}

/// A redaction as `RedactionApplier` answers it: the v3 scope, page refs,
/// and a `MutationMeta`. A page has no `failed` status: one that fails
/// fails the call, as a flatten's does.
#[test]
fn a_redaction_reads_and_writes_as_the_typescript_answers_it() {
    use epdf_engine::features::redaction::RedactionApplyResult;
    let redacted = json!({ "scope": { "pages": [page_ref(3u64)] },
        "results": [{ "page": page_ref(3u64), "status": "applied", "removedAnnotationCount": 0 }],
        "removedAnnotationCount": 0,
        "meta": { "affectedPages": [page_ref(3u64)], "cacheDelta": null, "opId": "op-1",
                  "undoable": false } });
    let read: RedactionApplyResult =
        serde_json::from_value(redacted.clone()).expect("a TS redaction");
    assert_eq!(serde_json::to_value(&read).unwrap(), redacted);
    let mut failed = redacted.clone();
    failed["results"][0]["status"] = json!("failed");
    assert!(
        serde_json::from_value::<RedactionApplyResult>(failed).is_err(),
        "no page status 'failed'"
    );
}
