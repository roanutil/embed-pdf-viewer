//! G11's reorder gate: the same rows end up in the same place, under the
//! names they had.
//!
//! Regenerate the vectors, deliberately, with:
//!   pnpm --filter @embedpdf/tooling-engine-conformance run annotation-move
//!
//! `annotation_base.pdf` is the page, because a reorder is about ORDER and
//! that fixture's nine annotations have identities that differ: reordering
//! three identical ones would pass for almost any implementation. Each case
//! replays the `annotations.reorder` request the generator sent (`request`,
//! 1233d1349) through `Host::call`.
//!
//! A reorder names nothing (bc0057f3a): an annotation with no `/NM` still
//! has none after it, and an inline one keeps its `baseIndex` name. That
//! needs the layer `open.fatMem` opens (`openBytesAsLayer`), which records
//! where a promoted page's inline annotations were born.

mod common;

use common::page_ref;
use common::replay::Replay;
use serde::Deserialize;
use serde_json::{Value, json};

const SCHEMA_VERSION: u32 = 1;
const DOC: &str = "reorder";

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
    before_order: Vec<Option<String>>,
    after_order: Vec<Option<String>>,
    /// The page's annotations after the reorder, as it answers them.
    order: Vec<Value>,
    /// The `annotations.reorder` request the generator sent.
    request: Value,
    meta: ExpectedMeta,
}

#[derive(Debug, Deserialize)]
struct ExpectedMeta {
    changed: Vec<Value>,
}

fn load() -> VectorFile {
    let path = test_support::corpus::vectors_dir("annotation").join("move.json");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let file: VectorFile = serde_json::from_str(&text).expect("move.json");
    assert_eq!(file.schema_version, SCHEMA_VERSION, "vector envelope moved");
    assert_eq!(file.kind, "annotation.move");
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

/// What one replayed reorder answered: its result and its event.
struct Reordered {
    result: Value,
    event: Value,
    listed: Vec<Value>,
}

fn run(file: &VectorFile, case: &Case) -> Reordered {
    let mut replay = Replay::open(&file.fixture, DOC);
    let answer = replay.replay(&case.request).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    Reordered {
        result: answer["result"].clone(),
        event: answer["event"].clone(),
        listed: listed(&mut replay, case.page_object_number),
    }
}

fn request_refs(case: &Case) -> Vec<Value> {
    case.request["refs"].as_array().cloned().unwrap_or_default()
}

#[test]
fn a_move_leaves_the_order_the_typescript_engine_left() {
    let file = load();
    assert!(file.cases.len() >= 7, "the case list shrank");

    for case in &file.cases {
        // The starting order, so a case whose fixture drifted fails here
        // rather than being blamed on the reorder.
        let mut fresh = Replay::open(&file.fixture, DOC);
        assert_eq!(
            names(&listed(&mut fresh, case.page_object_number)),
            case.before_order,
            "{}: the fixture no longer starts this way",
            case.name,
        );

        let reordered = run(&file, case);
        assert_eq!(
            reordered.result["order"].as_array().cloned().unwrap_or_default(),
            case.order,
            "{}: the order answered: {}",
            case.name,
            case.covers,
        );
        assert_eq!(refs(&reordered.listed), case.order, "{}: the order read back", case.name);
        assert_eq!(
            names(&reordered.listed),
            case.after_order,
            "{}: the names read back: {}",
            case.name,
            case.covers
        );
    }
}

#[test]
fn the_block_lands_where_it_was_asked_to_in_the_order_it_was_given() {
    // The two properties an order comparison could satisfy by accident. The
    // block is CONTIGUOUS where it lands, and its internal order is the
    // caller's rather than the document's.
    let file = load();
    for case in &file.cases {
        let order = run(&file, case).result["order"].as_array().cloned().unwrap_or_default();
        let given = request_refs(case);
        let first = order.iter().position(|r| *r == given[0]).expect("the first row");
        assert_eq!(
            order[first..first + given.len()],
            given[..],
            "{}: the block is not together in the order given",
            case.name
        );
        // Beside the neighbour it named, or at the end it named.
        let position = &case.request["position"];
        if let Some(neighbour) = position.get("before") {
            assert_eq!(order[first + given.len()], *neighbour, "{}", case.name);
        } else if let Some(neighbour) = position.get("after") {
            assert_eq!(order[first - 1], *neighbour, "{}", case.name);
        } else if position == "start" {
            assert_eq!(first, 0, "{}", case.name);
        } else {
            assert_eq!(first + given.len(), order.len(), "{}", case.name);
        }
    }

    // The case that makes the order claim non-vacuous: the same three
    // annotations, asked for in the opposite order, come out reversed.
    let block = |name: &str| {
        let case = file.cases.iter().find(|c| c.name == name).expect(name);
        let order = run(&file, case).result["order"].as_array().cloned().unwrap_or_default();
        let given = request_refs(case);
        let first = order.iter().position(|r| given.contains(r)).expect("the block");
        order[first..first + given.len()].to_vec()
    };
    let mut reversed = block("the-callers-order-not-the-documents");
    reversed.reverse();
    assert_eq!(block("a-scattered-block"), reversed);
}

#[test]
fn a_move_is_structural_and_says_so() {
    let file = load();
    for case in &file.cases {
        let reordered = run(&file, case);
        let meta = &reordered.result["meta"];
        assert_eq!(
            meta["changed"].as_array().cloned().unwrap_or_default(),
            case.meta.changed,
            "{}: the changed list",
            case.name,
        );
        assert_eq!(
            meta["changed"],
            json!(request_refs(case)),
            "{}: the rows moved, as given",
            case.name
        );
        assert_eq!(
            meta["affectedPages"],
            json!([page_ref(case.page_object_number)]),
            "{}",
            case.name
        );
        // The answer is the page's whole order, and its event says so.
        assert_eq!(reordered.event["type"], "annotations.reordered", "{}", case.name);
        assert_eq!(reordered.event["order"], reordered.result["order"], "{}", case.name);
        assert_eq!(reordered.event["page"], page_ref(case.page_object_number), "{}", case.name);
        // Nothing is renamed: the same names, in another order.
        let mut before: Vec<String> = case.order.iter().map(Value::to_string).collect();
        let mut after: Vec<String> =
            reordered.listed.iter().map(|a| a["ref"].to_string()).collect();
        before.sort();
        after.sort();
        assert_eq!(after, before, "{}: a name changed", case.name);
    }
}

#[test]
fn a_move_that_changes_nothing_is_still_structural() {
    // Moving a block to where it already is leaves the order alone and is
    // still a write: it names its page and the rows it moved, and publishes.
    let file = load();
    let case = file
        .cases
        .iter()
        .find(|c| c.name == "a-move-that-changes-nothing")
        .expect("the no-op case");
    let reordered = run(&file, case);
    assert_eq!(names(&reordered.listed), case.before_order);
    assert_eq!(
        reordered.result["meta"]["affectedPages"],
        json!([page_ref(case.page_object_number)])
    );
    assert_eq!(reordered.result["meta"]["changed"], json!(request_refs(case)));
    assert_eq!(reordered.event["type"], "annotations.reordered");
}

#[test]
fn a_move_refuses_what_it_cannot_do_and_changes_nothing() {
    let file = load();
    let case = &file.cases[0];
    let pon = case.page_object_number;
    let row = |n: u32| json!({ "kind": "objectNumber", "page": page_ref(pon), "objectNumber": n });
    let gone = row(9_999);
    let elsewhere = json!({ "kind": "objectNumber", "page": page_ref(pon + 1), "objectNumber": 5 });
    for (refs, position, code, what) in [
        (json!([]), json!("start"), "InvalidArg", "an empty batch"),
        (json!([row(5), row(5)]), json!("end"), "InvalidArg", "a row named twice"),
        (
            json!([row(5), row(6)]),
            json!({ "after": row(6) }),
            "InvalidArg",
            "a neighbour that moves",
        ),
        (json!([elsewhere]), json!("start"), "InvalidArg", "a row on another page"),
        (json!([row(5)]), json!({ "before": gone }), "NotFound", "a neighbour not on the page"),
        (json!([gone]), json!("start"), "NotFound", "a row not on the page"),
    ] {
        let mut replay = Replay::open(&file.fixture, DOC);
        let start = listed(&mut replay, pon);
        let refused = replay
            .replay(&json!({ "kind": "annotations.reorder", "jobId": "<job>", "opId": "<op>",
                "docId": DOC, "page": page_ref(pon), "refs": refs, "position": position }))
            .expect_err(what);
        assert_eq!(refused["code"], code, "{what}: {refused}");
        assert_eq!(listed(&mut replay, pon), start, "{what}: the page moved anyway");
    }

    // A position the wire doesn't have is not a position.
    let mut replay = Replay::open(&file.fixture, DOC);
    let refused = replay
        .replay(&json!({ "kind": "annotations.reorder", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "page": page_ref(pon), "refs": [row(5)], "toIndex": 3 }))
        .expect_err("the old spelling");
    assert_eq!(refused["code"], "WireFormat", "{refused}");
}

/// A reorder's undo puts each row back beside its old neighbours, and
/// undoing the undo moves them again.
#[test]
fn a_reorder_is_undone_beside_its_old_neighbours() {
    let file = load();
    let case = file.cases.iter().find(|c| c.name == "a-scattered-block").expect("a case");
    let pon = case.page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = refs(&listed(&mut replay, pon));
    let applied = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "reorder-1", "change": { "ops": [{ "type": "annotations.reorder",
                "page": page_ref(pon), "refs": case.request["refs"],
                "position": case.request["position"] }] } }))
        .expect("applied");
    assert_eq!(applied["result"]["items"][0]["type"], "annotations.reorder");
    assert_eq!(applied["result"]["meta"]["undoable"], true);
    assert_eq!(refs(&listed(&mut replay, pon)), case.order);

    let undone = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "undo-1", "change": { "undoOf": "reorder-1" } }))
        .expect("undone");
    let item = &undone["result"]["items"][0];
    assert_eq!(item["type"], "annotations.reorder", "{undone}");
    assert_eq!(item["order"], json!(before), "{undone}");
    assert_eq!(refs(&listed(&mut replay, pon)), before);

    replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "redo-1", "change": { "undoOf": "undo-1" } }))
        .expect("redone");
    assert_eq!(refs(&listed(&mut replay, pon)), case.order);
}

/// A row that moved again since is left where it is; the undo puts back
/// only the rows still where the reorder left them.
#[test]
fn an_undo_leaves_a_row_that_moved_since() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let row = |n: u32| json!({ "kind": "objectNumber", "page": page_ref(pon), "objectNumber": n });
    let mut replay = Replay::open(&file.fixture, DOC);
    let reorder = |replay: &mut Replay, op_id: &str, refs: Value, position: Value| {
        replay
            .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
                "opId": op_id, "change": { "ops": [{ "type": "annotations.reorder",
                    "page": page_ref(pon), "refs": refs, "position": position }] } }))
            .expect(op_id)
    };
    reorder(&mut replay, "reorder-1", json!([row(5), row(7)]), json!("end"));
    // The second row moves again, to the bottom.
    reorder(&mut replay, "reorder-2", json!([row(7)]), json!("start"));
    let moved_since = refs(&listed(&mut replay, pon));
    let undone = replay
        .replay(&json!({ "kind": "document.apply", "jobId": "<job>", "docId": DOC,
            "opId": "undo-1", "change": { "undoOf": "reorder-1" } }))
        .expect("undone");
    assert_eq!(undone["result"]["items"][0]["meta"]["changed"], json!([row(5)]), "{undone}");
    // The first row had nothing below it, so it goes back to the bottom;
    // the row that moved since stays where it went, now above it.
    let after = refs(&listed(&mut replay, pon));
    assert_eq!(after[0], row(5), "the row put back");
    assert_eq!(after[1], row(7), "the row that moved since");
    assert_eq!(after[2..], moved_since[1..moved_since.len() - 1], "the rest keep their order");
}

/// Widgets have an order of their own: a widget is not reordered among
/// annotations, nor an annotation beside one.
#[test]
fn a_widget_is_ordered_by_the_forms_verb() {
    let fixture = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";
    let mut replay = Replay::open(fixture, DOC);
    let forms = replay
        .replay(&json!({ "kind": "forms.list", "jobId": "<job>", "docId": DOC }))
        .expect("forms");
    let widget = forms["widgets"][0]["ref"].clone();
    let pon = widget["page"]["objectNumber"].as_u64().expect("its page") as u32;
    let annotation = listed(&mut replay, pon)[0]["ref"].clone();
    for (refs, position, what) in [
        (json!([widget]), json!("start"), "a widget among annotations"),
        (json!([annotation]), json!({ "after": widget }), "beside a widget"),
    ] {
        let refused = replay
            .replay(&json!({ "kind": "annotations.reorder", "jobId": "<job>", "opId": "<op>",
                "docId": DOC, "page": page_ref(pon), "refs": refs, "position": position }))
            .expect_err(what);
        assert_eq!(refused["code"], "InvalidArg", "{what}: {refused}");
        assert!(
            refused["message"].as_str().unwrap_or_default().contains("doc.forms.reorderWidgets")
        );
    }
}

// ── Ported from `runAnnotationMutationConformance` ─────────────────────────

fn highlight(replay: &mut Replay, pon: u32, contents: &str) -> Value {
    let created = replay
        .replay(&json!({ "kind": "annotations.create", "jobId": "<job>", "docId": DOC,
            "page": page_ref(pon), "draft": { "subtype": "highlight", "contents": contents,
            "quadPoints": [{ "upperLeft": { "x": 72, "y": 72 }, "upperRight": { "x": 200, "y": 72 },
                "lowerLeft": { "x": 72, "y": 92 }, "lowerRight": { "x": 200, "y": 92 } }] } }))
        .expect("created");
    created["result"]["annotation"]["ref"].clone()
}

fn reorder(replay: &mut Replay, pon: u32, refs: Value, position: Value) -> Value {
    replay
        .replay(&json!({ "kind": "annotations.reorder", "jobId": "<job>", "opId": "<op>",
            "docId": DOC, "page": page_ref(pon), "refs": refs, "position": position }))
        .expect("reordered")["result"]
        .clone()
}

/// "reorder of an annotation born inline keeps its baseIndex name".
#[test]
fn an_inline_annotation_reordered_keeps_its_birth_name() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let before = listed(&mut replay, pon);
    let inline = before.iter().find(|a| a["ref"]["kind"] == "baseIndex").expect("an inline one");
    let at_bottom = before[0]["ref"] == inline["ref"];
    let result = reorder(
        &mut replay,
        pon,
        json!([inline["ref"]]),
        json!(if at_bottom { "end" } else { "start" }),
    );
    let order = result["order"].as_array().expect("order");
    let at = if at_bottom { order.len() - 1 } else { 0 };
    assert_eq!(order[at], inline["ref"]);
    assert_eq!(result["meta"]["changed"], json!([inline["ref"]]));
    let read = &listed(&mut replay, pon)[at];
    assert_eq!(read["ref"], inline["ref"]);
    assert_eq!(read["nm"], inline["nm"]);
}

/// "reorder puts annotations right after or before their neighbour".
#[test]
fn a_reorder_puts_rows_right_after_or_before_their_neighbour() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let [a, b, c] =
        ["order-a", "order-b", "order-c"].map(|label| highlight(&mut replay, pon, label));
    let before = refs(&listed(&mut replay, pon));
    let others: Vec<Value> = before.iter().filter(|r| ![&a, &b, &c].contains(r)).cloned().collect();

    let result = reorder(&mut replay, pon, json!([a]), json!({ "after": c }));
    assert_eq!(result["meta"]["affectedPages"], json!([page_ref(pon)]));
    assert!(result["meta"].get("cacheDelta").is_some(), "{result}");
    assert_eq!(result["meta"]["changed"], json!([a]));
    let mut after_c = others.clone();
    after_c.extend([b.clone(), c.clone(), a.clone()]);
    assert_eq!(result["order"], json!(after_c));
    assert_eq!(refs(&listed(&mut replay, pon)), after_c);

    let back = reorder(&mut replay, pon, json!([a]), json!({ "before": b }));
    assert_eq!(back["order"], json!(before));
    assert_eq!(refs(&listed(&mut replay, pon)), before);
}

/// "reorder keeps the order given and goes to the bottom or the top".
#[test]
fn a_reorder_keeps_the_order_given_at_the_bottom_or_the_top() {
    let file = load();
    let pon = file.cases[0].page_object_number;
    let mut replay = Replay::open(&file.fixture, DOC);
    let ids = ["multi-1", "multi-2", "multi-3"].map(|label| highlight(&mut replay, pon, label));
    let given = json!([ids[2], ids[0], ids[1]]);
    let bottom = reorder(&mut replay, pon, given.clone(), json!("start"));
    assert_eq!(bottom["meta"]["changed"], given);
    assert_eq!(json!(bottom["order"].as_array().unwrap()[..3]), given);
    let top = reorder(&mut replay, pon, json!([ids[2]]), json!("end"));
    assert_eq!(top["order"].as_array().unwrap().last(), Some(&ids[2]));
    assert_eq!(top["order"].as_array().unwrap().len(), bottom["order"].as_array().unwrap().len());
}
