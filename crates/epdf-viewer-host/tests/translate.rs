use std::cell::RefCell;
use std::rc::Rc;

use epdf_engine::host::HostResponse;
use epdf_ffi::KernelThread;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::host_table::HostTable;
use epdf_viewer_host::native_engine::NativeEngine;
use epdf_viewer_host::translate::{Decode, decode, translate};
use futures::channel::oneshot;
use serde_json::{Value, json};

#[test]
fn metadata_read_becomes_its_wire_kind() {
    let call = translate("d1", "metadata.read", None, &[], 7).unwrap();
    assert_eq!(call.request, json!({"kind": "metadata.read", "jobId": 7, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Result));
}

#[test]
fn a_page_chain_carries_its_page() {
    let call = translate("d1", "page.render.image", Some(12), &[json!({"scale": 2})], 8).unwrap();
    assert_eq!(
        call.request,
        json!({
            "kind": "pages.render", "jobId": 8, "docId": "d1",
            "pageObjectNumber": 12,
            "options": {"scale": 2}
        })
    );
    assert!(matches!(call.decode, Decode::Raster));
}

#[test]
fn a_page_chain_without_a_page_is_invalid_input() {
    let error = translate("d1", "page.text.read", None, &[], 9).err().unwrap();
    assert_eq!(error.code(), Some("invalid-input"));
}

#[test]
fn an_unmapped_path_is_unsupported_not_a_panic() {
    let error = translate("d1", "signatures.prepare", None, &[], 10).err().unwrap();
    assert_eq!(error.code(), Some("unsupported"));
}

// ── decoding, against envelopes the host emitted for the report fixture ──

fn answer(json: &str) -> HostResponse {
    HostResponse { json: json.into(), payloads: Vec::new() }
}

const METADATA: &str = r#"{"author":null,"created":"2009-04-01T23:39:25.000Z","creator":"TeX","custom":{},"keywords":null,"modified":null,"producer":"pdfeTeX-1.21a","subject":null,"title":"T","trapped":"unknown"}"#;

#[test]
fn a_mutation_answers_its_inner_result_and_hands_back_its_event() {
    let json = format!(
        r#"{{"result":{{"event":{{"metadata":{METADATA},"origin":{{"kind":"local","serverId":null,"sessionId":"sess_1","sub":null,"ts":1790879896560}},"type":"metadata.updated"}},"mutationSequence":1,"result":{{"metadata":{METADATA}}}}}}}"#
    );
    let decoded = decode(&Decode::Mutation, answer(&json)).unwrap();
    let metadata: Value = serde_json::from_str(METADATA).unwrap();
    assert_eq!(decoded.value, json!({ "metadata": metadata }));
    let event = decoded.event.unwrap();
    assert_eq!(event["type"], "metadata.updated");
    assert_eq!(event["metadata"], metadata);
}

#[test]
fn a_page_list_gives_each_page_its_ref_and_keeps_the_host_fields() {
    let json = r#"{"result":{"pageCount":1,"pages":[{"boxes":{"crop":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0},"media":{"bottom":0.0,"left":0.0,"right":612.0,"top":792.0}},"index":0,"label":null,"pageObjectNumber":2,"rotation":0,"size":{"height":792.0,"width":612.0},"userUnit":1.0}]}}"#;
    let page = &decode(&Decode::PageList, answer(json)).unwrap().value["pages"][0];
    assert_eq!(page["ref"], json!({"kind": "objectNumber", "pageObjectNumber": 2}));
    assert_eq!(page["pageObjectNumber"], 2);
    assert_eq!(page["size"], json!({"height": 792.0, "width": 612.0}));
}

#[test]
fn a_read_answers_the_bare_result() {
    let json = format!(r#"{{"result":{METADATA}}}"#);
    let decoded = decode(&Decode::Result, answer(&json)).unwrap();
    assert_eq!(decoded.value["title"], "T");
    assert!(decoded.event.is_none());
}

#[test]
fn a_missing_member_is_an_error_not_the_whole_result() {
    let json = format!(r#"{{"result":{{"metadata":{METADATA}}}}}"#);
    assert!(decode(&Decode::Mutation, answer(&json)).is_err());
    assert!(decode(&Decode::Field("security"), answer(&json)).is_err());
    assert!(decode(&Decode::PageList, answer(r#"{"result":{"pageCount":0}}"#)).is_err());
    assert!(decode(&Decode::Result, answer("{}")).is_err());
}

#[test]
fn a_host_error_is_an_engine_error() {
    let json = r#"{"error":{"code":"DocPasswordRequired","message":"document e is locked and requires a password"}}"#;
    let error = decode(&Decode::PageList, answer(json)).err().unwrap();
    assert_eq!(error.code(), Some("DocPasswordRequired"));
}

// ── the engine trait over real PDFium ─────────────────────────────────────

/// Run `job` on a kernel thread over an engine on `worker`, and wait for what
/// it sends.
fn on_kernel<T: Send + 'static>(
    worker: EngineWorker,
    job: impl FnOnce(&NativeEngine, oneshot::Sender<T>) + Send + 'static,
) -> T {
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::new(worker, post))).unwrap();
    let answer = futures::executor::block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel();
        job(engine, tx);
        rx
    }))
    .unwrap();
    futures::executor::block_on(answer).unwrap()
}

fn open_input(fixture: &str) -> Value {
    json!({ "$host": HostTable::put(test_support::fixture(fixture)) })
}

#[test]
fn the_native_engine_opens_a_pdf_and_lists_its_pages() {
    let (list, text, index, missing) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let list = list.as_ref().unwrap().clone();
                let first = list["pages"][0]["ref"]["pageObjectNumber"].as_u64().unwrap() as u32;
                let index = doc.page_index(first);
                let missing =
                    doc.page_index(u32::MAX).err().and_then(|e| e.code().map(str::to_owned));
                doc.call("page.text.read", Some(first), vec![]).on_settle(move |text| {
                    let _ = tx.send((list, text.clone(), index, missing));
                });
            });
        });
    });
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
    assert!(text.is_ok(), "a page read reaches the host: {text:?}");
    assert_eq!(index.ok(), Some(0));
    assert_eq!(missing.as_deref(), Some("NotFound"));
}

#[test]
fn a_metadata_update_answers_its_result_after_publishing_its_event() {
    let (result, events) = on_kernel(EngineWorker::spawn(), |engine, tx| {
        engine.open(open_input(test_support::REPORT), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            let events = Rc::new(RefCell::new(Vec::<Value>::new()));
            let seen = events.clone();
            let off =
                doc.subscribe(Rc::new(move |event: &Value| seen.borrow_mut().push(event.clone())));
            doc.call("metadata.update", None, vec![json!({ "title": "Native" })]).on_settle(
                move |result| {
                    let delivered = events.borrow().clone();
                    off.call();
                    let _ = tx.send((result.clone(), delivered));
                },
            );
        });
    });
    let result = result.unwrap();
    assert_eq!(result["metadata"]["title"], "Native");
    assert_eq!(events.len(), 1, "one event, delivered before the call resolved");
    assert_eq!(events[0]["type"], "metadata.updated");
    assert_eq!(events[0]["metadata"]["title"], "Native");
}

#[test]
fn a_locked_open_rejects_and_leaves_no_session_on_the_host() {
    let worker = EngineWorker::spawn();
    let code = on_kernel(worker.clone(), |engine, tx| {
        engine.open(open_input(test_support::ENCRYPTED), None).on_settle(move |doc| {
            let _ = tx.send(doc.as_ref().err().and_then(|e| e.code().map(str::to_owned)));
        });
    });
    assert_eq!(code.as_deref(), Some("DocPasswordRequired"));
    // The engine's own close went out before the open rejected, and the
    // worker is first in, first out, so a second close finds nothing.
    let (tx, rx) = oneshot::channel();
    worker.submit(
        json!({ "kind": "close", "jobId": 0, "docId": "doc-1" }).to_string(),
        Vec::new(),
        Box::new(move |response| {
            let _ = tx.send(response.json);
        }),
    );
    let answer: Value = serde_json::from_str(&futures::executor::block_on(rx).unwrap()).unwrap();
    assert_eq!(answer, json!({ "result": { "closedSessions": 0 } }));
}
