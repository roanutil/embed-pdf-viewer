//! Helpers the engine's wire-level tests share.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU64, Ordering};

use epdf_engine::host::Host;
use serde_json::{Value, json};

static JOB: AtomicU64 = AtomicU64::new(1);

/// Send `request` with a fresh `jobId`; panic on an `error` envelope and
/// answer `result`.
pub fn call(host: &mut Host, mut request: Value) -> Value {
    request["jobId"] = json!(JOB.fetch_add(1, Ordering::Relaxed));
    let response = host.call(&request.to_string(), Vec::new());
    let envelope: Value = serde_json::from_str(&response.json).expect("every response is JSON");
    assert!(envelope.get("error").is_none(), "{request} failed: {envelope}");
    envelope["result"].clone()
}

/// Open a workspace-relative fixture and answer its doc id and the first
/// page's object number.
pub fn open_fixture(host: &mut Host, fixture: &str) -> (String, u32) {
    let bytes = std::fs::read(test_support::workspace_root().join(fixture))
        .unwrap_or_else(|e| panic!("read {fixture}: {e}"));
    let doc = format!("doc-{}", JOB.fetch_add(1, Ordering::Relaxed));
    let opened = host
        .call(&json!({ "kind": "open.fatMem", "jobId": 0, "docId": doc }).to_string(), vec![bytes]);
    let envelope: Value = serde_json::from_str(&opened.json).unwrap();
    assert!(envelope.get("error").is_none(), "open {fixture}: {envelope}");
    let list = call(host, json!({ "kind": "pages.list", "docId": doc }));
    let pon = list["pages"][0]["pageObjectNumber"].as_u64().expect("a first page") as u32;
    (doc, pon)
}
