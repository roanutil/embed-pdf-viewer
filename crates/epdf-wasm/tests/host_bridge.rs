//! The Stage B bridge: the whole engine, through the C ABI.
//!
//! What `borrowed_page.rs` is for Stage A, this is for Stage B. It covers
//! the three things that can be wrong between a host and the dispatcher and
//! that no Rust-side test of `Host` would catch: the framing of the payloads
//! in each direction, the ownership of the buffer handed back, and what
//! happens when the host gets any of it wrong.
//!
//! It runs on the host rather than in WASM. What is being tested is the ABI
//! and the ownership contract, which are the same either way; the linked
//! module is a separate gate.

use std::ffi::c_void;

use serde_json::Value;

const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

/// Frame a request's payloads the way a host does.
fn frame(payloads: &[&[u8]]) -> Vec<u8> {
    let mut out = (payloads.len() as u32).to_le_bytes().to_vec();
    for payload in payloads {
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(payload);
    }
    out
}

/// Take a response apart the way a host does: read the JSON length, then the
/// count, then each payload by its own length.
fn unframe(bytes: &[u8]) -> (Value, Vec<Vec<u8>>) {
    let mut at = 0usize;
    let u32_at = |at: &mut usize| {
        let value = u32::from_le_bytes(bytes[*at..*at + 4].try_into().expect("four bytes"));
        *at += 4;
        value as usize
    };
    let json_len = u32_at(&mut at);
    let json: Value =
        serde_json::from_slice(&bytes[at..at + json_len]).expect("the envelope is JSON");
    at += json_len;
    let count = u32_at(&mut at);
    let mut payloads = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u32_at(&mut at);
        payloads.push(bytes[at..at + len].to_vec());
        at += len;
    }
    assert_eq!(at, bytes.len(), "the frame has bytes nobody claimed");
    (json, payloads)
}

/// One call, taking and returning ownership exactly as the contract says.
fn call(host: *mut c_void, request: &str, payloads: &[&[u8]]) -> (Value, Vec<Vec<u8>>) {
    let framed = frame(payloads);
    let mut len = 0u32;
    let ptr = unsafe {
        epdf_wasm::epdf_host_call(
            host,
            request.as_ptr(),
            request.len() as u32,
            framed.as_ptr(),
            framed.len() as u32,
            &mut len,
        )
    };
    assert!(!ptr.is_null(), "a non-null out_len always gets a buffer");
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    unsafe { epdf_wasm::epdf_buffer_free(ptr, len) };
    unframe(&bytes)
}

fn fixture() -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join(FIXTURE))
        .unwrap_or_else(|e| panic!("read {FIXTURE}: {e}"))
}

fn open() -> *mut c_void {
    let host = epdf_wasm::epdf_host_create();
    assert!(!host.is_null(), "the engine was not created");
    let bytes = fixture();
    let (json, _) = call(
        host,
        &serde_json::json!({ "kind": "open.fatMem", "docId": "d" }).to_string(),
        &[&bytes],
    );
    assert!(json.get("error").is_none(), "the fixture would not open: {json}");
    host
}

#[test]
fn a_document_opens_reads_and_closes_across_the_abi() {
    // The path the whole crate exists for. The bytes go in as a payload, a
    // read comes back as JSON, and the close reports what it closed.
    let host = open();

    let (json, payloads) = call(
        host,
        &serde_json::json!({ "kind": "pages.list", "docId": "d" }).to_string(),
        &[],
    );
    let pages = json["result"]["pages"].as_array().expect("a page list");
    assert!(!pages.is_empty(), "the fixture has pages");
    assert!(payloads.is_empty(), "a JSON-only read returned payloads");

    let (json, _) =
        call(host, &serde_json::json!({ "kind": "close", "docId": "d" }).to_string(), &[]);
    assert_eq!(json["result"]["closedSessions"], 1);

    unsafe { epdf_wasm::epdf_host_destroy(host) };
}

#[test]
fn a_binary_result_comes_back_as_a_payload_rather_than_as_json() {
    // The half of the framing that only a save exercises. A PDF through the
    // JSON would be base64 and half again as large, on a path built to avoid
    // exactly that.
    let host = open();
    let (json, payloads) = call(
        host,
        &serde_json::json!({ "kind": "document.saveBuffer", "docId": "d", "mode": "incremental" })
            .to_string(),
        &[],
    );

    assert_eq!(payloads.len(), 1, "the saved document did not arrive as a payload");
    let saved = &payloads[0];
    assert_eq!(
        json["result"]["size"].as_u64(),
        Some(saved.len() as u64),
        "the reported size and the payload disagree",
    );
    assert!(saved.starts_with(b"%PDF-"), "the payload is not a PDF");

    unsafe { epdf_wasm::epdf_host_destroy(host) };
}

#[test]
fn a_refused_operation_answers_rather_than_failing_silently() {
    // The contract the whole envelope shape rests on: a host that got no
    // response could not tell a refused operation from a crashed engine.
    let host = open();
    let (json, payloads) = call(
        host,
        &serde_json::json!({ "kind": "pages.text", "docId": "d", "pageObjectNumber": 99_999 })
            .to_string(),
        &[],
    );
    assert!(json["error"]["code"].is_string(), "a refusal carried no code: {json}");
    assert!(payloads.is_empty(), "a refusal carried payloads");
    unsafe { epdf_wasm::epdf_host_destroy(host) };
}

#[test]
fn a_null_engine_is_refused_rather_than_dereferenced() {
    // The one mistake a host makes that would otherwise be a crash in a
    // process with no debugger attached.
    let (json, _) = call(
        std::ptr::null_mut(),
        &serde_json::json!({ "kind": "pages.list", "docId": "d" }).to_string(),
        &[],
    );
    assert_eq!(json["error"]["code"], "WireFormat");
}

#[test]
fn a_frame_that_does_not_parse_is_refused_rather_than_guessed_at() {
    // A count with nothing behind it. Reading the part that does parse would
    // hand the operation bytes that are not the ones it was sent, which is
    // worse than refusing.
    let host = open();
    let request = serde_json::json!({ "kind": "open.fatMem", "docId": "other" }).to_string();
    let broken = [9u32.to_le_bytes().to_vec(), 4u32.to_le_bytes().to_vec()].concat();

    let mut len = 0u32;
    let ptr = unsafe {
        epdf_wasm::epdf_host_call(
            host,
            request.as_ptr(),
            request.len() as u32,
            broken.as_ptr(),
            broken.len() as u32,
            &mut len,
        )
    };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    unsafe { epdf_wasm::epdf_buffer_free(ptr, len) };
    let (json, _) = unframe(&bytes);
    assert_eq!(json["error"]["code"], "WireFormat");

    unsafe { epdf_wasm::epdf_host_destroy(host) };
}

#[test]
fn a_request_that_is_not_utf8_is_refused() {
    // JSON is UTF-8 by definition, and a host that sent something else has a
    // bug worth naming rather than a parse error to puzzle over.
    let host = epdf_wasm::epdf_host_create();
    let invalid = [0xffu8, 0xfe, 0xfd];
    let mut len = 0u32;
    let ptr = unsafe {
        epdf_wasm::epdf_host_call(
            host,
            invalid.as_ptr(),
            invalid.len() as u32,
            std::ptr::null(),
            0,
            &mut len,
        )
    };
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec();
    unsafe { epdf_wasm::epdf_buffer_free(ptr, len) };
    let (json, _) = unframe(&bytes);
    assert_eq!(json["error"]["code"], "WireFormat");
    unsafe { epdf_wasm::epdf_host_destroy(host) };
}

#[test]
fn destroying_an_engine_twice_is_the_host_s_bug_and_destroying_null_is_not() {
    // Null is explicitly fine, because a host unwinding from a failed create
    // has a null pointer and nothing else to do with it.
    unsafe { epdf_wasm::epdf_host_destroy(std::ptr::null_mut()) };
}

#[test]
fn two_engines_hold_separate_sessions() {
    // One process, several engines. The server pool wants this and the C ABI
    // is where a global would otherwise have crept in: an engine that reached
    // for shared state would work in every single-engine test and lose
    // documents the moment a pool held two.
    let first = open();
    let second = epdf_wasm::epdf_host_create();
    assert!(!second.is_null(), "a second engine on this thread was refused");

    // The second engine is a working engine, not a null pointer. Asserting
    // only that it cannot see the first engine's document would pass for a
    // null one too, since a null engine refuses everything.
    let bytes = fixture();
    let (json, _) = call(
        second,
        &serde_json::json!({ "kind": "open.fatMem", "docId": "own" }).to_string(),
        &[&bytes],
    );
    assert!(json.get("error").is_none(), "the second engine cannot open a document: {json}");

    let (json, _) = call(
        second,
        &serde_json::json!({ "kind": "pages.list", "docId": "d" }).to_string(),
        &[],
    );
    assert!(
        json["error"].is_object(),
        "the second engine could see the first engine's document: {json}",
    );

    unsafe { epdf_wasm::epdf_host_destroy(first) };
    unsafe { epdf_wasm::epdf_host_destroy(second) };
}
