//! The page structure ops of `document.apply` against the conformance
//! suites upstream runs them through: `runPageReorderConformance`,
//! `runPageRotateConformance`, `runPageDeleteConformance`,
//! `runPageInsertBlankConformance` and `runPageInsertConformance`
//! (`core/src/conformance/`, through `main/test/page-*.test.ts`), over the
//! same multi-page `sample.pdf`. Then what a change adds: all of its ops or
//! none, and the undo of each op and of its undo.
//!
//! The suites' abort cases have no counterpart: a `Host::call` runs to the
//! end on this thread, and a cancelled job is refused before it starts.

mod common;

use epdf_engine::host::Host;
use serde_json::{Value, json};

const SAMPLE: &str = "examples/engine-runtime-demo/public/sample.pdf";
const DOC: &str = "d";

fn open() -> Host {
    open_bytes(test_support::fixture(SAMPLE))
}

fn open_bytes(bytes: Vec<u8>) -> Host {
    let mut host = Host::new();
    let response = host
        .call(&json!({ "kind": "open.fatMem", "jobId": 1, "docId": DOC }).to_string(), vec![bytes]);
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    host
}

/// Send `request` to the document, answering the result or the error.
fn send(host: &mut Host, mut request: Value, payloads: Vec<Vec<u8>>) -> Result<Value, Value> {
    request["jobId"] = json!(7);
    request["docId"] = json!(DOC);
    let response = host.call(&request.to_string(), payloads);
    let mut envelope: Value = serde_json::from_str(&response.json).expect("JSON");
    match envelope.get_mut("error") {
        Some(error) => Err(error.take()),
        None => Ok(envelope["result"].take()),
    }
}

/// One change of `ops` under `op_id`: its answer or its refusal.
fn apply(host: &mut Host, op_id: &str, ops: Value) -> Result<Value, Value> {
    apply_with(host, op_id, ops, Vec::new())
}

fn apply_with(
    host: &mut Host,
    op_id: &str,
    ops: Value,
    payloads: Vec<Vec<u8>>,
) -> Result<Value, Value> {
    send(
        host,
        json!({ "kind": "document.apply", "opId": op_id, "change": { "ops": ops } }),
        payloads,
    )
}

/// The undo of `op_id`.
fn undo(host: &mut Host, op_id: &str, undo_id: &str) -> Value {
    send(
        host,
        json!({ "kind": "document.apply", "opId": undo_id, "change": { "undoOf": op_id } }),
        Vec::new(),
    )
    .unwrap_or_else(|e| panic!("undo of {op_id}: {e}"))
}

/// The first item of a change's answer.
fn item(answer: &Value) -> Value {
    answer["result"]["items"][0].clone()
}

fn list(host: &mut Host) -> Value {
    send(host, json!({ "kind": "pages.list" }), Vec::new()).expect("pages.list")
}

/// The pages' refs, in display order.
fn refs(host: &mut Host) -> Vec<Value> {
    list(host)["pages"].as_array().unwrap().iter().map(|page| page["ref"].clone()).collect()
}

fn numbers(refs: &[Value]) -> Vec<u64> {
    refs.iter().map(|page| page["objectNumber"].as_u64().unwrap()).collect()
}

fn code(error: &Value) -> &str {
    error["code"].as_str().unwrap_or_default()
}

fn page(number: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

/// An object number no page has.
fn bogus(pages: &[Value]) -> u64 {
    numbers(pages).into_iter().max().unwrap_or(0) + 9999
}

/// A document whose annotations are all born inline
/// (`annotation_appearances.pdf`), with a blank page after its own, so a
/// page op has another page to work on. `sample.pdf` has no annotation, so
/// upstream's suites skip these cases there.
fn open_inline() -> (Host, Value) {
    let mut host = open_bytes(test_support::fixture(
        "tooling/engine-conformance/fixtures/annotation_appearances.pdf",
    ));
    apply(
        &mut host,
        "blank",
        json!([{ "type": "pages.insertBlank",
        "spec": { "size": { "width": 612, "height": 792 } } }]),
    )
    .expect("a second page");
    let inline = inline_annotation(&mut host).expect("an annotation born inline");
    (host, inline)
}

/// An annotation born inline, named by its `baseIndex`, or `None` when the
/// document has none.
fn inline_annotation(host: &mut Host) -> Option<Value> {
    let listing = send(host, json!({ "kind": "annotations.list" }), Vec::new()).expect("listing");
    listing["annotations"]
        .as_array()?
        .iter()
        .find(|annotation| annotation["ref"]["kind"] == "baseIndex")
        .cloned()
}

/// An annotation ref still names its annotation: an update through it
/// answers the same ref.
fn assert_still_names(host: &mut Host, annotation: &Value, op_id: &str) {
    let updated = send(
        host,
        json!({ "kind": "annotations.update", "opId": op_id, "ref": annotation["ref"],
            "patch": { "contents": "still alive" } }),
        Vec::new(),
    )
    .unwrap_or_else(|e| panic!("the ref stopped naming its annotation: {e}"));
    assert_eq!(updated["result"]["annotation"]["contents"], "still alive", "{updated}");
    assert_eq!(updated["result"]["annotation"]["ref"], annotation["ref"]);
}

// ── runPageReorderConformance ────────────────────────────────────────────

#[test]
fn a_reorder_moves_pages_to_a_position_and_answers_the_whole_order() {
    let mut host = open();
    let before = refs(&mut host);
    assert!(before.len() >= 3, "the fixture needs three pages");
    let target = before[2].clone();
    let answer = apply(
        &mut host,
        "r1",
        json!([{ "type": "pages.reorder", "pages": [target], "position": "start" }]),
    )
    .expect("reorder");
    let moved = item(&answer);
    assert_eq!(moved["pages"], json!([target]));
    let after: Vec<Value> =
        moved["layout"]["pages"].as_array().unwrap().iter().map(|p| p["ref"].clone()).collect();
    assert_eq!(after.len(), before.len());
    assert_eq!(after[0], target);
    for (index, entry) in moved["layout"]["pages"].as_array().unwrap().iter().enumerate() {
        assert_eq!(entry["index"], json!(index));
    }
    let mut sorted_before = numbers(&before);
    let mut sorted_after = numbers(&after);
    sorted_before.sort_unstable();
    sorted_after.sort_unstable();
    assert_eq!(sorted_before, sorted_after, "a page was lost or made up");
    assert_eq!(refs(&mut host), after, "the next listing disagrees with the answer");
}

#[test]
fn a_reorder_puts_the_pages_beside_their_neighbour_in_the_order_given() {
    let mut host = open();
    let before = refs(&mut host);
    let (first, second, third) = (before[0].clone(), before[1].clone(), before[2].clone());
    let rest = &before[3..];
    let order_is = |host: &mut Host, expected: Vec<Value>| assert_eq!(refs(host), expected);

    apply(
        &mut host,
        "r1",
        json!([{ "type": "pages.reorder", "pages": [third, first], "position": { "after": second } }]),
    )
    .expect("after");
    order_is(
        &mut host,
        [vec![second.clone(), third.clone(), first.clone()], rest.to_vec()].concat(),
    );

    apply(
        &mut host,
        "r2",
        json!([{ "type": "pages.reorder", "pages": [first], "position": { "before": second } }]),
    )
    .expect("before");
    order_is(&mut host, before.clone());

    let last = before.last().unwrap().clone();
    apply(
        &mut host,
        "r3",
        json!([{ "type": "pages.reorder", "pages": [first], "position": "end" }]),
    )
    .expect("end");
    assert_eq!(refs(&mut host).last(), Some(&first));
    apply(
        &mut host,
        "r4",
        json!([{ "type": "pages.reorder", "pages": [first], "position": { "before": second } }]),
    )
    .expect("back");
    assert_eq!(refs(&mut host).last(), Some(&last));
}

#[test]
fn a_reorder_refuses_a_page_twice_a_moving_neighbour_and_an_unknown_page() {
    let mut host = open();
    let pages = refs(&mut host);
    let twice = apply(
        &mut host,
        "r1",
        json!([{ "type": "pages.reorder", "pages": [pages[0], pages[0]], "position": "start" }]),
    )
    .expect_err("twice");
    assert_eq!(code(&twice), "InvalidArg", "{twice}");
    let moving = apply(
        &mut host,
        "r2",
        json!([{ "type": "pages.reorder", "pages": [pages[0], pages[1]],
            "position": { "after": pages[1] } }]),
    )
    .expect_err("a moving neighbour");
    assert_eq!(code(&moving), "InvalidArg", "{moving}");
    let unknown = page(bogus(&pages));
    let missing = apply(
        &mut host,
        "r3",
        json!([{ "type": "pages.reorder", "pages": [unknown], "position": "start" }]),
    )
    .expect_err("an unknown page");
    assert_eq!(code(&missing), "NotFound", "{missing}");
    let neighbour = apply(
        &mut host,
        "r4",
        json!([{ "type": "pages.reorder", "pages": [pages[0]], "position": { "after": unknown } }]),
    )
    .expect_err("an unknown neighbour");
    assert_eq!(code(&neighbour), "NotFound", "{neighbour}");
    assert_eq!(refs(&mut host), pages);
}

#[test]
fn an_inline_annotation_keeps_its_name_across_a_page_reorder() {
    let (mut host, inline) = open_inline();
    let other = refs(&mut host).into_iter().find(|p| *p != inline["page"]).expect("another page");
    apply(
        &mut host,
        "r1",
        json!([{ "type": "pages.reorder", "pages": [other], "position": "start" }]),
    )
    .expect("reorder");
    assert_still_names(&mut host, &inline, "u1");
}

// ── runPageRotateConformance ─────────────────────────────────────────────

#[test]
fn a_rotation_is_absolute_idempotent_and_applies_to_every_listed_page() {
    let mut host = open();
    let pages = refs(&mut host);
    let before = list(&mut host);
    let rotated = item(
        &apply(
            &mut host,
            "t1",
            json!([{ "type": "pages.rotate", "pages": [pages[0]], "rotation": 90 }]),
        )
        .expect("rotate"),
    );
    let entry = &rotated["layout"]["pages"][0];
    assert_eq!(entry["rotation"], 90);
    // Page space ignores `/Rotate`: the size stays the unrotated one.
    assert_eq!(entry["size"], before["pages"][0]["size"]);
    assert_eq!(rotated["rotation"], 90);
    assert_eq!(rotated["pages"], json!([pages[0]]));

    let first = item(
        &apply(
            &mut host,
            "t2",
            json!([{ "type": "pages.rotate", "pages": [pages[1]], "rotation": 180 }]),
        )
        .expect("once"),
    );
    let second = item(
        &apply(
            &mut host,
            "t3",
            json!([{ "type": "pages.rotate", "pages": [pages[1]], "rotation": 180 }]),
        )
        .expect("twice"),
    );
    assert_eq!(first["layout"], second["layout"], "a second absolute rotation changed something");

    let all = item(
        &apply(
            &mut host,
            "t4",
            json!([{ "type": "pages.rotate", "pages": pages, "rotation": 270 }]),
        )
        .expect("every page"),
    );
    assert!(all["layout"]["pages"].as_array().unwrap().iter().all(|p| p["rotation"] == 270));
}

#[test]
fn a_rotation_refuses_a_bad_value_a_page_twice_and_an_unknown_page() {
    let mut host = open();
    let pages = refs(&mut host);
    let bad = apply(
        &mut host,
        "t1",
        json!([{ "type": "pages.rotate", "pages": [pages[0]], "rotation": 45 }]),
    )
    .expect_err("45");
    assert_eq!(code(&bad), "InvalidArg", "{bad}");
    assert_eq!(bad["message"], "pages.rotate rotation must be 0, 90, 180 or 270 (got 45)");
    let twice = apply(
        &mut host,
        "t2",
        json!([{ "type": "pages.rotate", "pages": [pages[0], pages[0]], "rotation": 90 }]),
    )
    .expect_err("twice");
    assert_eq!(code(&twice), "InvalidArg", "{twice}");
    let unknown = apply(
        &mut host,
        "t3",
        json!([{ "type": "pages.rotate", "pages": [page(bogus(&pages))], "rotation": 90 }]),
    )
    .expect_err("unknown");
    assert_eq!(code(&unknown), "NotFound", "{unknown}");
}

#[test]
fn an_inline_annotation_keeps_its_name_when_its_page_turns() {
    let (mut host, inline) = open_inline();
    apply(
        &mut host,
        "t1",
        json!([{ "type": "pages.rotate", "pages": [inline["page"]], "rotation": 90 }]),
    )
    .expect("rotate");
    assert_still_names(&mut host, &inline, "u1");
}

// ── runPageDeleteConformance ─────────────────────────────────────────────

#[test]
fn a_delete_removes_the_page_and_a_later_read_of_it_is_not_found() {
    let mut host = open();
    let before = refs(&mut host);
    let victim = before[1].clone();
    let deleted = item(
        &apply(&mut host, "d1", json!([{ "type": "pages.delete", "pages": [victim] }]))
            .expect("delete"),
    );
    assert_eq!(deleted["pages"], json!([victim]));
    assert_eq!(deleted["layout"]["pageCount"], json!(before.len() - 1));
    let expected: Vec<Value> = before.iter().filter(|p| **p != victim).cloned().collect();
    let after: Vec<Value> =
        deleted["layout"]["pages"].as_array().unwrap().iter().map(|p| p["ref"].clone()).collect();
    assert_eq!(after, expected);
    assert_eq!(refs(&mut host), expected);
    let gone = send(&mut host, json!({ "kind": "pages.text", "page": victim }), Vec::new())
        .expect_err("a deleted page");
    assert_eq!(code(&gone), "NotFound", "{gone}");
}

#[test]
fn a_delete_refuses_every_page_a_page_twice_and_an_unknown_page() {
    let mut host = open();
    let pages = refs(&mut host);
    let every = apply(&mut host, "d1", json!([{ "type": "pages.delete", "pages": pages }]))
        .expect_err("every page");
    assert_eq!(code(&every), "InvalidArg", "{every}");
    let twice =
        apply(&mut host, "d2", json!([{ "type": "pages.delete", "pages": [pages[0], pages[0]] }]))
            .expect_err("twice");
    assert_eq!(code(&twice), "InvalidArg", "{twice}");
    let unknown =
        apply(&mut host, "d3", json!([{ "type": "pages.delete", "pages": [page(bogus(&pages))] }]))
            .expect_err("unknown");
    assert_eq!(code(&unknown), "NotFound", "{unknown}");
    assert_eq!(refs(&mut host), pages);
}

#[test]
fn an_inline_annotation_keeps_its_name_when_another_page_goes() {
    let (mut host, inline) = open_inline();
    let victim = refs(&mut host).into_iter().find(|p| *p != inline["page"]).expect("another page");
    apply(&mut host, "d1", json!([{ "type": "pages.delete", "pages": [victim] }])).expect("delete");
    assert_still_names(&mut host, &inline, "u1");
}

// ── runPageInsertBlankConformance ────────────────────────────────────────

const SIZE: (f64, f64) = (396.0, 612.0);

fn size() -> Value {
    json!({ "width": SIZE.0, "height": SIZE.1 })
}

#[test]
fn a_blank_page_is_appended_with_a_fresh_number() {
    let mut host = open();
    let before = refs(&mut host);
    let inserted = item(
        &apply(
            &mut host,
            "b1",
            json!([{ "type": "pages.insertBlank", "spec": { "size": size() } }]),
        )
        .expect("append"),
    );
    let fresh = inserted["insertedPages"].as_array().expect("insertedPages");
    assert_eq!(fresh.len(), 1);
    assert!(!before.contains(&fresh[0]), "the number was not fresh");
    assert_eq!(inserted["layout"]["pageCount"], json!(before.len() + 1));
    let appended = inserted["layout"]["pages"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(appended["ref"], fresh[0]);
    assert_eq!(appended["size"], size());
    assert_eq!(appended["rotation"], 0);
}

#[test]
fn a_position_and_a_count_place_the_blank_block_after_its_neighbour() {
    let mut host = open();
    let before = refs(&mut host);
    let inserted = item(
        &apply(
            &mut host,
            "b1",
            json!([{ "type": "pages.insertBlank", "spec": { "size": size(), "count": 2 },
                "position": { "after": before[0] } }]),
        )
        .expect("insert"),
    );
    let fresh = inserted["insertedPages"].as_array().unwrap().clone();
    assert_eq!(fresh.len(), 2);
    assert_ne!(fresh[0], fresh[1]);
    let after = refs(&mut host);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1..3], fresh[..]);
    assert_eq!(after[3..], before[1..]);
    assert_eq!(inserted["layout"]["pages"][1]["size"], size());
    assert_eq!(inserted["layout"]["pages"][2]["size"], size());
}

#[test]
fn blank_pages_persist_through_a_save_and_a_reopen() {
    let mut host = open();
    let count = refs(&mut host).len();
    apply(&mut host, "b1", json!([{ "type": "pages.insertBlank", "spec": { "size": size() } }]))
        .expect("insert");
    let saved = saved_bytes(&mut host);
    let mut reopened = open_bytes(saved);
    let listed = list(&mut reopened);
    assert_eq!(listed["pageCount"], json!(count + 1));
    let tail = &listed["pages"][count]["size"];
    assert_eq!(
        (tail["width"].as_f64().unwrap().round(), tail["height"].as_f64().unwrap().round()),
        SIZE
    );
}

#[test]
fn a_blank_insert_refuses_a_bad_size_or_count_and_an_unknown_neighbour() {
    let mut host = open();
    let before = refs(&mut host);
    for (at, spec) in [
        json!({ "size": { "width": 0, "height": 612 } }),
        json!({ "size": { "width": 396, "height": -10 } }),
        json!({ "size": size(), "count": 0 }),
        json!({ "size": size(), "count": 101 }),
    ]
    .into_iter()
    .enumerate()
    {
        let refused = apply(
            &mut host,
            &format!("b{at}"),
            json!([{ "type": "pages.insertBlank", "spec": spec }]),
        )
        .expect_err("bad spec");
        assert_eq!(code(&refused), "InvalidArg", "{spec}: {refused}");
    }
    let unknown = apply(
        &mut host,
        "b9",
        json!([{ "type": "pages.insertBlank", "spec": { "size": size() },
            "position": { "after": page(bogus(&before)) } }]),
    )
    .expect_err("unknown neighbour");
    assert_eq!(code(&unknown), "NotFound", "{unknown}");
    assert_eq!(refs(&mut host), before);
}

#[test]
fn a_blank_page_takes_the_object_number_the_op_names() {
    let mut host = Host::new();
    let opened = send(
        &mut host,
        json!({ "kind": "open.fatMem", "reserveObjectNumbers": 4 }),
        vec![test_support::fixture(SAMPLE)],
    )
    .expect("open");
    let first = opened["objectNumbers"]["first"].as_u64().expect("a reserved range");
    let inserted = item(
        &apply(
            &mut host,
            "b1",
            json!([{ "type": "pages.insertBlank", "spec": { "size": size(), "count": 2 },
                "objectNumbers": [first + 1, first] }]),
        )
        .expect("numbered"),
    );
    assert_eq!(inserted["insertedPages"], json!([page(first + 1), page(first)]));
    let mismatched = apply(
        &mut host,
        "b2",
        json!([{ "type": "pages.insertBlank", "spec": { "size": size(), "count": 2 },
            "objectNumbers": [first + 2] }]),
    )
    .expect_err("one number for two pages");
    assert_eq!(code(&mismatched), "InvalidArg", "{mismatched}");
}

// ── runPageInsertConformance ─────────────────────────────────────────────

/// The first `count` pages of the document, as a PDF of their own.
fn extracted(host: &mut Host, pages: &[Value]) -> Vec<u8> {
    let response = host.call(
        &json!({ "kind": "pages.extract", "jobId": 7, "docId": DOC, "pages": pages }).to_string(),
        Vec::new(),
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    response.payloads.into_iter().next().expect("the extracted bytes")
}

fn saved_bytes(host: &mut Host) -> Vec<u8> {
    let response = host.call(
        &json!({ "kind": "document.saveBuffer", "jobId": 7, "docId": DOC, "mode": "rewrite" })
            .to_string(),
        Vec::new(),
    );
    assert!(!response.json.contains("\"error\""), "{}", response.json);
    response.payloads.into_iter().next().expect("the saved bytes")
}

#[test]
fn an_insert_appends_every_source_page_with_fresh_numbers() {
    let mut host = open();
    let before = list(&mut host);
    let before_refs = refs(&mut host);
    let single = extracted(&mut host, &before_refs[..1]);
    let inserted = item(
        &apply_with(
            &mut host,
            "i1",
            json!([{ "type": "pages.insert", "source": { "payload": 0 } }]),
            vec![single],
        )
        .expect("insert"),
    );
    let fresh = inserted["insertedPages"].as_array().unwrap().clone();
    assert_eq!(fresh.len(), 1);
    assert!(!before_refs.contains(&fresh[0]));
    let count = before_refs.len();
    assert_eq!(inserted["layout"]["pageCount"], json!(count + 1));
    assert_eq!(inserted["layout"]["pages"][count]["ref"], fresh[0]);
    assert_eq!(inserted["layout"]["pages"][count]["size"], before["pages"][0]["size"]);
}

#[test]
fn an_insert_places_the_block_after_its_neighbour_in_source_order() {
    let mut host = open();
    let before = refs(&mut host);
    let two = extracted(&mut host, &before[..2]);
    let inserted = item(
        &apply_with(
            &mut host,
            "i1",
            json!([{ "type": "pages.insert", "source": { "payload": 0 },
                "position": { "after": before[0] } }]),
            vec![two],
        )
        .expect("insert"),
    );
    let fresh = inserted["insertedPages"].as_array().unwrap().clone();
    assert_eq!(fresh.len(), 2);
    let after = refs(&mut host);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1..3], fresh[..]);
    assert_eq!(after[3..], before[1..]);
}

#[test]
fn inserted_pages_persist_through_a_save_and_a_reopen() {
    // The source is retained on the session until it closes
    // (`retainUntilClose`): an imported stream still reads through it, so a
    // save after the insert must still find it.
    let mut host = open();
    let before = refs(&mut host);
    let single = extracted(&mut host, &before[..1]);
    apply_with(
        &mut host,
        "i1",
        json!([{ "type": "pages.insert", "source": { "payload": 0 } }]),
        vec![single],
    )
    .expect("insert");
    let mut reopened = open_bytes(saved_bytes(&mut host));
    assert_eq!(list(&mut reopened)["pageCount"], json!(before.len() + 1));
}

#[test]
fn an_insert_refuses_empty_bytes_garbage_and_an_unknown_neighbour() {
    let mut host = open();
    let before = refs(&mut host);
    let op = json!([{ "type": "pages.insert", "source": { "payload": 0 } }]);
    let empty = apply_with(&mut host, "i1", op.clone(), vec![Vec::new()]).expect_err("empty");
    assert_eq!(code(&empty), "InvalidArg", "{empty}");
    let garbage =
        apply_with(&mut host, "i2", op, vec![b"not a pdf at all".to_vec()]).expect_err("garbage");
    assert_eq!(code(&garbage), "MalformedPdf", "{garbage}");
    let single = extracted(&mut host, &before[..1]);
    let unknown = apply_with(
        &mut host,
        "i3",
        json!([{ "type": "pages.insert", "source": { "payload": 0 },
            "position": { "before": page(bogus(&before)) } }]),
        vec![single],
    )
    .expect_err("unknown neighbour");
    assert_eq!(code(&unknown), "NotFound", "{unknown}");
    assert_eq!(refs(&mut host), before);
}

#[test]
fn an_insert_takes_its_source_as_a_list_of_bytes_too() {
    // A recorded request holds upstream's typed array as a list of numbers
    // (`recording-engine.ts`); the op reads it as the same bytes.
    let mut host = open();
    let before = refs(&mut host);
    let single = extracted(&mut host, &before[..1]);
    let inserted = item(
        &apply(&mut host, "i1", json!([{ "type": "pages.insert", "source": single }]))
            .expect("insert"),
    );
    assert_eq!(inserted["insertedPages"].as_array().map(Vec::len), Some(1));
}

// ── A change: all of its ops or none ─────────────────────────────────────

#[test]
fn a_refused_op_takes_back_every_page_op_before_it() {
    let mut host = open();
    let before = list(&mut host);
    let pages = refs(&mut host);
    let single = extracted(&mut host, &pages[..1]);
    let refused = apply_with(
        &mut host,
        "all",
        json!([
            { "type": "pages.rotate", "pages": [pages[0]], "rotation": 90 },
            { "type": "pages.reorder", "pages": [pages[2]], "position": "start" },
            { "type": "pages.delete", "pages": [pages[1]] },
            { "type": "pages.insertBlank", "spec": { "size": size() }, "position": "start" },
            { "type": "pages.insert", "source": { "payload": 0 } },
            { "type": "pages.rotate", "pages": [page(bogus(&pages))], "rotation": 90 },
        ]),
        vec![single],
    )
    .expect_err("the last op names no page");
    assert_eq!(code(&refused), "NotFound", "{refused}");
    assert_eq!(list(&mut host), before, "a refused change left part of itself");
}

#[test]
fn a_later_op_resolves_pages_against_the_document_the_earlier_left() {
    // An op's places are resolved when it runs: the page the insert made is
    // a neighbour the reorder after it can name.
    let mut host = open();
    let pages = refs(&mut host);
    let answer = apply(
        &mut host,
        "seq",
        json!([
            { "type": "pages.delete", "pages": [pages[0]] },
            { "type": "pages.reorder", "pages": [pages[2]], "position": "start" },
        ]),
    )
    .expect("both");
    let items = answer["result"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(refs(&mut host)[0], pages[2]);
    assert_eq!(answer["events"].as_array().map(Vec::len), Some(2));
}

// ── Undo ─────────────────────────────────────────────────────────────────

#[test]
fn an_undone_rotation_turns_the_pages_back_and_its_redo_turns_them_again() {
    let mut host = open();
    let before = list(&mut host);
    let pages = refs(&mut host);
    apply(
        &mut host,
        "t1",
        json!([{ "type": "pages.rotate", "pages": [pages[0], pages[1]], "rotation": 90 }]),
    )
    .expect("rotate");
    let undone = undo(&mut host, "t1", "u1");
    assert_eq!(item(&undone)["type"], "pages.rotate");
    assert_eq!(list(&mut host), before);
    undo(&mut host, "u1", "u2");
    let layout = list(&mut host);
    assert_eq!(layout["pages"][0]["rotation"], 90);
    assert_eq!(layout["pages"][1]["rotation"], 90);
}

#[test]
fn an_undone_rotation_leaves_a_page_someone_turned_since() {
    let mut host = open();
    let pages = refs(&mut host);
    apply(
        &mut host,
        "t1",
        json!([{ "type": "pages.rotate", "pages": [pages[0]], "rotation": 90 }]),
    )
    .expect("rotate");
    apply(
        &mut host,
        "t2",
        json!([{ "type": "pages.rotate", "pages": [pages[0]], "rotation": 180 }]),
    )
    .expect("turned since");
    let undone = undo(&mut host, "t1", "u1");
    assert_eq!(item(&undone)["type"], "skipped", "{undone}");
    assert_eq!(list(&mut host)["pages"][0]["rotation"], 180);
}

#[test]
fn an_undone_reorder_puts_the_pages_back() {
    let mut host = open();
    let before = refs(&mut host);
    apply(
        &mut host,
        "r1",
        json!([{ "type": "pages.reorder", "pages": [before[2], before[0]], "position": "end" }]),
    )
    .expect("reorder");
    let undone = undo(&mut host, "r1", "u1");
    assert_eq!(item(&undone)["type"], "pages.reorder");
    assert_eq!(refs(&mut host), before);
    undo(&mut host, "u1", "u2");
    let redone = refs(&mut host);
    assert_eq!(redone[redone.len() - 2..], [before[2].clone(), before[0].clone()]);
}

#[test]
fn an_undone_delete_brings_the_same_pages_back_where_they_were() {
    let mut host = open();
    let before = list(&mut host);
    let pages = refs(&mut host);
    apply(&mut host, "d1", json!([{ "type": "pages.delete", "pages": [pages[2], pages[0]] }]))
        .expect("delete");
    let undone = undo(&mut host, "d1", "u1");
    let restored = item(&undone);
    assert_eq!(restored["type"], "pages.restore");
    assert_eq!(restored["pages"], json!([pages[0], pages[2]]), "in display order");
    assert_eq!(list(&mut host), before);
    assert_eq!(undone["events"][0]["type"], "pages.restored", "{undone}");
    // Its undo deletes them again.
    undo(&mut host, "u1", "u2");
    assert_eq!(refs(&mut host).len(), pages.len() - 2);
}

#[test]
fn an_undone_insert_deletes_the_new_pages_unless_someone_changed_them() {
    let mut host = open();
    let before = list(&mut host);
    apply(
        &mut host,
        "b1",
        json!([{ "type": "pages.insertBlank", "spec": { "size": size(), "count": 2 } }]),
    )
    .expect("insert");
    let undone = undo(&mut host, "b1", "u1");
    assert_eq!(item(&undone)["type"], "pages.delete");
    assert_eq!(list(&mut host), before);
    // Redone, the pages come back under the same numbers.
    let redone = undo(&mut host, "u1", "u2");
    assert_eq!(item(&redone)["type"], "pages.restore");

    let mut host = open();
    let inserted = item(
        &apply(
            &mut host,
            "b1",
            json!([{ "type": "pages.insertBlank", "spec": { "size": size() } }]),
        )
        .expect("insert"),
    );
    let fresh = inserted["insertedPages"][0].clone();
    apply(&mut host, "t1", json!([{ "type": "pages.rotate", "pages": [fresh], "rotation": 90 }]))
        .expect("turned since");
    let undone = undo(&mut host, "b1", "u1");
    assert_eq!(item(&undone)["type"], "skipped", "{undone}");
    assert!(refs(&mut host).contains(&fresh), "the undo deleted a page someone turned");
}

#[test]
fn an_undone_copy_in_deletes_the_copied_pages() {
    let mut host = open();
    let before = list(&mut host);
    let pages = refs(&mut host);
    let two = extracted(&mut host, &pages[..2]);
    apply_with(
        &mut host,
        "i1",
        json!([{ "type": "pages.insert", "source": { "payload": 0 }, "position": "start" }]),
        vec![two],
    )
    .expect("insert");
    let undone = undo(&mut host, "i1", "u1");
    assert_eq!(item(&undone)["type"], "pages.delete");
    assert_eq!(list(&mut host), before);
}
