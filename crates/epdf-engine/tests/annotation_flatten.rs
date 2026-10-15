//! `annotations.flatten` and `annotations.exportAppearance` through the
//! host: the argument keys, the event, and the payload. What each does to
//! the page is pinned by `pages_mutate.rs` against the TypeScript.

use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture, page_ref};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_appearances.pdf";

fn nm(page: u64, nm: &str) -> Value {
    json!({ "kind": "nm", "page": page_ref(page), "nm": nm })
}

#[test]
fn an_applied_flatten_publishes_annotations_flattened() {
    let mut host = Host::new();
    let (doc, page) = open_fixture(&mut host, FIXTURE);
    let answer = call(
        &mut host,
        json!({ "kind": "annotations.flatten", "docId": doc,
        "page": page_ref(page), "refs": [nm(page, "a-three-mode-square")], "usage": "display" }),
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
    let answer = call(
        &mut host,
        json!({ "kind": "annotations.flatten", "docId": doc,
        "page": page_ref(page), "refs": [nm(page, "a-bare-highlight")], "usage": "display" }),
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
    let before = call(
        &mut host,
        json!({ "kind": "annotations.listRawPage", "docId": doc,
        "page": page_ref(page) }),
    );
    let response = host.call(
        &json!({ "kind": "annotations.exportAppearance", "jobId": 4, "docId": doc,
            "page": page_ref(page), "refs": [nm(page, "a-rotated-polygon")] })
        .to_string(),
        Vec::new(),
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{envelope}");
    assert_eq!(response.payloads.len(), 1);
    assert!(response.payloads[0].starts_with(b"%PDF-"));
    assert_eq!(envelope["result"]["size"], json!(response.payloads[0].len()));
    let after = call(
        &mut host,
        json!({ "kind": "annotations.listRawPage", "docId": doc,
        "page": page_ref(page) }),
    );
    assert_eq!(before, after, "an export is a read");
}
