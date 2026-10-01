use epdf_viewer_host::translate::{Decode, translate};
use serde_json::json;

#[test]
fn metadata_read_becomes_its_wire_kind() {
    let call = translate("d1", "metadata.read", None, &[], 7).unwrap();
    assert_eq!(call.request, json!({"kind": "metadata.read", "jobId": 7, "docId": "d1"}));
    assert!(matches!(call.decode, Decode::Field("metadata")));
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

use epdf_ffi::KernelThread;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::host_table::HostTable;
use epdf_viewer_host::native_engine::NativeEngine;

#[test]
fn the_native_engine_opens_a_pdf_and_lists_its_pages() {
    let bytes = test_support::fixture(test_support::REPORT);
    let thread =
        KernelThread::spawn(|post| Ok(NativeEngine::new(EngineWorker::spawn(), post))).unwrap();
    let answers = futures::executor::block_on(thread.call(move |engine: &mut NativeEngine| {
        let (tx, rx) = futures::channel::oneshot::channel();
        let id = HostTable::put(bytes);
        engine.open(json!({ "$host": id }), None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let list = list.as_ref().unwrap().clone();
                let first = list["pages"][0]["pageObjectNumber"].as_u64().unwrap() as u32;
                doc.call("page.text.read", Some(first), vec![]).on_settle(move |text| {
                    let _ = tx.send((list, text.clone()));
                });
            });
        });
        rx
    }))
    .unwrap();
    let (list, text) = futures::executor::block_on(answers).unwrap();
    assert_eq!(list["pages"].as_array().map_or(0, Vec::len), 14);
    assert!(text.is_ok(), "a page read reaches the host: {text:?}");
}
