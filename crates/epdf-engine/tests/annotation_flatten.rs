//! `annotations.flatten`, `annotations.exportAppearance` and `pages.flatten`
//! through the host: refs, results, `meta`, the event and the payload.
//! Ports upstream's `runAnnotationFlattenConformance`,
//! `runAnnotationAppearanceExportConformance` and `runPageFlattenConformance`
//! (`packages/engine/core/src/conformance/`), over the same `sample.pdf`.
//! What each does to the corpus's pages is pinned by `flatten_corpus.rs`.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::page_ref;
use common::replay::Replay;

/// Upstream's fixture for these suites: more than one page, so there is a
/// second page for the foreign-ref cases.
const SAMPLE: &str = "examples/engine-runtime-demo/public/sample.pdf";
const APPEARANCES: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";

/// What one request answered: the whole result envelope, or the error.
fn send(replay: &mut Replay, request: Value) -> Result<Value, Value> {
    replay.replay(&request)
}

fn pages(replay: &mut Replay) -> Value {
    send(replay, json!({ "kind": "pages.list", "jobId": "<job>", "docId": "<doc>" }))
        .expect("pages.list")
}

fn first_pages(replay: &mut Replay) -> (u64, u64) {
    let layout = pages(replay);
    let pon = |i: usize| layout["pages"][i]["ref"]["objectNumber"].as_u64().expect("a page");
    (pon(0), pon(1))
}

fn listed(replay: &mut Replay, pon: u64) -> Vec<Value> {
    let listing = send(
        replay,
        json!({ "kind": "annotations.list", "jobId": "<job>", "docId": "<doc>",
            "pages": [page_ref(pon)] }),
    )
    .expect("annotations.list");
    listing["annotations"].as_array().cloned().unwrap_or_default()
}

fn refs(annotations: &[Value]) -> Vec<Value> {
    annotations.iter().map(|a| a["ref"].clone()).collect()
}

/// `square(x, y, size)` of the suites, created on `pon` under `op_id`.
fn square(replay: &mut Replay, pon: u64, x: f64, y: f64, size: f64, op_id: &str) -> Value {
    let created = send(
        replay,
        json!({ "kind": "annotations.create", "jobId": "<job>", "opId": op_id, "docId": "<doc>",
            "page": page_ref(pon), "draft": { "subtype": "square",
            "box": { "x": x, "y": y, "width": size, "height": size },
            "color": "#1428dc", "strokeWidth": 2 } }),
    )
    .expect("a square");
    created["result"]["annotation"]["ref"].clone()
}

fn flatten(replay: &mut Replay, pon: u64, refs: &[Value], op_id: &str) -> Result<Value, Value> {
    send(
        replay,
        json!({ "kind": "annotations.flatten", "effect": "contentWrite", "jobId": "<job>",
            "opId": op_id, "docId": "<doc>", "page": page_ref(pon), "refs": refs,
            "usage": "display" }),
    )
}

fn export(replay: &mut Replay, pon: u64, refs: &[Value]) -> Result<Vec<u8>, Value> {
    let answer = replay.send(
        &json!({ "kind": "annotations.exportAppearance", "effect": "snapshot", "jobId": "<job>",
            "docId": "<doc>", "page": page_ref(pon), "refs": refs }),
        Vec::new(),
    );
    let result = answer.outcome?;
    assert_eq!(answer.payloads.len(), 1, "{result}");
    assert_eq!(result["size"], json!(answer.payloads[0].len()), "{result}");
    Ok(answer.payloads.into_iter().next().expect("one payload"))
}

fn open(fixture: &str) -> Replay {
    Replay::open(fixture, "flatten")
}

/// "flattens exactly the given refs and reports per-ref status".
#[test]
fn a_flatten_applies_exactly_the_given_refs_and_reports_each() {
    let mut replay = open(SAMPLE);
    let layout_before = pages(&mut replay);
    let (pon, _) = first_pages(&mut replay);
    let a = square(&mut replay, pon, 20.0, 20.0, 40.0, "op-a");
    let b = square(&mut replay, pon, 80.0, 20.0, 40.0, "op-b");
    let c = square(&mut replay, pon, 140.0, 20.0, 40.0, "op-c");
    // Hidden, so a display flatten leaves it.
    let hidden = send(
        &mut replay,
        json!({ "kind": "annotations.update", "jobId": "<job>", "opId": "op-hide",
            "docId": "<doc>", "ref": c, "patch": { "subtype": "square", "hidden": true } }),
    )
    .expect("hidden");
    let before = listed(&mut replay, pon);

    let answer = flatten(&mut replay, pon, &[a.clone(), c.clone()], "op-flatten").expect("flat");
    let result = &answer["result"];
    assert_eq!(result["page"], page_ref(pon));
    assert_eq!(result["usage"], "display");
    assert_eq!(
        result["results"],
        json!([{ "ref": a, "status": "applied" }, { "ref": c, "status": "unchanged" }])
    );
    assert_eq!(
        result["meta"],
        json!({ "affectedPages": [page_ref(pon)], "cacheDelta": null, "opId": "op-flatten",
            "undoable": false }),
    );
    assert_eq!(answer["wrote"], true);
    // One bump for the write, where this engine used to bump twice.
    assert_eq!(answer["mutationSequence"], json!(hidden["mutationSequence"].as_u64().unwrap() + 1));

    // One event, the result as the one fact of the write.
    let event = &answer["event"];
    assert_eq!(event["type"], "annotations.flattened", "{event}");
    for (key, value) in result.as_object().expect("an object") {
        assert_eq!(&event[key], value, "the event changed '{key}'");
    }
    assert_eq!(event["origin"]["tx"], json!({ "id": "op-flatten", "index": 0, "count": 1 }));

    // The layout is untouched; `a` is content now, `b` and `c` remain.
    assert_eq!(pages(&mut replay), layout_before);
    let after = refs(&listed(&mut replay, pon));
    assert_eq!(after.len(), before.len() - 1);
    assert!(after.contains(&b) && after.contains(&c), "{after:?}");
    assert!(!after.contains(&a), "{after:?}");
}

#[test]
fn a_flatten_that_applies_nothing_is_no_mutation() {
    let mut replay = open(SAMPLE);
    let (pon, _) = first_pages(&mut replay);
    let c = square(&mut replay, pon, 140.0, 20.0, 40.0, "op-c");
    let hidden = send(
        &mut replay,
        json!({ "kind": "annotations.update", "jobId": "<job>", "opId": "op-hide",
            "docId": "<doc>", "ref": c, "patch": { "subtype": "square", "hidden": true } }),
    )
    .expect("hidden");
    let before = listed(&mut replay, pon);

    let answer = flatten(&mut replay, pon, std::slice::from_ref(&c), "op-noop").expect("flat");
    assert_eq!(answer["result"]["results"], json!([{ "ref": c, "status": "unchanged" }]));
    assert_eq!(
        answer["result"]["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "op-noop", "undoable": false }),
    );
    assert_eq!(answer["wrote"], false);
    assert!(answer.get("event").is_none(), "{answer}");
    assert_eq!(answer["mutationSequence"], hidden["mutationSequence"], "no mutation");
    assert_eq!(listed(&mut replay, pon), before);
}

/// A flatten is final: it ends undo for every change before it.
#[test]
fn a_flatten_ends_undo_for_the_changes_before_it() {
    let undo = |replay: &mut Replay| {
        send(
            replay,
            json!({ "kind": "document.apply", "jobId": "<job>", "opId": "op-undo",
                "docId": "<doc>", "change": { "undoOf": "op-b" } }),
        )
    };
    for flattened in [false, true] {
        let mut replay = open(SAMPLE);
        let (pon, _) = first_pages(&mut replay);
        let a = square(&mut replay, pon, 20.0, 20.0, 40.0, "op-a");
        square(&mut replay, pon, 80.0, 20.0, 40.0, "op-b");
        if !flattened {
            undo(&mut replay).expect("a create undoes");
            continue;
        }
        flatten(&mut replay, pon, &[a], "op-flatten").expect("flat");
        let refused = undo(&mut replay).expect_err("undo after a flatten");
        assert_eq!(refused["code"], "UndoUnavailable", "{refused}");
    }
}

/// "a ref on another page rejects InvalidArg and mutates nothing".
#[test]
fn a_ref_on_another_page_is_refused_and_flattens_nothing() {
    let mut replay = open(SAMPLE);
    let (pon, other) = first_pages(&mut replay);
    let own = square(&mut replay, pon, 20.0, 100.0, 40.0, "op-own");
    let foreign = square(&mut replay, other, 20.0, 100.0, 40.0, "op-foreign");
    let before = listed(&mut replay, pon);
    let refused = flatten(&mut replay, pon, &[own, foreign], "op-flatten").expect_err("foreign");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(
        refused["message"],
        format!("annotations.flatten refs must all target page {pon}; got ref on page {other}")
    );
    assert_eq!(listed(&mut replay, pon), before);
}

/// "an empty ref list rejects InvalidArg; an unknown ref rejects NotFound".
#[test]
fn no_refs_is_malformed_and_an_unknown_ref_is_not_found() {
    let mut replay = open(SAMPLE);
    let (pon, _) = first_pages(&mut replay);
    let empty = flatten(&mut replay, pon, &[], "op-empty").expect_err("no refs");
    assert_eq!(empty["code"], "InvalidArg", "{empty}");
    assert_eq!(empty["message"], "annotations.flatten requires at least one ref");
    let unknown = json!({ "kind": "objectNumber", "page": page_ref(pon),
        "objectNumber": 987_654_321 });
    let refused = flatten(&mut replay, pon, &[unknown], "op-unknown").expect_err("unknown");
    assert_eq!(refused["code"], "NotFound", "{refused}");
}

/// The wire's old names are not names any more.
#[test]
fn an_nm_ref_is_not_a_name() {
    let mut replay = open(APPEARANCES);
    let pon = pages(&mut replay)["pages"][0]["ref"]["objectNumber"].as_u64().expect("a page");
    let refused = flatten(
        &mut replay,
        pon,
        &[json!({ "kind": "nm", "page": page_ref(pon), "nm": "a-three-mode-square" })],
        "op-nm",
    )
    .expect_err("an nm ref");
    assert_eq!(refused["code"], "WireFormat", "{refused}");
}

/// "returns a single-page PDF sized to the union rect; source untouched".
#[test]
fn an_export_is_one_page_the_size_of_the_union_and_leaves_the_source_alone() {
    let mut replay = open(SAMPLE);
    let (pon, _) = first_pages(&mut replay);
    let a = square(&mut replay, pon, 20.0, 20.0, 40.0, "op-a");
    let b = square(&mut replay, pon, 100.0, 60.0, 30.0, "op-b");
    let before = listed(&mut replay, pon);

    let bytes = export(&mut replay, pon, &[a, b]).expect("exported");
    assert!(bytes.starts_with(b"%PDF"), "not a PDF");
    assert_eq!(listed(&mut replay, pon), before, "an export is a read");

    let mut host = Host::new();
    let opened = host.call(
        &json!({ "kind": "open.fatMem", "effect": "open", "jobId": 1, "docId": "exported",
            "password": null })
        .to_string(),
        vec![bytes],
    );
    assert!(!opened.json.contains("\"error\""), "{}", opened.json);
    let mut exported = Replay::adopt(host, "exported");
    let layout = pages(&mut exported);
    assert_eq!(layout["pageCount"], 1, "{layout}");
    // The union of [20..60]x[20..60] and [100..130]x[60..90]: 110 x 70.
    let size = &layout["pages"][0]["size"];
    assert_eq!(size["width"].as_f64().map(f64::round), Some(110.0), "{size}");
    assert_eq!(size["height"].as_f64().map(f64::round), Some(70.0), "{size}");
    let exported_pon = layout["pages"][0]["ref"]["objectNumber"].as_u64().expect("its page");
    assert!(listed(&mut exported, exported_pon).is_empty(), "the appearances are content");
}

/// "a ref on another page and an empty list reject InvalidArg".
#[test]
fn an_export_of_no_refs_or_a_foreign_ref_is_refused() {
    let mut replay = open(SAMPLE);
    let (pon, other) = first_pages(&mut replay);
    let empty = export(&mut replay, pon, &[]).expect_err("no refs");
    assert_eq!(empty["code"], "InvalidArg", "{empty}");
    assert_eq!(empty["message"], "annotations.exportAppearance requires at least one ref");
    let own = square(&mut replay, pon, 20.0, 100.0, 40.0, "op-own");
    let foreign = square(&mut replay, other, 20.0, 100.0, 40.0, "op-foreign");
    let refused = export(&mut replay, pon, &[own, foreign]).expect_err("foreign");
    assert_eq!(refused["code"], "InvalidArg", "{refused}");
    assert_eq!(
        refused["message"],
        format!(
            "annotations.exportAppearance refs must all target page {pon}; got ref on page {other}"
        )
    );
}

/// "flattens eligible appearances once without changing layout".
#[test]
fn a_page_flatten_applies_once_and_then_finds_nothing() {
    let mut replay = open(SAMPLE);
    let layout_before = pages(&mut replay);
    let (pon, _) = first_pages(&mut replay);
    square(&mut replay, pon, 20.0, 20.0, 40.0, "op-a");
    let before = listed(&mut replay, pon);
    let page_flatten = |replay: &mut Replay, pages: Value, op_id: &str| {
        send(
            replay,
            json!({ "kind": "pages.flatten", "effect": "contentWrite", "jobId": "<job>",
                "opId": op_id, "docId": "<doc>", "pages": pages, "usage": "display" }),
        )
    };

    let answer = page_flatten(&mut replay, json!([page_ref(pon)]), "op-1").expect("flat");
    let result = &answer["result"];
    assert_eq!(
        *result,
        json!({ "pages": [page_ref(pon)], "usage": "display",
            "results": [{ "page": page_ref(pon), "status": "applied" }],
            "meta": { "affectedPages": [page_ref(pon)], "cacheDelta": null, "opId": "op-1",
                "undoable": false } }),
    );
    assert_eq!(answer["wrote"], true);
    let event = &answer["event"];
    assert_eq!(event["type"], "pages.flattened", "{event}");
    assert_eq!(event["pages"], result["pages"]);
    assert_eq!(event["origin"]["tx"]["id"], "op-1");
    assert_eq!(pages(&mut replay), layout_before);
    assert!(listed(&mut replay, pon).len() < before.len());

    let sequence = answer["mutationSequence"].clone();
    let again = page_flatten(&mut replay, json!([page_ref(pon)]), "op-2").expect("flat already");
    assert_eq!(
        again["result"]["results"],
        json!([{ "page": page_ref(pon), "status": "unchanged" }])
    );
    assert_eq!(
        again["result"]["meta"],
        json!({ "affectedPages": [], "cacheDelta": null, "opId": "op-2", "undoable": false }),
    );
    assert_eq!(again["wrote"], false);
    assert!(again.get("event").is_none(), "{again}");
    assert_eq!(again["mutationSequence"], sequence);

    let twice =
        page_flatten(&mut replay, json!([page_ref(pon), page_ref(pon)]), "op-3").expect_err("dup");
    assert_eq!(twice["code"], "InvalidArg", "{twice}");
    assert_eq!(
        twice["message"],
        format!("pages.flatten was given duplicate page object number {pon}")
    );
    let none = page_flatten(&mut replay, json!([]), "op-4").expect_err("no pages");
    assert_eq!(none["message"], "pages.flatten requires at least one page", "{none}");
}
