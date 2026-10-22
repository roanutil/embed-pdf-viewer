//! The document outline: `outline.read`, and the `document.apply` ops
//! `outline.create`, `outline.update`, `outline.move` and `outline.delete`.
//!
//! Ports `runOutlineConformance`
//! (`packages/engine/core/src/conformance/runOutlineConformance.ts`, run by
//! `packages/engine/main/test/outline.test.ts`) through the host, each verb
//! of `doc.outline` as the one-op change `outlineVerbs` sends, and the
//! prediction rules of `packages/engine/core/src/outline/predict.ts` the
//! conformance checks the answers against. The rest pins what upstream's
//! `outlineChanges.ts` does that the conformance doesn't reach: the undo and
//! redo of each op, an undo past a later change, a parent opened again, and
//! a refused change taken back whole. No vector covers the outline beyond
//! `wire/requests.json`.

mod common;

use epdf_engine::host::Host;
use serde_json::{Value, json};
use test_support::canonical_numbers;

const DOC: &str = "d";

/// A PDF file of `objects`, numbered from 1 (`pdfOf`).
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

/// `OUTLINE_FIXTURE_PDF`: two pages of 300 × 300 and an outline, five items
/// showing when it opens. A (6), open: to page 3 at (20, 280) in the file;
/// its child A1 (10) fits page 4. B (7): red and bold, a URI action. C (8),
/// closed: the named destination `chapter-two` (page 4, `/FitH` 250); its
/// child C1 (11) goes nowhere. D (9): a JavaScript action.
fn outline_fixture() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests 12 0 R >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Outlines /First 6 0 R /Last 9 0 R /Count 5 >>",
        "<< /Title (A) /Parent 5 0 R /Next 7 0 R /First 10 0 R /Last 10 0 R /Count 1 \
         /Dest [3 0 R /XYZ 20 280 null] >>",
        "<< /Title (B) /Parent 5 0 R /Prev 6 0 R /Next 8 0 R /C [1 0 0] /F 2 \
         /A << /S /URI /URI (https://example.com/) >> >>",
        "<< /Title (C) /Parent 5 0 R /Prev 7 0 R /Next 9 0 R /First 11 0 R /Last 11 0 R \
         /Count -1 /Dest (chapter-two) >>",
        "<< /Title (D) /Parent 5 0 R /Prev 8 0 R /A << /S /JavaScript /JS (app.alert\\(1\\)) >> >>",
        "<< /Title (A1) /Parent 6 0 R /Dest [4 0 R /Fit] >>",
        "<< /Title (C1) /Parent 8 0 R >>",
        "<< /Names [(chapter-two) [4 0 R /FitH 250]] >>",
    ])
}

/// `BROKEN_OUTLINE_FIXTURE_PDF`: One (4) and Two (5), whose `/Next` comes
/// back to One, and under One a direct item no one can name.
fn broken_fixture() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /Outlines 3 0 R >>",
        "<< /Type /Pages /Kids [6 0 R] /Count 1 >>",
        "<< /Type /Outlines /First 4 0 R /Last 5 0 R /Count 2 >>",
        "<< /Title (One) /Parent 3 0 R /Next 5 0 R /First << /Title (Direct) >> >>",
        "<< /Title (Two) /Parent 3 0 R /Prev 4 0 R /Next 4 0 R >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
    ])
}

/// `OUTLINE_TARGETS_FIXTURE_PDF`: targets stored every way files store
/// them. Shared one (6) and Shared two (7) use one action object (11), which
/// the link on page 3 (14) uses too; Hyperref (8) goes to the named
/// destination `section.1` of the `/Names` tree; Old dests (9) to the name
/// `/chapter` of the catalog's `/Dests`; By number (10) names its page by
/// index.
fn targets_fixture() -> Vec<u8> {
    pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R /Outlines 5 0 R /Names << /Dests 12 0 R >> /Dests 13 0 R >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [14 0 R] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
        "<< /Type /Outlines /First 6 0 R /Last 10 0 R /Count 5 >>",
        "<< /Title (Shared one) /Parent 5 0 R /Next 7 0 R /A 11 0 R >>",
        "<< /Title (Shared two) /Parent 5 0 R /Prev 6 0 R /Next 8 0 R /A 11 0 R >>",
        "<< /Title (Hyperref) /Parent 5 0 R /Prev 7 0 R /Next 9 0 R \
         /A << /S /GoTo /D (section.1) >> >>",
        "<< /Title (Old dests) /Parent 5 0 R /Prev 8 0 R /Next 10 0 R /Dest /chapter >>",
        "<< /Title (By number) /Parent 5 0 R /Prev 9 0 R /Dest [1 /Fit] >>",
        "<< /S /GoTo /D [3 0 R /XYZ 10 290 null] >>",
        "<< /Names [(section.1) [4 0 R /XYZ 0 200 null]] >>",
        "<< /chapter << /D [4 0 R /FitH 250] >> >>",
        "<< /Type /Annot /Subtype /Link /Rect [0 0 50 50] /P 3 0 R /A 11 0 R >>",
    ])
}

fn r(object_number: u32) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": object_number })
}

const A: u32 = 6;
const B: u32 = 7;
const C: u32 = 8;
const D: u32 = 9;
const A1: u32 = 10;
const C1: u32 = 11;
const FIRST_PAGE: u32 = 3;
const SECOND_PAGE: u32 = 4;

const SHARED_ONE: u32 = 6;
const SHARED_TWO: u32 = 7;
const HYPERREF: u32 = 8;
const OLD_DESTS: u32 = 9;
const BY_NUMBER: u32 = 10;

/// Open `bytes` as `d`, with `scope` when given (every right otherwise).
fn open_with(bytes: Vec<u8>, scope: Option<Value>) -> Host {
    let mut host = Host::new();
    let mut request = json!({ "kind": "open.fatMem", "jobId": 1, "docId": DOC });
    if let Some(scope) = scope {
        request["scope"] = scope;
    }
    let response = host.call_filled(&request.to_string(), vec![bytes]);
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "open failed: {envelope}");
    host
}

fn open(bytes: Vec<u8>) -> Host {
    open_with(bytes, None)
}

/// Send `request` to `d`: its `result`, or its `error`.
fn send(host: &mut Host, mut request: Value) -> Result<Value, Value> {
    request["jobId"] = json!(7);
    request["docId"] = json!(DOC);
    let response = host.call_filled(&request.to_string(), Vec::new());
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope["result"].take()),
    }
}

/// `doc.outline.get()`: the `outline` of `{ outline }`, the payload
/// `outline.read` answers without its tag.
fn get(host: &mut Host) -> Value {
    send(host, json!({ "kind": "outline.read" })).expect("the outline reads")["outline"].take()
}

/// `doc.apply(change)`: the whole answer, or the refusal.
fn apply(host: &mut Host, change: Value) -> Result<Value, Value> {
    send(host, json!({ "kind": "document.apply", "effect": "write", "change": change }))
}

/// One op as `outlineVerbs` runs it: its item without `type` and
/// `skipped`, or the refusal.
fn verb(host: &mut Host, op: Value) -> Result<Value, Value> {
    let kind = op["type"].as_str().unwrap().to_owned();
    let answer = apply(host, json!({ "ops": [op] }))?;
    let mut item = answer["result"]["items"][0].clone();
    assert_eq!(item["type"], kind.as_str(), "{answer}");
    let fields = item.as_object_mut().unwrap();
    fields.remove("type");
    fields.remove("skipped");
    Ok(item)
}

fn create(host: &mut Host, data: Value, parent: Option<u32>, position: Option<Value>) -> Value {
    try_create(host, data, parent, position).unwrap_or_else(|e| panic!("create refused: {e}"))
}

fn try_create(
    host: &mut Host,
    data: Value,
    parent: Option<u32>,
    position: Option<Value>,
) -> Result<Value, Value> {
    let mut op = json!({ "type": "outline.create", "parent": parent.map(r), "data": data });
    if let Some(position) = position {
        op["position"] = position;
    }
    verb(host, op)
}

fn update(host: &mut Host, item: u32, patch: Value) -> Value {
    verb(host, json!({ "type": "outline.update", "ref": r(item), "patch": patch }))
        .unwrap_or_else(|e| panic!("update refused: {e}"))
}

fn move_items(
    host: &mut Host,
    items: &[u32],
    parent: Option<u32>,
    position: Value,
) -> Result<Value, Value> {
    let refs: Vec<Value> = items.iter().map(|item| r(*item)).collect();
    verb(
        host,
        json!({ "type": "outline.move", "refs": refs, "parent": parent.map(r),
            "position": position }),
    )
}

fn delete(host: &mut Host, item: u32) -> Value {
    verb(host, json!({ "type": "outline.delete", "ref": r(item) }))
        .unwrap_or_else(|e| panic!("delete refused: {e}"))
}

fn undo(host: &mut Host, op_id: &Value) -> Value {
    apply(host, json!({ "undoOf": op_id })).unwrap_or_else(|e| panic!("undo refused: {e}"))
        ["result"]
        .take()
}

fn titles(outline: &Value) -> Vec<String> {
    outline["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}

fn children(outline: &Value, parent: Option<u32>) -> Vec<String> {
    let parent = parent.map_or(Value::Null, r);
    outline["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["parent"] == parent)
        .map(|item| item["title"].as_str().unwrap().to_owned())
        .collect()
}

fn item_of(outline: &Value, item: u32) -> Option<Value> {
    outline["items"].as_array().unwrap().iter().find(|i| i["ref"] == r(item)).cloned()
}

/// The children a write's orders give `parent`, by object number.
fn order_of(orders: &Value, parent: Option<u32>) -> Vec<u64> {
    let parent = parent.map_or(Value::Null, r);
    let order = orders.as_array().unwrap().iter().find(|o| o["parent"] == parent).expect("order");
    order["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["objectNumber"].as_u64().unwrap())
        .collect()
}

/// Where the document's one link goes.
fn link_target_of(host: &mut Host) -> Value {
    let list = send(host, json!({ "kind": "annotations.list" })).expect("annotations");
    list["annotations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|annotation| annotation["subtype"] == "link")
        .map(|link| link["target"].clone())
        .expect("a link")
}

/// `toMatchObject`: every key of `expected` is in `actual` with a value that
/// matches, numbers by value.
fn matches(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected
            .iter()
            .all(|(key, value)| actual.get(key).is_some_and(|found| matches(found, value))),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual.iter().zip(expected).all(|(found, value)| matches(found, value))
        }
        (Value::Number(actual), Value::Number(expected)) => actual.as_f64() == expected.as_f64(),
        _ => actual == expected,
    }
}

#[track_caller]
fn assert_matches(actual: &Value, expected: Value) {
    assert!(matches(actual, &expected), "\n  actual: {actual}\nexpected: {expected}");
}

#[track_caller]
fn assert_same(actual: &Value, expected: &Value) {
    assert_eq!(canonical_numbers(actual), canonical_numbers(expected));
}

// ── The prediction rules (`core/src/outline/predict.ts`) ───────────────────

/// `actionTreeOf`: the action a write makes for a target, as a read of it
/// returns it.
fn action_tree_of(target: &Value) -> Value {
    let root = match target["kind"].as_str().unwrap() {
        "uri" => json!({ "type": "uri", "subtype": "URI", "uri": target["uri"], "isMap": false,
            "next": [] }),
        "goto" => json!({ "type": "goto", "subtype": "GoTo",
            "destination": target["destination"], "next": [] }),
        _ => json!({ "type": "named", "subtype": "Named", "name": target["name"], "next": [] }),
    };
    json!({ "root": root, "incomplete": false, "warningFlags": 0, "warnings": [] })
}

/// `targetFieldsOf`: the target, and the `/A` action it is written as.
fn target_fields(item: &mut Value, target: &Value) {
    if target.is_null() {
        item["target"] = Value::Null;
        item["action"] = Value::Null;
    } else {
        item["target"] = target.clone();
        item["action"] = action_tree_of(target);
    }
}

fn color_as_read(color: &Value) -> Value {
    color.as_str().map_or(Value::Null, |color| json!(color.to_lowercase()))
}

/// `outlineItemOfDraft`.
fn item_of_draft(reference: &Value, parent: Option<u32>, draft: &Value) -> Value {
    let mut item = json!({
        "ref": reference,
        "parent": parent.map_or(Value::Null, r),
        "title": draft["title"],
        "open": false,
        "color": color_as_read(&draft["color"]),
        "bold": draft.get("bold").cloned().unwrap_or(json!(false)),
        "italic": draft.get("italic").cloned().unwrap_or(json!(false)),
    });
    target_fields(&mut item, draft.get("target").unwrap_or(&Value::Null));
    item
}

/// `outlineItemAfter`.
fn item_after(item: &Value, patch: &Value, has_children: bool) -> Value {
    let mut next = item.clone();
    for (key, value) in patch.as_object().unwrap() {
        match key.as_str() {
            "target" => target_fields(&mut next, value),
            "color" => next["color"] = color_as_read(value),
            "open" => next["open"] = json!(value.as_bool().unwrap() && has_children),
            _ => next[key] = value.clone(),
        }
    }
    next
}

// ── runOutlineConformance ──────────────────────────────────────────────────

#[test]
fn reads_every_item_depth_first_with_its_target_its_look_and_whether_it_opens() {
    let mut host = open(outline_fixture());
    let outline = get(&mut host);
    assert_eq!(outline["incomplete"], false);
    assert_eq!(titles(&outline), ["A", "A1", "B", "C", "C1", "D"]);
    assert_matches(
        &item_of(&outline, A).unwrap(),
        json!({ "parent": null, "open": true, "color": null, "bold": false, "italic": false,
            "action": null, "target": { "kind": "goto",
                "destination": { "kind": "xyz", "page": r(FIRST_PAGE), "x": 20, "y": 20 } } }),
    );
    assert_matches(
        &item_of(&outline, A1).unwrap(),
        json!({ "parent": r(A), "open": false,
            "target": { "kind": "goto", "destination": { "kind": "fit", "page": r(SECOND_PAGE) } } }),
    );
    assert_matches(
        &item_of(&outline, B).unwrap(),
        json!({ "color": "#ff0000", "bold": true,
            "target": { "kind": "uri", "uri": "https://example.com/" },
            "action": { "incomplete": false,
                "root": { "type": "uri", "uri": "https://example.com/" } } }),
    );
    // A named destination, resolved, measured from the page's top.
    assert_matches(
        &item_of(&outline, C).unwrap(),
        json!({ "open": false, "target": { "kind": "goto",
            "destination": { "kind": "fitH", "page": r(SECOND_PAGE), "y": 50 } } }),
    );
    assert_matches(
        &item_of(&outline, C1).unwrap(),
        json!({ "parent": r(C), "target": null, "action": null }),
    );
    assert_matches(
        &item_of(&outline, D).unwrap(),
        json!({ "target": { "kind": "javascript" },
            "action": { "root": { "type": "javascript", "script": "app.alert(1)" } } }),
    );
}

#[test]
fn a_target_reads_the_same_however_the_file_stores_it() {
    let mut host = open(targets_fixture());
    let outline = get(&mut host);
    assert_eq!(outline["incomplete"], false);
    let shared = json!({ "kind": "goto",
        "destination": { "kind": "xyz", "page": r(FIRST_PAGE), "x": 10, "y": 10 } });
    assert_matches(
        &item_of(&outline, SHARED_ONE).unwrap(),
        json!({ "target": shared, "action": { "root": { "type": "goto" } } }),
    );
    assert_matches(&item_of(&outline, SHARED_TWO).unwrap(), json!({ "target": shared }));
    assert_matches(&link_target_of(&mut host), shared);
    assert_matches(
        &item_of(&outline, HYPERREF).unwrap(),
        json!({ "target": { "kind": "goto",
                "destination": { "kind": "xyz", "page": r(SECOND_PAGE), "x": 0, "y": 100 } },
            "action": { "root": { "type": "goto" } } }),
    );
    assert_matches(
        &item_of(&outline, OLD_DESTS).unwrap(),
        json!({ "target": { "kind": "goto",
                "destination": { "kind": "fitH", "page": r(SECOND_PAGE), "y": 50 } },
            "action": null }),
    );
    assert_matches(
        &item_of(&outline, BY_NUMBER).unwrap(),
        json!({ "target": { "kind": "goto",
            "destination": { "kind": "fit", "page": r(SECOND_PAGE) } } }),
    );
}

#[test]
fn a_new_target_leaves_an_action_the_item_shared_alone_and_an_undo_brings_it_back() {
    let mut host = open(targets_fixture());
    let before = get(&mut host);
    let link = link_target_of(&mut host);
    let changed = update(
        &mut host,
        SHARED_ONE,
        json!({ "target": { "kind": "uri", "uri": "https://example.com/" } }),
    );
    assert_eq!(changed["item"]["target"], json!({ "kind": "uri", "uri": "https://example.com/" }));
    let after = get(&mut host);
    assert_same(&item_of(&after, SHARED_TWO).unwrap(), &item_of(&before, SHARED_TWO).unwrap());
    assert_same(&link_target_of(&mut host), &link);
    undo(&mut host, &changed["meta"]["opId"]);
    assert_same(&get(&mut host), &before);
}

#[test]
fn a_delete_and_its_undo_keep_a_target_however_the_file_stored_it() {
    let mut host = open(targets_fixture());
    let before = get(&mut host);
    let link = link_target_of(&mut host);
    for item in [SHARED_TWO, HYPERREF, OLD_DESTS, BY_NUMBER] {
        let deleted = delete(&mut host, item);
        assert_eq!(item_of(&get(&mut host), item), None);
        undo(&mut host, &deleted["meta"]["opId"]);
        assert_same(&get(&mut host), &before);
    }
    // The action Shared two used is still there for the link.
    assert_same(&link_target_of(&mut host), &link);
}

#[test]
fn a_document_without_an_outline_reads_an_empty_one() {
    let mut host = open(pdf_of(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>",
    ]));
    assert_eq!(get(&mut host), json!({ "items": [], "incomplete": false }));
    // The first item makes the outline.
    create(&mut host, json!({ "title": "First" }), None, None);
    assert_eq!(titles(&get(&mut host)), ["First"]);
}

#[test]
fn a_create_goes_to_its_place_under_its_parent_at_the_number_it_names() {
    let mut host = open(outline_fixture());
    let reserved =
        send(&mut host, json!({ "kind": "objectNumbers.reserve", "count": 1 })).expect("a number");
    let number = reserved["range"]["first"].as_u64().unwrap() as u32;
    let first = verb(
        &mut host,
        json!({ "type": "outline.create", "parent": r(A), "position": "start",
            "objectNumber": number, "data": { "title": "A0", "target": { "kind": "goto",
                "destination": { "kind": "fit", "page": r(FIRST_PAGE) } } } }),
    )
    .expect("the create at a number");
    assert_matches(&first["item"], json!({ "ref": r(number), "parent": r(A), "title": "A0" }));
    assert_eq!(first["order"], json!({ "parent": r(A), "children": [r(number), r(A1)] }));
    let after = create(
        &mut host,
        json!({ "title": "B2", "color": "#00ff00" }),
        None,
        Some(json!({ "after": r(B) })),
    );
    assert_eq!(after["order"]["parent"], Value::Null);
    let created = after["item"]["ref"]["objectNumber"].as_u64().unwrap();
    let order: Vec<u64> = after["order"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["objectNumber"].as_u64().unwrap())
        .collect();
    assert_eq!(order, [6, 7, created, 8, 9]);
    let outline = get(&mut host);
    assert_eq!(titles(&outline), ["A", "A0", "A1", "B", "B2", "C", "C1", "D"]);
    assert_matches(
        &item_of(&outline, created as u32).unwrap(),
        json!({ "color": "#00ff00", "target": null }),
    );
}

#[test]
fn an_update_writes_the_fields_it_names_and_leaves_the_others() {
    let mut host = open(outline_fixture());
    let renamed = update(&mut host, B, json!({ "title": "Bee", "italic": true }));
    assert_matches(
        &renamed["item"],
        json!({ "title": "Bee", "italic": true, "bold": true, "color": "#ff0000",
            "target": { "kind": "uri" } }),
    );
    let cleared = update(&mut host, B, json!({ "color": null, "target": null, "bold": false }));
    assert_matches(
        &cleared["item"],
        json!({ "color": null, "target": null, "bold": false, "italic": true }),
    );
    let retargeted = update(
        &mut host,
        C1,
        json!({ "target": { "kind": "goto",
            "destination": { "kind": "xyz", "page": r(SECOND_PAGE), "x": 10, "y": 40 } } }),
    );
    assert_matches(
        &retargeted["item"]["target"],
        json!({ "kind": "goto",
            "destination": { "kind": "xyz", "page": r(SECOND_PAGE), "x": 10, "y": 40 } }),
    );
    assert_matches(&retargeted["item"]["action"], json!({ "root": { "type": "goto" } }));
}

#[test]
fn open_takes_effect_on_an_item_with_children_and_a_leaf_has_no_open_state() {
    let mut host = open(outline_fixture());
    assert_eq!(update(&mut host, C, json!({ "open": true }))["item"]["open"], true);
    assert_eq!(update(&mut host, A, json!({ "open": false }))["item"]["open"], false);
    assert_eq!(update(&mut host, D, json!({ "open": true }))["item"]["open"], false);
}

#[test]
fn a_write_answers_what_the_prediction_rules_say_it_leaves() {
    let mut host = open(outline_fixture());
    let drafts = [
        json!({ "title": "Plain" }),
        json!({ "title": "To a spot", "target": { "kind": "goto",
                "destination": { "kind": "xyz", "page": r(SECOND_PAGE), "x": 10, "y": 40,
                    "zoom": null } },
            "color": "#00AAFF", "bold": true }),
        json!({ "title": "To a site", "target": { "kind": "uri", "uri": "https://example.com/" },
            "italic": true }),
        json!({ "title": "Next", "target": { "kind": "named", "name": "NextPage" } }),
        json!({ "title": "Fit", "target": { "kind": "goto",
            "destination": { "kind": "fit", "page": r(FIRST_PAGE) } } }),
    ];
    for draft in &drafts {
        let created = create(&mut host, draft.clone(), Some(C), None);
        let item = &created["item"];
        assert_same(item, &item_of_draft(&item["ref"], Some(C), draft));
    }
    let before = item_of(&get(&mut host), B).unwrap();
    let patches = [
        json!({ "title": "Bee", "italic": true }),
        json!({ "color": "#ABCDEF" }),
        json!({ "color": null, "target": null, "bold": false }),
        json!({ "target": { "kind": "named", "name": "LastPage" } }),
        json!({ "open": true }),
    ];
    let mut current = before;
    for patch in &patches {
        let item = update(&mut host, B, patch.clone())["item"].clone();
        assert_same(&item, &item_after(&current, patch, false));
        current = item;
    }
    let parent = item_of(&get(&mut host), C).unwrap();
    let opened = update(&mut host, C, json!({ "open": true }))["item"].clone();
    assert_same(&opened, &item_after(&parent, &json!({ "open": true }), true));
}

#[test]
fn a_move_takes_items_together_with_the_items_under_them_and_names_both_parents() {
    let mut host = open(outline_fixture());
    let moved =
        move_items(&mut host, &[C, D], Some(A), json!({ "after": r(A1) })).expect("the move");
    assert_eq!(moved["meta"]["changed"], json!([r(C), r(D)]));
    assert_eq!(order_of(&moved["orders"], None), [6, 7]);
    assert_eq!(order_of(&moved["orders"], Some(A)), [10, 8, 9]);
    let outline = get(&mut host);
    assert_eq!(titles(&outline), ["A", "A1", "C", "C1", "D", "B"]);
    assert_matches(&item_of(&outline, C).unwrap(), json!({ "parent": r(A), "open": false }));
    assert_matches(&item_of(&outline, C1).unwrap(), json!({ "parent": r(C) }));
    // Back to the top, first.
    move_items(&mut host, &[C], None, json!("start")).expect("the move back");
    assert_eq!(children(&get(&mut host), None), ["C", "A", "B"]);
}

#[test]
fn a_move_or_a_create_that_can_not_be_made_is_refused() {
    let mut host = open(outline_fixture());
    let code = |refused: Result<Value, Value>| refused.expect_err("refused")["code"].clone();
    assert_eq!(code(move_items(&mut host, &[A], Some(A1), json!("end"))), "InvalidArg");
    assert_eq!(code(move_items(&mut host, &[A], Some(A), json!("end"))), "InvalidArg");
    assert_eq!(code(move_items(&mut host, &[A, B], None, json!({ "after": r(B) }))), "InvalidArg");
    assert_eq!(code(move_items(&mut host, &[A, A], None, json!("end"))), "InvalidArg");
    assert_eq!(code(move_items(&mut host, &[A, A1], None, json!("end"))), "InvalidArg");
    assert_eq!(code(move_items(&mut host, &[A], None, json!({ "after": r(C1) }))), "NotFound");
    assert_eq!(code(move_items(&mut host, &[99], None, json!("end"))), "NotFound");
    assert_eq!(code(try_create(&mut host, json!({ "title": "x" }), Some(3), None)), "NotFound");
    assert_eq!(titles(&get(&mut host)), ["A", "A1", "B", "C", "C1", "D"]);
}

#[test]
fn a_delete_takes_the_items_under_the_item_and_an_undo_brings_the_same_ones_back() {
    let mut host = open(outline_fixture());
    let before = get(&mut host);
    let deleted = delete(&mut host, C);
    assert_eq!(deleted["meta"]["changed"], json!([r(C), r(C1)]));
    assert_eq!(titles(&get(&mut host)), ["A", "A1", "B", "D"]);
    let undone = undo(&mut host, &deleted["meta"]["opId"]);
    let types: Vec<&str> =
        undone["items"].as_array().unwrap().iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["outline.restore"]);
    assert_same(&get(&mut host), &before);
}

#[test]
fn expect_refuses_a_change_whose_assumption_does_not_hold() {
    let mut host = open(outline_fixture());
    let refused = apply(
        &mut host,
        json!({ "ops": [{ "type": "outline.update", "ref": r(B), "patch": { "title": "Bee" },
            "expect": { "title": "Not B" } }] }),
    )
    .expect_err("the assumption does not hold");
    assert_eq!(refused["code"], "ChangeConflict", "{refused}");
    assert_eq!(item_of(&get(&mut host), B).unwrap()["title"], "B");
}

#[test]
fn each_write_publishes_its_outline_event_with_its_result() {
    let mut host = open(outline_fixture());
    let mut events = Vec::new();
    let mut run = |host: &mut Host, op: Value| -> Value {
        let answer = apply(host, json!({ "ops": [op] })).expect("the write");
        events.extend(answer["events"].as_array().unwrap().iter().cloned());
        answer["result"]["items"][0].clone()
    };
    let created = run(
        &mut host,
        json!({ "type": "outline.create", "parent": r(C), "data": { "title": "New" } }),
    )["item"]["ref"]
        .clone();
    run(
        &mut host,
        json!({ "type": "outline.update", "ref": created, "patch": { "title": "Newer" } }),
    );
    run(
        &mut host,
        json!({ "type": "outline.move", "refs": [created], "parent": null, "position": "end" }),
    );
    run(&mut host, json!({ "type": "outline.delete", "ref": created }));
    let outline: Vec<&Value> =
        events.iter().filter(|e| e["type"].as_str().unwrap().starts_with("outline.")).collect();
    let types: Vec<&str> = outline.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["outline.created", "outline.updated", "outline.moved", "outline.deleted"]);
    assert_matches(outline[0], json!({ "item": { "title": "New", "parent": r(C) } }));
    assert_matches(outline[1], json!({ "item": { "title": "Newer" } }));
    assert_matches(outline[2], json!({ "meta": { "changed": [created] } }));
    assert_matches(outline[3], json!({ "deleted": [created] }));
}

#[test]
fn without_doc_outline_modify_every_write_is_refused_and_reading_still_works() {
    let mut host = open_with(outline_fixture(), Some(json!(["doc.open", "doc.render"])));
    assert_eq!(titles(&get(&mut host)), ["A", "A1", "B", "C", "C1", "D"]);
    for op in [
        json!({ "type": "outline.create", "parent": null, "data": { "title": "x" } }),
        json!({ "type": "outline.update", "ref": r(A), "patch": { "title": "x" } }),
        json!({ "type": "outline.move", "refs": [r(A)], "parent": null, "position": "end" }),
        json!({ "type": "outline.delete", "ref": r(A) }),
    ] {
        let refused = verb(&mut host, op.clone()).expect_err("no doc.outline.modify");
        assert_eq!(refused["code"], "Forbidden", "{op}: {refused}");
    }
    assert_eq!(titles(&get(&mut host)), ["A", "A1", "B", "C", "C1", "D"]);
}

#[test]
fn an_item_whose_page_is_gone_keeps_its_place_and_goes_nowhere_it_can_be_followed() {
    let mut host = open(outline_fixture());
    apply(&mut host, json!({ "ops": [{ "type": "pages.delete", "pages": [r(FIRST_PAGE)] }] }))
        .expect("the page goes");
    let outline = get(&mut host);
    assert_eq!(titles(&outline), ["A", "A1", "B", "C", "C1", "D"]);
    assert_eq!(item_of(&outline, A).unwrap()["target"], json!({ "kind": "unsupported" }));
}

#[test]
fn a_broken_outline_reads_what_it_can_and_says_incomplete() {
    let mut host = open(broken_fixture());
    let outline = get(&mut host);
    assert_eq!(outline["incomplete"], true);
    assert_eq!(titles(&outline), ["One", "Two"]);
    // What can be named can still be written.
    update(&mut host, 5, json!({ "title": "Second" }));
    assert_eq!(titles(&get(&mut host)), ["One", "Second"]);
}

// ── outlineChanges.ts: undo, redo, and a change taken back ─────────────────

#[test]
fn each_op_undoes_to_the_outline_before_and_redoes_to_the_outline_after() {
    let ops = [
        json!({ "type": "outline.create", "parent": r(A), "position": { "before": r(A1) },
            "data": { "title": "New", "color": "#123456" } }),
        json!({ "type": "outline.update", "ref": r(B),
            "patch": { "title": "Bee", "color": null, "italic": true, "target": { "kind": "goto",
                "destination": { "kind": "xyz", "page": r(SECOND_PAGE), "x": 5, "y": 6 } } } }),
        json!({ "type": "outline.move", "refs": [r(D), r(B)], "parent": r(C),
            "position": "start" }),
        json!({ "type": "outline.delete", "ref": r(A) }),
    ];
    for op in ops {
        let mut host = open(outline_fixture());
        let before = get(&mut host);
        let done =
            apply(&mut host, json!({ "ops": [op.clone()] })).expect("the op")["result"].take();
        assert_eq!(done["meta"]["undoable"], true, "{op}");
        let after = get(&mut host);
        assert_ne!(canonical_numbers(&after), canonical_numbers(&before), "{op} changed nothing");

        let undone = undo(&mut host, &done["meta"]["opId"]);
        assert!(
            undone["items"].as_array().unwrap().iter().all(|i| i["type"] != "skipped"),
            "{op}: {undone}"
        );
        assert_same(&get(&mut host), &before);

        undo(&mut host, &undone["meta"]["opId"]);
        assert_same(&get(&mut host), &after);
    }
}

#[test]
fn an_undo_puts_back_only_the_fields_that_still_show_the_update() {
    let mut host = open(outline_fixture());
    let changed = update(&mut host, B, json!({ "title": "Bee", "bold": false }));
    // Someone renames it since: the title stays theirs, bold comes back.
    update(&mut host, B, json!({ "title": "Theirs" }));
    let undone = undo(&mut host, &changed["meta"]["opId"]);
    let item = &undone["items"][0];
    assert_eq!(item["type"], "outline.update", "{undone}");
    assert_eq!(item["skipped"], json!(["title"]));
    let b = item_of(&get(&mut host), B).unwrap();
    assert_eq!(b["title"], "Theirs");
    assert_eq!(b["bold"], true);
}

#[test]
fn undoing_a_create_leaves_an_item_someone_changed_since() {
    let mut host = open(outline_fixture());
    let created = create(&mut host, json!({ "title": "New" }), None, None);
    let number = created["item"]["ref"]["objectNumber"].as_u64().unwrap() as u32;
    update(&mut host, number, json!({ "title": "Renamed" }));
    let undone = undo(&mut host, &created["meta"]["opId"]);
    assert_eq!(
        undone["items"][0],
        json!({ "type": "skipped", "op": "outline.delete",
        "meta": undone["items"][0]["meta"] })
    );
    assert!(item_of(&get(&mut host), number).is_some());
}

#[test]
fn undoing_a_delete_that_emptied_an_open_parent_opens_it_again() {
    let mut host = open(outline_fixture());
    let before = get(&mut host);
    assert_eq!(item_of(&before, A).unwrap()["open"], true);
    let deleted = delete(&mut host, A1);
    assert_eq!(item_of(&get(&mut host), A).unwrap()["open"], false, "a leaf has no open state");
    let undone = undo(&mut host, &deleted["meta"]["opId"]);
    let types: Vec<&str> =
        undone["items"].as_array().unwrap().iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["outline.restore", "outline.update"]);
    assert_same(&get(&mut host), &before);
}

#[test]
fn a_refused_change_takes_back_the_outline_ops_before_it() {
    let mut host = open(outline_fixture());
    let before = get(&mut host);
    let refused = apply(
        &mut host,
        json!({ "ops": [
            { "type": "outline.create", "parent": null, "data": { "title": "New" } },
            { "type": "outline.update", "ref": r(B), "patch": { "title": "Bee" } },
            { "type": "outline.move", "refs": [r(C)], "parent": r(A), "position": "end" },
            { "type": "outline.delete", "ref": r(D) },
            { "type": "outline.update", "ref": r(99), "patch": { "title": "x" } },
        ] }),
    )
    .expect_err("no item 99");
    assert_eq!(refused["code"], "NotFound", "{refused}");
    assert_same(&get(&mut host), &before);
}

#[test]
fn a_field_no_write_can_take_is_refused_before_anything_is_written() {
    let mut host = open(outline_fixture());
    let before = get(&mut host);
    for (patch, field) in [
        (json!({ "color": "red" }), "color"),
        (json!({ "title": "x".repeat(32_768) }), "title"),
        (json!({ "title": 3 }), "title"),
        (json!({ "target": { "kind": "javascript" } }), "target"),
        (json!({ "target": { "kind": "named", "name": "Print" } }), "target"),
    ] {
        let refused = verb(
            &mut host,
            json!({ "type": "outline.update", "ref": r(B),
            "patch": patch }),
        )
        .expect_err("refused");
        assert_eq!(refused["code"], "InvalidArg", "{refused}");
        assert_eq!(refused["details"]["field"], field, "{refused}");
    }
    assert_same(&get(&mut host), &before);
}

#[test]
fn an_expect_compares_an_axis_left_out_apart_from_a_null_one() {
    // `pdfDestinationOf` leaves out an axis the caller left out, so an
    // `expect` without `zoom` doesn't hold against A's read, whose zoom is
    // null; with `zoom: null` it holds.
    let mut host = open(outline_fixture());
    let target = |zoom: Option<Value>| {
        let mut destination = json!({ "kind": "xyz", "page": r(FIRST_PAGE), "x": 20, "y": 20 });
        if let Some(zoom) = zoom {
            destination["zoom"] = zoom;
        }
        json!({ "kind": "goto", "destination": destination })
    };
    let op = |expect: Value| {
        json!({ "ops": [{ "type": "outline.update", "ref": r(A), "patch": { "title": "A!" },
            "expect": { "target": expect } }] })
    };
    let refused = apply(&mut host, op(target(None))).expect_err("an axis left out");
    assert_eq!(refused["code"], "ChangeConflict", "{refused}");
    apply(&mut host, op(target(Some(Value::Null)))).expect("a null axis");
    assert_eq!(item_of(&get(&mut host), A).unwrap()["title"], "A!");
}

#[test]
fn the_read_answers_the_outline_under_its_key() {
    let mut host = open(outline_fixture());
    let answer = send(&mut host, json!({ "kind": "outline.read" })).expect("the outline reads");
    let keys: Vec<&String> = answer.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["outline"]);
    assert_eq!(answer["outline"]["incomplete"], false);
    // In page space: A goes to (20, 20) from page 3's top-left.
    assert_matches(
        &item_of(&answer["outline"], A).unwrap()["target"],
        json!({ "destination": { "x": 20, "y": 20 } }),
    );
}

// ── The fields as upstream's writer reads them ─────────────────────────────

#[test]
fn a_flag_reads_by_truthiness_and_null_clears_it() {
    let mut host = open(outline_fixture());
    // B is bold: `null` clears the bit, as `false` does.
    assert_eq!(update(&mut host, B, json!({ "bold": null }))["item"]["bold"], false);
    assert_eq!(update(&mut host, B, json!({ "bold": "yes" }))["item"]["bold"], true);
    assert_eq!(update(&mut host, B, json!({ "italic": 1 }))["item"]["italic"], true);
    assert_eq!(update(&mut host, B, json!({ "italic": 0 }))["item"]["italic"], false);
    // `open: null` closes an open parent; a truthy value opens a closed one.
    assert_eq!(update(&mut host, A, json!({ "open": null }))["item"]["open"], false);
    assert_eq!(update(&mut host, C, json!({ "open": "yes" }))["item"]["open"], true);
    let created =
        create(&mut host, json!({ "title": "Flags", "bold": 1, "italic": "" }), None, None);
    assert_matches(&created["item"], json!({ "bold": true, "italic": false }));
}

#[test]
fn a_draft_without_a_title_is_refused_as_upstream_s_write_fails_and_writes_nothing() {
    // Upstream's checks let it through and `withUtf16String(undefined)`
    // throws a TypeError, which the worker answers `Unknown`. A title that
    // is there but not a string is `assertWritableFields`' `InvalidArg`.
    let mut host = open(outline_fixture());
    let before = get(&mut host);
    let refused =
        try_create(&mut host, json!({ "color": "#00ff00" }), None, None).expect_err("no title");
    assert_eq!(refused["code"], "Unknown", "{refused}");
    let refused =
        try_create(&mut host, json!({ "title": null }), None, None).expect_err("a null title");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(refused["details"]["field"], "title");
    // A parent the outline doesn't have is refused first, as upstream's
    // `parentNumberOf` runs before the write.
    let refused = try_create(&mut host, json!({}), Some(3), None).expect_err("no parent 3");
    assert_eq!(refused["code"], "NotFound", "{refused}");
    assert_same(&get(&mut host), &before);
}

// ── An undo of a target past a later change (the fallback) ─────────────────

#[test]
fn an_undo_past_a_later_rename_brings_the_old_target_back() {
    let mut host = open(outline_fixture());
    let before = item_of(&get(&mut host), B).unwrap();
    let retargeted = update(
        &mut host,
        B,
        json!({ "target": { "kind": "goto",
            "destination": { "kind": "fit", "page": r(SECOND_PAGE) } } }),
    );
    update(&mut host, B, json!({ "title": "Theirs" }));
    let undone = undo(&mut host, &retargeted["meta"]["opId"]);
    let item = &undone["items"][0];
    assert_eq!(item["type"], "outline.update", "{undone}");
    assert!(item.get("skipped").is_none(), "{undone}");
    let b = item_of(&get(&mut host), B).unwrap();
    assert_eq!(b["title"], "Theirs");
    assert_same(&b["target"], &before["target"]);
    assert_matches(
        &b["action"],
        json!({ "root": { "type": "uri", "uri": "https://example.com/" } }),
    );
}

#[test]
fn an_old_target_no_write_can_make_lands_in_skipped() {
    // D's target is a script, which a write can't make: past a later
    // rename, its undo puts back what it can and names the target skipped.
    let mut host = open(outline_fixture());
    let changed = update(
        &mut host,
        D,
        json!({ "target": { "kind": "uri", "uri": "https://example.com/" }, "italic": true }),
    );
    update(&mut host, D, json!({ "title": "Theirs" }));
    let undone = undo(&mut host, &changed["meta"]["opId"]);
    let item = &undone["items"][0];
    assert_eq!(item["type"], "outline.update", "{undone}");
    assert_eq!(item["skipped"], json!(["target"]));
    let d = item_of(&get(&mut host), D).unwrap();
    assert_eq!(d["italic"], false);
    assert_eq!(d["title"], "Theirs");
    assert_eq!(d["target"], json!({ "kind": "uri", "uri": "https://example.com/" }));
}
