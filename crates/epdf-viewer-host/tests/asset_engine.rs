//! The stamp plugin's asset engine: a second `NativeEngine` on its own
//! engine thread, opening library bytes and rendering encoded previews.

use epdf_engine::host::Host;
use epdf_ffi::KernelThread;
use epdf_kernel::bytes::Bytes;
use epdf_kernel::engine::Engine;
use epdf_viewer_host::engine_worker::EngineWorker;
use epdf_viewer_host::native_engine::NativeEngine;
use futures::channel::oneshot;
use futures::executor::block_on;
use serde_json::{Value, json};

fn stamps() -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join("packages/default-stamps/en/stamps.pdf"))
        .unwrap()
}

#[test]
fn the_asset_engine_opens_bytes_and_renders_png() {
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::assets(post))).unwrap();
    let answer = block_on(thread.call(|(engine, _worker): &mut (NativeEngine, EngineWorker)| {
        let (tx, rx) = oneshot::channel::<(Value, Result<Value, String>)>();
        let input = json!({ "kind": "bytes", "id": "t", "bytes": Bytes(stamps()).to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let list = list.as_ref().unwrap().clone();
                let pon = list["pages"][0]["ref"]["objectNumber"].as_u64().unwrap() as u32;
                let options =
                    json!({ "viewport": { "kind": "width", "width": 64 }, "format": "png" });
                doc.call("page.render.image", Some(pon), vec![options]).on_settle(move |image| {
                    let image = image.clone().map_err(|e| e.to_string());
                    let _ = tx.send((list, image));
                });
            });
        });
        rx
    }))
    .unwrap();
    let (list, image) = block_on(answer).unwrap();
    let named = list["namedPages"].as_array().unwrap();
    assert!(!named.is_empty(), "the shipped library has a registry");
    assert!(named[0]["target"]["page"]["objectNumber"].is_u64(), "{}", named[0]);
    let image = image.unwrap();
    assert_eq!(image["contentType"], json!("image/png"));
    assert_eq!(image["width"], json!(64));
    let png = Bytes::from_json(&image["source"]["bytes"]).unwrap().0;
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn the_document_engine_still_answers_raw_pixels() {
    // Only the asset engine encodes: a viewer's render stays a raster.
    let worker = EngineWorker::spawn_with(Host::new);
    let thread =
        KernelThread::spawn(move |post| Ok(NativeEngine::new(worker, post, None))).unwrap();
    let answer = block_on(thread.call(|engine: &mut NativeEngine| {
        let (tx, rx) = oneshot::channel::<Value>();
        let input = json!({ "kind": "bytes", "bytes": Bytes(stamps()).to_json() });
        engine.open(input, None).on_settle(move |doc| {
            let doc = doc.as_ref().ok().cloned().unwrap();
            doc.clone().call("pages.list", None, vec![]).on_settle(move |list| {
                let pon =
                    list.as_ref().unwrap()["pages"][0]["ref"]["objectNumber"].as_u64().unwrap();
                let options = json!({ "viewport": { "kind": "width", "width": 64 } });
                doc.call("page.render.image", Some(pon as u32), vec![options]).on_settle(
                    move |image| {
                        let _ = tx.send(image.as_ref().unwrap().clone());
                    },
                );
            });
        });
        rx
    }))
    .unwrap();
    let image = block_on(answer).unwrap();
    assert_eq!(image["source"]["kind"], json!("pixels"));
}

#[test]
fn dropping_the_host_joins_the_asset_engine_thread() {
    let thread = KernelThread::spawn(|post| Ok(NativeEngine::assets(post))).unwrap();
    let witness =
        block_on(thread.call(|(_, worker): &mut (NativeEngine, EngineWorker)| worker.alive()))
            .unwrap();
    assert!(witness.upgrade().is_some());
    block_on(thread.call(|(_, worker): &mut (NativeEngine, EngineWorker)| worker.join())).unwrap();
    drop(thread);
    assert!(witness.upgrade().is_none());
}
