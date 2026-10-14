//! Helpers the wire-level engine tests share.
//!
//! 16 signature: the `sig_*` helpers open a fixture through `open.fatMem`
//! (named after its file) and send a request, reading its `result` and
//! payloads.
#![allow(dead_code)]

use epdf_engine::host::Host;
use serde_json::{Value, json};

fn sig_read_fixture(path: &str) -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(path))
        .unwrap_or_else(|e| panic!("read {path}: {e}"))
}

fn sig_send(host: &mut Host, mut request: Value, payloads: Vec<Vec<u8>>) -> (Value, Vec<Vec<u8>>) {
    if request.get("jobId").is_none() {
        request["jobId"] = json!(1);
    }
    let response = host.call(&request.to_string(), payloads);
    let envelope: Value = serde_json::from_str(&response.json).expect("every response is JSON");
    assert!(envelope.get("error").is_none(), "{request} failed: {envelope}");
    (envelope["result"].clone(), response.payloads)
}

/// Open the workspace-relative `path` with `extra` merged into the
/// `open.fatMem` request. Answers the document id.
pub fn sig_open_fixture_with(host: &mut Host, path: &str, extra: Value) -> String {
    let doc = path.rsplit('/').next().unwrap_or(path).to_string();
    let mut request = json!({ "kind": "open.fatMem", "docId": doc });
    if let Value::Object(extra) = extra {
        for (k, v) in extra {
            request[k] = v;
        }
    }
    sig_send(host, request, vec![sig_read_fixture(path)]);
    doc
}

/// Open `path` with the default scope. Answers the document id and its
/// first page's object number.
pub fn sig_open_fixture(host: &mut Host, path: &str) -> (String, u32) {
    let doc = sig_open_fixture_with(host, path, json!({}));
    let pages = sig_call(host, json!({ "kind": "pages.list", "docId": doc }));
    let first = pages
        .as_array()
        .or_else(|| pages["pages"].as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.pointer("/ref/pageObjectNumber").or_else(|| p.get("pageObjectNumber")))
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no first page in {pages}")) as u32;
    (doc, first)
}

/// Send `request` and answer its `result`, failing on an error envelope.
pub fn sig_call(host: &mut Host, request: Value) -> Value {
    sig_send(host, request, Vec::new()).0
}

/// `call`, answering the first payload.
pub fn sig_call_bytes(host: &mut Host, request: Value) -> Vec<u8> {
    sig_call_bytes_with(host, request).1
}

/// `call`, answering the result and the first payload.
pub fn sig_call_bytes_with(host: &mut Host, request: Value) -> (Value, Vec<u8>) {
    let (result, mut payloads) = sig_send(host, request, Vec::new());
    assert!(!payloads.is_empty(), "no payload in the answer: {result}");
    (result, payloads.remove(0))
}

/// `call` with one payload.
pub fn sig_call_with_payload(host: &mut Host, request: Value, payload: Vec<u8>) -> Value {
    sig_send(host, request, vec![payload]).0
}
