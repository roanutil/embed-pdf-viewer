//! `annotations.flatten` and `annotations.exportAppearance` through the
//! host: the argument keys, the event, and the payload. What each does to
//! the page is pinned by `pages_mutate.rs` against the TypeScript.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture, page_ref};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";

/// The wire's name for the annotation whose `/NM` is `nm`: the `ref` the
/// list answers (an object number, or a `baseIndex` for one born inline).
fn by_nm(host: &mut Host, doc: &str, page: u64, nm: &str) -> Value {
    let listed = list(host, doc, page);
    let found = listed["annotations"]
        .as_array()
        .and_then(|all| all.iter().find(|a| a["nm"] == nm))
        .unwrap_or_else(|| panic!("no annotation named {nm}: {listed}"));
    found["ref"].clone()
}

/// The page's annotations (`annotations.list`).
fn list(host: &mut Host, doc: &str, page: u64) -> Value {
    call(host, json!({ "kind": "annotations.list", "docId": doc, "pages": [page_ref(page)] }))
}

#[test]
fn an_applied_flatten_publishes_annotations_flattened() {
    let mut host = Host::new();
    let (doc, page) = open_fixture(&mut host, FIXTURE);
    let target = by_nm(&mut host, &doc, page, "a-three-mode-square");
    let answer = call(
        &mut host,
        json!({ "kind": "annotations.flatten", "docId": doc,
        "page": page_ref(page), "refs": [target], "usage": "display" }),
    );
    assert_eq!(answer["event"]["type"], json!("annotations.flattened"), "{answer}");
    assert_eq!(answer["result"]["results"][0]["status"], json!("applied"));
    let pins = &answer["result"]["meta"]["affectedPages"][0];
    assert_eq!(pins["revision"]["generation"], json!(1), "{answer}");
    // The flattener notes the mutation and the finish notes it again, as
    // `WorkerHost.finishMutation` does for this tag.
    assert_eq!(answer["mutationSequence"], json!(2), "{answer}");
}

#[test]
fn a_flatten_that_applies_nothing_is_no_mutation() {
    let mut host = Host::new();
    let (doc, page) = open_fixture(&mut host, FIXTURE);
    let target = by_nm(&mut host, &doc, page, "a-bare-highlight");
    let answer = call(
        &mut host,
        json!({ "kind": "annotations.flatten", "docId": doc,
        "page": page_ref(page), "refs": [target], "usage": "display" }),
    );
    assert!(answer.get("event").is_none(), "{answer}");
    assert!(answer["result"]["meta"].is_null());
    assert_eq!(answer["result"]["results"][0]["status"], json!("skipped"));
    assert_eq!(answer["mutationSequence"], json!(0));
}

#[test]
fn an_export_answers_a_pdf_and_leaves_the_page_alone() {
    let mut host = Host::new();
    let (doc, page) = open_fixture(&mut host, FIXTURE);
    let before = list(&mut host, &doc, page);
    let target = by_nm(&mut host, &doc, page, "a-rotated-polygon");
    let response = host.call(
        &json!({ "kind": "annotations.exportAppearance", "jobId": 4, "docId": doc,
            "page": page_ref(page), "refs": [target] })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{envelope}");
    assert_eq!(response.payloads.len(), 1);
    assert!(response.payloads[0].starts_with(b"%PDF-"));
    assert_eq!(envelope["result"]["size"], json!(response.payloads[0].len()));
    let after = list(&mut host, &doc, page);
    assert_eq!(before, after, "an export is a read");
}
