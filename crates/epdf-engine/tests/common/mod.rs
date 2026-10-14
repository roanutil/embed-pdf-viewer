//! Helpers the engine's wire-level tests share. One block per plan, in
//! plan order (`docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md`).

#![allow(dead_code)]

use epdf_engine::host::Host;
use serde_json::{Value, json};

// ── 08 measurement ──────────────────────────────────────────────────────────

/// Open a repo-relative fixture as `docId` `"d"` and answer the doc id and
/// its first page's object number.
pub fn open_fixture(host: &mut Host, relative: &str) -> (String, u64) {
    let response = host.call(
        &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
        vec![test_support::fixture(relative)],
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "open failed: {envelope}");
    let pages = call(host, json!({ "kind": "pages.list", "docId": "d" }));
    let pon = pages["pages"][0]["pageObjectNumber"].as_u64().expect("a first page");
    ("d".into(), pon)
}

/// Send `request` with a job id, panic on an `error` envelope, and answer
/// the `result`.
pub fn call(host: &mut Host, mut request: Value) -> Value {
    request["jobId"] = json!(2);
    let response = host.call(&request.to_string(), Vec::new());
    let mut envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_none(), "{request} failed: {envelope}");
    envelope["result"].take()
}

/// Send `request` and answer the `error` envelope it must produce.
pub fn call_err(host: &mut Host, mut request: Value) -> Value {
    request["jobId"] = json!(3);
    let response = host.call(&request.to_string(), Vec::new());
    let mut envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert!(envelope.get("error").is_some(), "{request} did not fail: {envelope}");
    envelope["error"].take()
}
