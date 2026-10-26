#![cfg(feature = "uniffi")]

//! The stamp handle over the test kernel. Its fake engine opens no library
//! bytes, so these pin the handle's own behaviour: listing, unknown ids,
//! arming errors, events and persistence over a fake store. The host tests
//! and the Swift tests cover import over real PDFium.
//!
//! Migrated by hand in Task 13.24; not compiled until `epdf-ffi` builds
//! again (Task 13.33).

use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, HostError, KernelThread, PageRef, ViewerError};
use epdf_plugin_stamp::ffi::{
    StampAddAssetInput, StampEvent, StampEventListener, StampHandle, StampLibraryStore,
    StoredStampLibrary,
};
use epdf_plugin_stamp::{STAMP, StampConfig, stamp_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, TestSpec, one_document};
use serde_json::json;

#[derive(Default)]
struct Events(Mutex<Vec<StampEvent>>);

impl StampEventListener for Events {
    fn on_event(&self, event: StampEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct Store(Mutex<Vec<StoredStampLibrary>>);

impl StampLibraryStore for Store {
    fn list(&self) -> Result<Vec<StoredStampLibrary>, HostError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn put(&self, id: String, bytes: Vec<u8>) -> Result<(), HostError> {
        let mut rows = self.0.lock().unwrap();
        rows.retain(|r| r.id != id);
        rows.push(StoredStampLibrary { id, bytes });
        Ok(())
    }
    fn delete(&self, id: String) -> Result<(), HostError> {
        self.0.lock().unwrap().retain(|r| r.id != id);
        Ok(())
    }
}

/// A store whose `list` throws, as a Swift store reading a bad directory would.
struct BrokenStore;

impl StampLibraryStore for BrokenStore {
    fn list(&self) -> Result<Vec<StoredStampLibrary>, HostError> {
        Err(HostError::Failed { detail: "no disk".into() })
    }
    fn put(&self, _: String, _: Vec<u8>) -> Result<(), HostError> {
        Ok(())
    }
    fn delete(&self, _: String) -> Result<(), HostError> {
        Ok(())
    }
}

fn handle() -> (KernelThread<TestKernel>, Arc<StampHandle>, String) {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![stamp_plugin(StampConfig::default())], one_document())
    })
    .unwrap();
    let post = thread.post();
    let (handle, document_id) = block_on(thread.call(move |k| {
        let cap = k.kernel.capability(&STAMP, None).unwrap();
        (StampHandle::new(cap, post), k.document_id.clone())
    }))
    .unwrap();
    (thread, handle, document_id)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn a_fresh_workspace_has_no_libraries() {
    let (_thread, stamp, _) = handle();
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
    assert!(block_on(stamp.assets(None)).unwrap().is_empty());
    assert!(block_on(stamp.can_import()).unwrap());
}

#[test]
fn unknown_ids_read_as_none() {
    let (_thread, stamp, _) = handle();
    assert!(block_on(stamp.library("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.asset("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.asset_preview("nope".into())).unwrap().is_none());
    assert!(block_on(stamp.read_asset_bytes("nope".into())).unwrap().is_none());
}

#[test]
fn arming_an_unknown_asset_is_refused() {
    let (_thread, stamp, document_id) = handle();
    let error =
        block_on(stamp.arm_asset(document_id.clone(), "nope".into(), None, CancelToken::new()))
            .unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
    assert!(block_on(stamp.armed_asset(document_id)).unwrap().is_none());
}

#[test]
fn importing_non_pdf_bytes_is_refused() {
    let (_thread, stamp, _) = handle();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let error =
        block_on(stamp.import_library(png, Default::default(), CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
}

#[test]
fn an_unconfigured_library_write_is_refused_before_open() {
    let (_thread, stamp, _) = handle();
    // No asset engine is configured and the test kernel's engine reports no
    // local brand (`Engine::is_local`), so creating a library's blank PDF is
    // refused before any open; nothing is created.
    let error =
        block_on(stamp.create_library("Mine".into(), Default::default(), CancelToken::new()))
            .unwrap_err();
    assert_eq!(code(&error), Some("unsupported"), "{error:?}");
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
}

#[test]
fn an_engine_that_cannot_export_appearances_answers_unsupported() {
    // The fake engine lists every `page.annotations.*` member, so it stands
    // in for one that has none by refusing the export; the host test
    // `create_asset_from_annotations_is_unsupported_natively` covers the
    // native engine, where the member is absent.
    let spec = json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "services": [],
        "pages": [{ "index": 0, "ref": { "kind": "objectNumber", "objectNumber": 1 },
            "label": null, "size": { "width": 600, "height": 800 }, "rotation": 0 }] } } });
    let thread = KernelThread::spawn(move |_| {
        let spec = TestSpec::new(spec).respond_page("page.annotations.exportAppearance", |_, _| {
            Err(epdf_kernel::Error::engine("NotImplemented", "no appearance export"))
        });
        TestKernel::open(vec![stamp_plugin(StampConfig::default())], spec)
    })
    .unwrap();
    let post = thread.post();
    let (stamp, document_id) = block_on(thread.call(move |k| {
        (StampHandle::new(k.kernel.capability(&STAMP, None).unwrap(), post), k.document_id.clone())
    }))
    .unwrap();
    let error = block_on(stamp.create_asset_from_annotations(
        document_id,
        PageRef { page_object_number: 1 },
        Vec::new(),
        StampAddAssetInput::default(),
        CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(code(&error), Some("unsupported"), "{error:?}");
}

#[test]
fn a_cancelled_import_answers_operation_cancelled() {
    let (_thread, stamp, _) = handle();
    let token = CancelToken::new();
    token.cancel();
    let error =
        block_on(stamp.import_library(b"%PDF".to_vec(), Default::default(), token)).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
}

#[test]
fn restore_reads_nothing_from_an_empty_store() {
    let (_thread, stamp, _) = handle();
    let restored = block_on(stamp.restore(Arc::new(Store::default()), CancelToken::new())).unwrap();
    assert!(restored.is_empty());
}

#[test]
fn a_store_that_throws_fails_the_restore_with_its_error() {
    let (_thread, stamp, _) = handle();
    let error = block_on(stamp.restore(Arc::new(BrokenStore), CancelToken::new())).unwrap_err();
    match error {
        ViewerError::Kernel { name, code, detail } => {
            assert_eq!((name.as_str(), code.as_deref()), ("HostError", Some("operation-failed")));
            assert!(detail.contains("no disk"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn persist_returns_a_registration_that_unregisters_once() {
    let (thread, stamp, _) = handle();
    let registration = block_on(stamp.persist(Arc::new(Store::default()), Vec::new())).unwrap();
    registration.unregister();
    registration.unregister();
    block_on(thread.call(|_| ())).unwrap();
}

#[test]
fn subscribe_and_cancel_do_not_leak() {
    let (thread, stamp, _) = handle();
    let events = Arc::new(Events::default());
    let subscription = stamp.subscribe(events.clone());
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert!(events.0.lock().unwrap().is_empty());
    assert_eq!(Arc::strong_count(&events), 1, "the listener was released");
}
