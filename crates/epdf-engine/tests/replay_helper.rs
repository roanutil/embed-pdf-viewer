//! `common::replay`: a recorded request goes out with this run's ids and
//! nothing else changed.

mod common;

use common::replay::{JOB, OP, Replay, substitute};
use serde_json::json;

/// `vectors/annotation/delete.json`'s first case, as recorded.
fn recorded_delete() -> serde_json::Value {
    json!({
        "kind": "annotations.delete", "effect": "write", "jobId": JOB, "opId": OP,
        "docId": "delete:by-object-number",
        "ref": { "kind": "objectNumber", "page": { "kind": "objectNumber", "objectNumber": 3 },
            "objectNumber": 5 },
        "authority": { "identity": {}, "grants": null },
    })
}

#[test]
fn the_ids_and_the_document_are_this_runs() {
    let sent = substitute(&recorded_delete(), "d", 7, "op-7");
    assert_eq!(sent["docId"], "d");
    assert_eq!(sent["jobId"], 7, "the job id is a number, as WorkerJobId is");
    assert_eq!(sent["opId"], "op-7");
}

#[test]
fn nothing_but_the_ids_and_the_document_changes() {
    let recorded = recorded_delete();
    let mut sent = substitute(&recorded, "d", 7, "op-7");
    for (key, value) in [("docId", &recorded["docId"]), ("jobId", &recorded["jobId"])] {
        sent[key] = value.clone();
    }
    sent["opId"] = recorded["opId"].clone();
    assert_eq!(sent, recorded);
}

#[test]
fn a_placeholder_is_replaced_at_any_depth() {
    // `document.applyChanges` names an op per change, inside the list.
    let recorded = json!({ "kind": "document.applyChanges", "jobId": JOB, "docId": "x",
        "changes": [{ "opId": OP, "change": { "ops": [] } }, { "opId": OP }] });
    let sent = substitute(&recorded, "d", 3, "op-3");
    assert_eq!(sent["changes"][0]["opId"], "op-3");
    assert_eq!(sent["changes"][1]["opId"], "op-3");
}

#[test]
fn only_the_exact_placeholders_are_replaced() {
    // A value that merely mentions one, and a key spelled like one, are data.
    let recorded = json!({ "kind": "metadata.update", "jobId": JOB, "docId": "x",
        "patch": { "title": "<job> and <op>", "subject": "<OP>" }, "<op>": 1 });
    let sent = substitute(&recorded, "d", 3, "op-3");
    assert_eq!(sent["patch"]["title"], "<job> and <op>");
    assert_eq!(sent["patch"]["subject"], "<OP>");
    assert_eq!(sent["<op>"], 1);
}

#[test]
fn a_request_without_a_doc_id_gets_this_runs() {
    let sent = substitute(&json!({ "kind": "pages.list", "jobId": JOB }), "d", 1, "op-1");
    assert_eq!(sent["docId"], "d");
}

#[test]
fn a_replay_reaches_the_host_and_answers_its_result() {
    let mut replay = Replay::open("tooling/engine-conformance/fixtures/annotation_kinds.pdf", "d");
    let recorded = json!({ "kind": "pages.list", "effect": "read", "jobId": JOB,
        "docId": "recorded-elsewhere" });
    let pages = replay.replay(&recorded).expect("pages.list answers");
    assert!(pages["pages"][0]["ref"]["objectNumber"].as_u64().is_some_and(|n| n > 0), "{pages}");
}

#[test]
fn a_replay_answers_the_hosts_error() {
    let mut replay = Replay::open("tooling/engine-conformance/fixtures/annotation_kinds.pdf", "d");
    let recorded = json!({ "kind": "pages.text", "effect": "read", "jobId": JOB, "docId": "x",
        "page": { "kind": "objectNumber", "objectNumber": 999_999 } });
    let error = replay.replay(&recorded).expect_err("no such page");
    assert_eq!(error["code"], "NotFound", "{error}");
}
