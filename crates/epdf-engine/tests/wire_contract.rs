//! Every wire kind reaches an implementation.
//!
//! Replays `vectors/wire/requests.json`, which the TypeScript compiler
//! produced by reading the `WorkerRequest` union: sixty-five kinds and the
//! arguments each one carries.
//!
//! # What a dispatcher can get wrong that a compiler cannot catch
//!
//! `Host::call` takes a string and a JSON body. Nothing in its signature
//! says which kinds exist, so a missing arm compiles, a misspelled argument
//! key compiles, and both fail at runtime against a host that has no way to
//! tell a protocol gap from an engine fault. That is the entire reason this
//! file exists.
//!
//! The check is deliberately weak about SUCCESS and strict about ROUTING. An
//! arm is exercised with a body built from the vector's own field names, and
//! what is asserted is that the kind does not come back `NotImplemented`:
//! whether the operation then succeeds depends on the fixture, and pinning
//! results is what the other sixty-odd suites do. What cannot be allowed is
//! a kind the dispatcher has never heard of.

use epdf_engine::error::EngineErrorCode;
use epdf_engine::host::{Host, Payloads};

const VECTOR: &str = "tooling/engine-conformance/vectors/wire/requests.json";
const DOC_ID: &str = "doc-1";

/// A document with annotations, forms, attachments and `/PieceInfo`, so most
/// arms have something real to act on.
const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

#[derive(Debug)]
struct Case {
    kind: String,
    fields: Vec<(String, bool)>,
    control: bool,
}

fn cases() -> Vec<Case> {
    let raw = std::fs::read_to_string(test_support::workspace_root().join(VECTOR))
        .unwrap_or_else(|e| panic!("read {VECTOR}: {e}"));
    let file: serde_json::Value = serde_json::from_str(&raw).expect("the vector parses");
    assert_eq!(file["schemaVersion"], 1, "the vector's envelope changed shape");
    file["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|case| Case {
            kind: case["kind"].as_str().expect("kind").to_owned(),
            fields: case["fields"]
                .as_array()
                .expect("fields")
                .iter()
                .map(|f| {
                    (
                        f["name"].as_str().expect("name").to_owned(),
                        f["optional"].as_bool().expect("optional"),
                    )
                })
                .collect(),
            control: case["control"].as_bool().expect("control"),
        })
        .collect()
}

fn open() -> Host {
    let mut host = Host::new();
    let bytes = std::fs::read(test_support::workspace_root().join(FIXTURE))
        .unwrap_or_else(|e| panic!("read {FIXTURE}: {e}"));
    let response = host.call(
        &serde_json::json!({ "kind": "open.fatMem", "docId": DOC_ID }).to_string(),
        vec![bytes],
    );
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert!(body.get("error").is_none(), "the fixture would not open: {}", response.json);
    host
}

/// Something of roughly the right shape for a required argument.
///
/// Not a valid value, and not trying to be: the operation may well refuse
/// it. What it must not do is refuse it as an unknown kind, and a body with
/// the argument missing entirely would be refused earlier than the arm that
/// would have read it.
fn placeholder(name: &str) -> serde_json::Value {
    match name {
        "pageObjectNumber" | "widget" | "toIndex" | "destIndex" | "pageIndex" | "count" => {
            serde_json::json!(1)
        }
        "pageObjectNumbers" => serde_json::json!([1]),
        "refs" => serde_json::json!([{ "kind": "index", "pageObjectNumber": 1, "index": 0,
            "revision": { "sessionId": "s", "pageObjectNumber": 1, "generation": 0 } }]),
        "ref" => serde_json::json!({ "kind": "key", "key": "nothing" }),
        "rotation" => serde_json::json!(90),
        "usage" => serde_json::json!("display"),
        "mode" => serde_json::json!("incremental"),
        "format" => serde_json::json!("fdf"),
        "size" => serde_json::json!({ "width": 612.0, "height": 792.0 }),
        "scope" => serde_json::json!({ "kind": "document" }),
        "request" => serde_json::json!({ "query": { "text": "the" } }),
        "draft" => serde_json::json!({ "subtype": "square",
            "rect": { "left": 0.0, "bottom": 0.0, "right": 1.0, "top": 1.0 } }),
        "patch" => serde_json::json!({}),
        "value" => serde_json::json!({ "kind": "text", "text": "x" }),
        "file" => serde_json::json!({ "name": "a.txt" }),
        "effects" => serde_json::json!([]),
        "encode" => serde_json::json!({ "format": "png" }),
        "weight" => serde_json::json!(400),
        "italic" => serde_json::json!(false),
        "path" | "basePath" => serde_json::json!("/nonexistent/for-this-check.pdf"),
        _ => serde_json::json!("x"),
    }
}

/// The body for one kind: the envelope, plus every required argument.
fn request(case: &Case) -> String {
    let mut body = serde_json::Map::new();
    body.insert("kind".into(), case.kind.as_str().into());
    body.insert("jobId".into(), "job-1".into());
    body.insert("docId".into(), DOC_ID.into());
    for (name, optional) in &case.fields {
        if *optional {
            continue;
        }
        body.insert(name.clone(), placeholder(name));
    }
    serde_json::Value::Object(body).to_string()
}

/// Enough payloads for any arm that wants one.
///
/// A PDF header rather than empty bytes, because an arm that opens its
/// payload as a document should fail on the CONTENT rather than on there
/// being nothing there, and the two look the same from here otherwise.
fn payloads() -> Payloads {
    vec![b"%PDF-1.7\n".to_vec(), b"%PDF-1.7\n".to_vec()]
}

#[test]
fn every_wire_kind_routes_to_an_implementation() {
    let cases = cases();
    assert_eq!(cases.len(), 65, "the vector and the protocol disagree about how many kinds exist");

    let mut unrouted = Vec::new();
    for case in &cases {
        // A control frame steers the worker rather than asking it for a
        // result. They are routed too, and covered by their own tests; what
        // they are not is safe to fire blindly here, since `shutdown`
        // destroys the registry the rest of the loop is using.
        if case.control {
            continue;
        }
        // Both open frames make a session, and the fixture is already open
        // under this id. Firing them would either collide with it or leave a
        // second session behind for the arms that follow.
        if case.kind.starts_with("open.") {
            continue;
        }

        let mut host = open();
        let response = host.call(&request(case), payloads());
        let body: serde_json::Value =
            serde_json::from_str(&response.json).expect("every response is JSON");
        let Some(error) = body.get("error") else { continue };
        let code = error["code"].as_str().unwrap_or("");
        if code == format!("{:?}", EngineErrorCode::NotImplemented) {
            unrouted.push(format!("{}: {}", case.kind, error["message"].as_str().unwrap_or("")));
        }
    }

    assert!(
        unrouted.is_empty(),
        "{} of {} kinds reach no implementation:\n  {}",
        unrouted.len(),
        cases.len(),
        unrouted.join("\n  "),
    );
}

#[test]
fn an_unknown_kind_is_refused_rather_than_ignored() {
    // The negative half, so the check above cannot pass by routing
    // everything to the same arm. A kind that is not in the protocol has to
    // come back as one this build does not implement.
    let mut host = open();
    let response = host.call(
        &serde_json::json!({ "kind": "pages.teleport", "docId": DOC_ID }).to_string(),
        Vec::new(),
    );
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::NotImplemented));
}

#[test]
fn a_request_that_is_not_json_is_a_wire_error_rather_than_a_panic() {
    // The dispatcher's whole contract is that it always answers. A host that
    // got nothing back could not tell a refused operation from a crashed
    // engine, which is the failure this shape exists to prevent.
    let mut host = Host::new();
    let response = host.call("{not json", Vec::new());
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("still JSON");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat));
    assert!(response.payloads.is_empty(), "a failure carried payloads");
}

#[test]
fn a_session_operation_without_a_document_names_what_is_missing() {
    // Reported rather than defaulted. An engine that picked "the only open
    // session" would work in every test and fail the moment a host opened
    // two documents.
    let mut host = Host::new();
    let response = host.call(&serde_json::json!({ "kind": "pages.list" }).to_string(), Vec::new());
    let body: serde_json::Value = serde_json::from_str(&response.json).expect("the response");
    assert_eq!(body["error"]["code"], format!("{:?}", EngineErrorCode::WireFormat));
    assert!(
        body["error"]["message"].as_str().unwrap_or("").contains("pages.list"),
        "the refusal does not say which request it was: {}",
        response.json,
    );
}
