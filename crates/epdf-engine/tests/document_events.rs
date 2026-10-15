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
                           "rect": { "left": 10.0, "bottom": 10.0, "right": 50.0, "top": 50.0 } } }),
    );
    assert_eq!(created["event"]["type"], json!("annotation.created"), "{created}");
    assert_eq!(created["event"]["page"], page, "{created}");
    assert!(created["result"].get("page").is_none(), "the page leaked into the result");
    assert_result_rides(&created);
    let annotation = created["result"]["created"]["ref"].clone();

    let updated = call(
        &mut host,
        json!({ "kind": "annotations.update", "docId": doc, "ref": annotation,
                "patch": { "subtype": "square", "contents": "moved on" } }),
    );
    assert_eq!(updated["event"]["type"], json!("annotation.updated"), "{updated}");
    assert_eq!(updated["event"]["page"], page, "{updated}");
    assert_result_rides(&updated);
    let annotation = updated["result"]["updated"]["ref"].clone();

    let moved = call(
        &mut host,
        json!({ "kind": "annotations.move", "docId": doc, "page": page,
                "refs": [annotation], "toIndex": 0 }),
    );
    assert_eq!(moved["event"]["type"], json!("annotation.moved"), "{moved}");
    assert_eq!(moved["event"]["page"], page, "{moved}");
    assert_result_rides(&moved);

    let deleted =
        call(&mut host, json!({ "kind": "annotations.delete", "docId": doc, "ref": annotation }));
    assert_eq!(deleted["event"]["type"], json!("annotation.deleted"), "{deleted}");
    assert_eq!(deleted["event"]["page"], page, "{deleted}");
    assert_result_rides(&deleted);
}

#[test]
fn a_move_names_the_block_and_the_insertion_point() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    assert!(pages.len() >= 2, "the fixture needs two pages");
    // The caller's order, not the document's.
    let block = json!([pages[1], pages[0]]);
    let moved = call(
        &mut host,
        json!({ "kind": "pages.move", "docId": doc, "pages": block, "destIndex": 0 }),
    );
    assert_eq!(moved["event"]["type"], json!("pages.moved"), "{moved}");
    assert_eq!(moved["event"]["pages"], block);
    assert_eq!(moved["event"]["destIndex"], json!(0));
    assert_eq!(moved["result"]["cache"], Value::Null);
    assert!(moved["result"].as_object().unwrap().contains_key("cache"), "{moved}");
    assert!(moved["result"].get("pages").is_none(), "the block leaked into the result");
    assert_result_rides(&moved);
}

#[test]
fn a_rotation_names_its_pages_and_its_rotation() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    let rotated = call(
        &mut host,
        json!({ "kind": "pages.rotate", "docId": doc, "pages": [pages[0]], "rotation": 90 }),
    );
    assert_eq!(rotated["event"]["type"], json!("pages.rotated"), "{rotated}");
    assert_eq!(rotated["event"]["pages"], json!([pages[0]]));
    assert_eq!(rotated["event"]["rotation"], json!(90));
    assert!(rotated["result"].as_object().unwrap().contains_key("cache"), "{rotated}");
    assert_result_rides(&rotated);
}

#[test]
fn a_delete_names_the_retired_pages() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, ROTATED);
    let pages = page_refs(&mut host, &doc);
    let deleted =
        call(&mut host, json!({ "kind": "pages.delete", "docId": doc, "pages": [pages[0]] }));
    assert_eq!(deleted["event"]["type"], json!("pages.deleted"), "{deleted}");
    assert_eq!(deleted["event"]["pages"], json!([pages[0]]));
    assert!(deleted["result"].as_object().unwrap().contains_key("cache"), "{deleted}");
    assert_result_rides(&deleted);
}

#[test]
fn an_insert_names_its_insertion_point_only_when_the_call_did() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let size = json!({ "width": 200.0, "height": 300.0 });
    let placed = call(
        &mut host,
        json!({ "kind": "pages.insertBlank", "docId": doc, "size": size, "destIndex": 0 }),
    );
    assert_eq!(placed["event"]["type"], json!("pages.inserted"), "{placed}");
    assert_eq!(placed["event"]["destIndex"], json!(0));
    assert!(placed["result"].get("destIndex").is_none(), "the index leaked into the result");
    assert!(placed["result"].as_object().unwrap().contains_key("cache"), "{placed}");
    assert_result_rides(&placed);

    let appended =
        call(&mut host, json!({ "kind": "pages.insertBlank", "docId": doc, "size": size }));
    assert_eq!(appended["event"]["type"], json!("pages.inserted"), "{appended}");
    assert!(appended["event"].get("destIndex").is_none(), "{appended}");
}

#[test]
fn a_metadata_write_answers_a_local_cache() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let updated = call(
        &mut host,
        json!({ "kind": "metadata.update", "docId": doc, "patch": { "title": "x" } }),
    );
    assert_eq!(updated["event"]["type"], json!("metadata.updated"), "{updated}");
    assert!(updated["result"].as_object().unwrap().contains_key("cache"), "{updated}");
    assert_eq!(updated["result"]["cache"], Value::Null);
    assert_result_rides(&updated);
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
                "patch": { "Flag": true } }),
    );
    assert!(written.get("event").is_none(), "{written}");
    assert_eq!(written["mutationSequence"], json!(1), "the write is still a mutation");
}

#[test]
fn attachment_writes_answer_the_typescript_results() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, RICH);
    let created = envelope(
        &mut host,
        json!({ "kind": "attachments.create", "docId": doc,
                "file": { "name": "notes.txt", "mimeType": "text/plain" } }),
        vec![b"hello".to_vec()],
    )["result"]
        .clone();
    assert_eq!(created["event"]["type"], json!("attachment.created"), "{created}");
    assert_eq!(created["result"]["created"]["key"], json!("notes.txt"));
    assert!(created["result"].as_object().unwrap().contains_key("cache"), "{created}");
    assert_result_rides(&created);

    let deleted = call(
        &mut host,
        json!({ "kind": "attachments.delete", "docId": doc,
                "ref": { "kind": "key", "key": "notes.txt" } }),
    );
    assert_eq!(deleted["event"]["type"], json!("attachment.deleted"), "{deleted}");
    assert_eq!(
        deleted["result"],
        json!({ "deleted": { "kind": "key", "key": "notes.txt" }, "cache": null }),
    );
    assert_result_rides(&deleted);
}

#[test]
fn an_effects_batch_publishes_form_effects_applied() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let effect = json!({ "kind": "setValue", "ref": { "kind": "objectNumber", "fieldObjectNumber": 4 },
                         "value": { "type": "text", "value": "Ada" } });
    let applied =
        call(&mut host, json!({ "kind": "forms.applyEffects", "docId": doc, "effects": [effect] }));
    assert_eq!(applied["event"]["type"], json!("form.effectsApplied"), "{applied}");
    assert_result_rides(&applied);

    // The same value again writes nothing: no mutation, no event.
    let again =
        call(&mut host, json!({ "kind": "forms.applyEffects", "docId": doc, "effects": [effect] }));
    assert!(again.get("event").is_none(), "{again}");
    assert!(again["meta"].is_null(), "{again}");
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
    let pins = &flattened["result"]["meta"]["affectedPages"];
    assert_eq!(pins.as_array().map(Vec::len), Some(1), "{flattened}");
    assert_eq!(pins[0]["page"], page_ref(pon));
    assert_eq!(pins[0]["revision"]["generation"], json!(1), "{flattened}");
    assert_eq!(flattened["result"]["meta"]["cacheDelta"], Value::Null);
    // The flattener notes the mutation once, and the finish skips this tag
    // as `WorkerHost.finishMutation` does.
    assert_eq!(flattened["mutationSequence"], json!(1), "{flattened}");
    assert_result_rides(&flattened);

    // Flat already: unchanged, which is no mutation.
    let again = call(
        &mut host,
        json!({ "kind": "pages.flatten", "docId": doc, "pages": [page_ref(pon)], "usage": "display" }),
    );
    assert_eq!(again["result"]["results"][0]["status"], json!("unchanged"), "{again}");
    assert!(again.get("event").is_none(), "{again}");
    assert_eq!(again["result"]["meta"], Value::Null);
    assert_eq!(again["mutationSequence"], json!(1), "{again}");
}

#[test]
fn a_redaction_carries_meta_and_publishes_only_a_change() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, KINDS);
    let scope = json!({ "kind": "pages", "pages": [page_ref(pon)] });
    let applied =
        call(&mut host, json!({ "kind": "redaction.apply", "docId": doc, "scope": scope }));
    assert_eq!(applied["event"]["type"], json!("redaction.applied"), "{applied}");
    assert_eq!(applied["result"]["results"][0]["status"], json!("applied"));
    assert_eq!(applied["result"]["meta"]["affectedPages"][0]["page"], page_ref(pon));
    assert_eq!(applied["mutationSequence"], json!(1), "{applied}");
    assert_result_rides(&applied);

    let again = call(&mut host, json!({ "kind": "redaction.apply", "docId": doc, "scope": scope }));
    assert_eq!(again["result"]["results"][0]["status"], json!("unchanged"), "{again}");
    assert!(again.get("event").is_none(), "{again}");
    assert_eq!(again["result"]["meta"], Value::Null);
    assert_eq!(again["mutationSequence"], json!(1), "{again}");
}

const SIGNING: &str = "packages/engine/main/test/fixtures/unsigned_sigfield.pdf";

#[test]
fn a_parked_signing_publishes_prepared_and_its_abort_publishes_aborted() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, SIGNING);
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"]
        [0]["field"]
        .clone();
    let prepared = envelope(
        &mut host,
        json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }),
        Vec::new(),
    );
    let signing_id = prepared["result"]["signingId"].clone();
    assert!(signing_id.is_string(), "{prepared}");
    // Beside the result, which stays the bare `SignaturePrepareResult`.
    assert!(prepared["result"].get("event").is_none(), "{prepared}");
    let event = &prepared["event"];
    assert_eq!(event["type"], json!("signature.prepared"), "{prepared}");
    assert_eq!(event["signingId"], signing_id);
    assert_eq!(event["field"], field);
    assert_eq!(event["origin"]["kind"], json!("local"));

    let aborted = envelope(
        &mut host,
        json!({ "kind": "signatures.abort", "docId": doc, "signingId": signing_id }),
        Vec::new(),
    );
    assert_eq!(aborted["result"]["status"], json!("aborted"), "{aborted}");
    assert_eq!(
        aborted["event"].as_object().map(|e| {
            let mut keys: Vec<_> = e.keys().cloned().collect();
            keys.sort();
            keys
        }),
        Some(vec!["origin".to_string(), "signingId".to_string(), "type".to_string()]),
        "{aborted}",
    );
    assert_eq!(aborted["event"]["type"], json!("signature.aborted"));
    assert_eq!(aborted["event"]["signingId"], signing_id);

    // Nothing parked any more: not an abort, so no notice.
    let nothing = envelope(
        &mut host,
        json!({ "kind": "signatures.abort", "docId": doc, "signingId": signing_id }),
        Vec::new(),
    );
    assert_ne!(nothing["result"]["status"], json!("aborted"), "{nothing}");
    assert!(nothing.get("event").is_none(), "{nothing}");
}

/// `EMPTY_FORM_META` and `FormMutator`'s `EMPTY_META`: a form write is
/// non-structural at the page-list level, so a local engine pins nothing.
fn empty_meta() -> Value {
    json!({ "affectedPages": [], "cacheDelta": null })
}

/// Answer a form write, pinning what every one of them shares: the
/// TypeScript result's `meta`, the event that spreads the result beside
/// its type, and a sequence that moved past `last`.
fn form_write(
    host: &mut Host,
    last: &mut u64,
    request: Value,
    payloads: Vec<Vec<u8>>,
    event: &str,
) -> Value {
    let answer = envelope(host, request.clone(), payloads)["result"].clone();
    assert_eq!(answer["result"]["meta"], empty_meta(), "{request}: {answer}");
    assert_eq!(answer["event"]["type"], json!(event), "{answer}");
    assert_result_rides(&answer);
    let sequence = answer["mutationSequence"].as_u64().expect("a mutation sequence");
    assert!(sequence > *last, "{request} did not move the sequence past {last}: {answer}");
    *last = sequence;
    answer
}

#[test]
fn every_form_write_carries_meta() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let mut last = 0;
    let text = json!({ "kind": "objectNumber", "fieldObjectNumber": 4 });

    let set = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.setValue", "docId": doc, "ref": text,
                "value": { "type": "text", "value": "Ada" } }),
        Vec::new(),
        "form.valueChanged",
    );
    assert!(set["result"]["field"].is_object(), "{set}");
    form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.reset", "docId": doc, "ref": text }),
        Vec::new(),
        "form.valueChanged",
    );

    let exported = host.call(
        &json!({ "kind": "forms.export", "jobId": 9, "docId": doc, "format": "xfdf" }).to_string(),
        Vec::new(),
    );
    let data = exported.payloads.into_iter().next().expect("the exported data");
    form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.import", "docId": doc, "format": "xfdf" }),
        vec![data],
        "form.imported",
    );
    form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.repair", "docId": doc }),
        Vec::new(),
        "form.repaired",
    );

    // The field edits answer `{field, meta}`, not the bare field.
    let created = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.createField", "docId": doc,
                "draft": { "family": "text", "name": "new.plain" } }),
        Vec::new(),
        "form.fieldCreated",
    );
    assert_eq!(created["result"]["field"]["name"], json!("new.plain"), "{created}");
    let updated = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.updateField", "docId": doc, "ref": text,
                "patch": { "family": "text", "readOnly": false } }),
        Vec::new(),
        "form.fieldUpdated",
    );
    assert_eq!(updated["result"]["field"]["fieldObjectNumber"], json!(4), "{updated}");

    let radio = json!({ "kind": "objectNumber", "fieldObjectNumber": 9 });
    let widget = |annot: u64| json!({ "kind": "objectNumber", "page": page_ref(3u64), "annotObjectNumber": annot });
    let detached = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.detachWidget", "docId": doc, "ref": radio, "widget": widget(7) }),
        Vec::new(),
        "form.widgetDetached",
    );
    assert!(detached["result"]["field"].is_object(), "{detached}");
    let attached = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.attachWidget", "docId": doc, "ref": radio, "widget": widget(7),
                "onState": "Red" }),
        Vec::new(),
        "form.widgetAttached",
    );
    assert!(attached["result"]["field"].is_object(), "{attached}");
}

#[test]
fn a_field_delete_removes_its_widgets_and_names_them() {
    // `WorkerHost.handleFormsDeleteField` deletes the widgets the field
    // let go of through the annotation plane, and answers them as
    // `removedWidgets`, which the render plugin reads to repaint.
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, FORMS);
    let before = on_page(&mut host, &doc);
    assert!(before.contains(&7) && before.contains(&8), "the fixture's radio widgets: {before:?}");
    let mut last = 0;
    let deleted = form_write(
        &mut host,
        &mut last,
        json!({ "kind": "forms.deleteField", "docId": doc,
                "ref": { "kind": "objectNumber", "fieldObjectNumber": 9 } }),
        Vec::new(),
        "form.fieldDeleted",
    );
    assert_eq!(deleted["result"]["deletedFieldObjectNumber"], json!(9), "{deleted}");
    let removed = deleted["result"]["removedWidgets"].as_array().expect("removedWidgets");
    let removed: Vec<_> = removed.iter().map(|w| w["annotObjectNumber"].clone()).collect();
    assert_eq!(removed, vec![json!(7), json!(8)], "{deleted}");
    assert!(deleted["result"].get("detachedWidgets").is_none(), "{deleted}");

    let after = on_page(&mut host, &doc);
    assert!(!after.contains(&7) && !after.contains(&8), "the widgets are still there: {after:?}");
}

/// The object numbers of page 3's annotations.
fn on_page(host: &mut Host, doc: &str) -> Vec<u64> {
    let list = call(
        host,
        json!({ "kind": "annotations.listRawPage", "docId": doc, "page": page_ref(3u64) }),
    );
    list["annotations"]
        .as_array()
        .unwrap_or_else(|| panic!("annotations: {list}"))
        .iter()
        .filter_map(|a| a["ref"]["annotObjectNumber"].as_u64())
        .collect()
}
