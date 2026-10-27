//! The wire surface a Swift or Kotlin facade drives.
//!
//! `native_surface.rs` covers the Phase 1 typed methods: open, pages,
//! geometry, render. This covers the other export, which is everything else
//! the engine can do, reached through one JSON call rather than through
//! sixty-five generated signatures.
//!
//! The uniffi glue is not exercised here and cannot be from Rust; what is,
//! is the part a generated binding cannot get wrong for us: that a request
//! and its payloads arrive together, that a binary result comes back beside
//! the JSON rather than inside it, and that all of it happens on the owner
//! thread where the PDFium handles live.

use std::sync::Arc;

use epdf_uniffi::{PdfEngine, PdfRequest};

const FIXTURE: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

fn engine() -> Arc<PdfEngine> {
    PdfEngine::new()
}

fn call(
    engine: &PdfEngine,
    request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> serde_json::Value {
    let response = engine
        .call(PdfRequest { json: request.to_string(), payloads })
        .expect("the engine thread answered");
    let json: serde_json::Value =
        serde_json::from_str(&response.json).expect("the envelope is JSON");
    json
}

fn call_full(
    engine: &PdfEngine,
    request: serde_json::Value,
    payloads: Vec<Vec<u8>>,
) -> (serde_json::Value, Vec<Vec<u8>>) {
    let response = engine
        .call(PdfRequest { json: request.to_string(), payloads })
        .expect("the engine thread answered");
    (serde_json::from_str(&response.json).expect("the envelope is JSON"), response.payloads)
}

fn open(engine: &PdfEngine) {
    let json = call(
        engine,
        serde_json::json!({ "kind": "open.fatMem", "effect": "open", "jobId": 901, "docId": "d" }),
        vec![test_support::fixture(FIXTURE)],
    );
    assert!(json.get("error").is_none(), "the fixture would not open: {json}");
}

#[test]
fn a_document_opens_reads_and_closes_through_one_call() {
    let engine = engine();
    open(&engine);

    let json = call(
        &engine,
        serde_json::json!({ "kind": "pages.list", "effect": "read", "jobId": 902, "docId": "d" }),
        Vec::new(),
    );
    assert!(
        !json["result"]["pages"].as_array().expect("a page list").is_empty(),
        "the fixture has pages",
    );

    let json = call(
        &engine,
        serde_json::json!({ "kind": "close", "effect": "close", "jobId": 903, "docId": "d" }),
        Vec::new(),
    );
    // `{ tag: 'close' }`: the tag alone, a null result.
    assert_eq!(json, serde_json::json!({ "result": null }));
    let json = call(
        &engine,
        serde_json::json!({ "kind": "pages.list", "effect": "read", "jobId": 904, "docId": "d" }),
        Vec::new(),
    );
    assert_eq!(json["error"]["code"], "DocNotOpen", "the close left the session open: {json}");
    engine.shutdown();
}

#[test]
fn a_binary_result_travels_beside_the_json_rather_than_inside_it() {
    // A saved document is megabytes. Base64 through the JSON would make it
    // half again as large and would make a facade decode it before it could
    // write a file.
    let engine = engine();
    open(&engine);

    let (json, payloads) = call_full(
        &engine,
        serde_json::json!({ "kind": "document.saveBuffer", "effect": "snapshot", "jobId": 905, "docId": "d", "mode": "incremental" }),
        Vec::new(),
    );
    assert_eq!(payloads.len(), 1, "the saved document did not arrive as a payload");
    assert_eq!(json["result"]["size"].as_u64(), Some(payloads[0].len() as u64));
    assert!(payloads[0].starts_with(b"%PDF-"));
    engine.shutdown();
}

#[test]
fn a_refused_operation_answers_rather_than_raising() {
    // Deliberately not a uniffi error. A facade that had to catch exceptions
    // for a document problem and read envelopes for everything else would
    // have two error paths for one kind of failure.
    let engine = engine();
    open(&engine);
    let json = call(
        &engine,
        serde_json::json!({ "kind": "pages.text", "effect": "read", "jobId": 906, "docId": "d", "page": { "kind": "objectNumber", "pageObjectNumber": 99_999 } }),
        Vec::new(),
    );
    assert!(json["error"]["code"].is_string(), "a refusal carried no code: {json}");
    engine.shutdown();
}

#[test]
fn the_wire_and_the_typed_methods_see_the_same_documents() {
    // Two surfaces, one engine. A facade migrating from the Phase 1 methods
    // to the wire must not find itself with two sets of open documents, and
    // a separate host per surface is exactly how that would happen.
    let engine = engine();
    let document =
        engine.open(test_support::fixture(FIXTURE), None).expect("the typed surface opens it");
    let typed = document.pages().expect("the typed page list");

    open(&engine);
    let json = call(
        &engine,
        serde_json::json!({ "kind": "pages.list", "effect": "read", "jobId": 907, "docId": "d" }),
        Vec::new(),
    );
    let wired = json["result"]["pages"].as_array().expect("the wire page list");

    assert_eq!(
        typed.len(),
        wired.len(),
        "the two surfaces disagree about how many pages the document has",
    );
    engine.shutdown();
}

#[test]
fn a_shut_down_engine_refuses_at_the_boundary_rather_than_in_an_envelope() {
    // The one failure that is NOT an envelope, and the distinction is worth
    // keeping. `shutdown` ends the owner thread, so there is nothing left to
    // route the request to: an envelope would be this crate inventing an
    // answer on behalf of an engine that no longer exists.
    //
    // A facade reads it the same way either way, and a pool reads it
    // differently: an envelope means retry with another request, and this
    // means get another engine.
    let engine = engine();
    open(&engine);
    engine.shutdown();

    let refused = engine
        .call(PdfRequest {
            json: serde_json::json!({ "kind": "pages.list", "effect": "read", "jobId": 908, "docId": "d" }).to_string(),
            payloads: Vec::new(),
        })
        .expect_err("a shut-down engine served a read");
    assert!(
        format!("{refused:?}").contains("DocNotOpen"),
        "the refusal does not say the engine is gone: {refused:?}",
    );
}
