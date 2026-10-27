//! The uniffi handle over the current plugin, migrated by hand in Task
//! 13.26.
#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, ResourceStatus, ViewerError};
use epdf_plugin_metadata::ffi::{
    DocumentMetadataTrapped, MetadataEvent, MetadataEventListener, MetadataField, MetadataHandle,
    MetadataPatch,
};
use epdf_plugin_metadata::{METADATA, metadata_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec, one_document};
use serde_json::{Map, Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<MetadataEvent>>, AtomicUsize);

impl MetadataEventListener for Events {
    fn on_event(&self, event: MetadataEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle_over(
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<MetadataHandle>) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![metadata_plugin()], spec())).unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| MetadataHandle::new(k.capability(&METADATA).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

/// The engine's current metadata wire, as `doc.metadata` answers it: the
/// standard fields and the custom keys are read and changed apart, and each
/// write publishes its confirmed event (with an origin) before it answers.
fn spec_with(title: &str, custom: Value) -> TestSpec {
    let mut fields = Map::new();
    for key in
        ["title", "author", "subject", "keywords", "producer", "creator", "createdAt", "modifiedAt"]
    {
        fields.insert(key.into(), Value::Null);
    }
    fields.insert("title".into(), json!(title));
    fields.insert("trapped".into(), json!("unknown"));
    let origin =
        json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null });
    let standard = Rc::new(RefCell::new(fields));
    let keys = Rc::new(RefCell::new(custom.as_object().cloned().unwrap_or_default()));
    let (read, read_custom) = (standard.clone(), keys.clone());
    let custom_origin = origin.clone();
    TestSpec::new(one_document())
        .respond("metadata.get", move |_| Ok(Reply::value(Value::Object(read.borrow().clone()))))
        .respond("metadata.custom.get", move |_| {
            Ok(Reply::value(Value::Object(read_custom.borrow().clone())))
        })
        .respond("metadata.update", move |args| {
            let mut fields = standard.borrow_mut();
            for (key, value) in args[0].as_object().into_iter().flatten() {
                fields.insert(key.clone(), value.clone());
            }
            let metadata = Value::Object(fields.clone());
            Ok(Reply {
                value: json!({ "metadata": metadata }),
                events: vec![
                    json!({ "type": "metadata.updated", "metadata": metadata, "origin": origin }),
                ],
            })
        })
        .respond("metadata.custom.update", move |args| {
            let mut custom = keys.borrow_mut();
            for (key, value) in args[0].as_object().into_iter().flatten() {
                match value {
                    Value::Null => custom.remove(key),
                    value => custom.insert(key.clone(), value.clone()),
                };
            }
            let custom = Value::Object(custom.clone());
            Ok(Reply {
                value: json!({ "custom": custom }),
                events: vec![json!({
                    "type": "metadata.customUpdated",
                    "custom": custom,
                    "origin": custom_origin,
                })],
            })
        })
}

fn handle() -> (KernelThread<TestKernel>, Arc<MetadataHandle>) {
    handle_over(|| spec_with("Report", json!({ "Dept": "Ops" })))
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

#[test]
fn ensure_loaded_then_snapshot_reads_the_metadata() {
    let (_thread, metadata) = handle();
    // Nothing is read until someone asks.
    assert_eq!(block_on(metadata.status()).unwrap(), ResourceStatus::Idle);
    assert_eq!(block_on(metadata.snapshot()).unwrap(), None);
    block_on(metadata.ensure_loaded(token())).unwrap();
    assert_eq!(block_on(metadata.status()).unwrap(), ResourceStatus::Ready);
    assert_eq!(block_on(metadata.custom_status()).unwrap(), ResourceStatus::Ready);
    let snapshot = block_on(metadata.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.title.as_deref(), Some("Report"));
    assert_eq!(snapshot.author, None);
    assert_eq!(snapshot.created_at, None);
    assert_eq!(snapshot.trapped, DocumentMetadataTrapped::Unknown);
    let custom = block_on(metadata.custom_snapshot()).unwrap().unwrap();
    assert_eq!(custom.get("Dept").map(String::as_str), Some("Ops"));
    assert!(block_on(metadata.can_update()).unwrap());
    // A refresh re-reads the standard fields.
    block_on(metadata.refresh(token())).unwrap();
    block_on(metadata.refresh_custom(token())).unwrap();
    assert_eq!(block_on(metadata.status()).unwrap(), ResourceStatus::Ready);
}

#[test]
fn an_update_is_observed() {
    let (_thread, metadata) = handle();
    let events = Arc::new(Events::default());
    let _sub = metadata.subscribe(events.clone());
    block_on(metadata.ensure_loaded(token())).unwrap();
    let patch = MetadataPatch { title: Some("Renamed".into()), ..Default::default() };
    assert_eq!(block_on(metadata.update(patch, token())).unwrap().title, Some("Renamed".into()));
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(MetadataEvent::Updated { event }) if event.changed_keys == ["title"]
            && event.previous.as_ref().and_then(|p| p.title.as_deref()) == Some("Report")),
        "{seen:?}"
    );
}

#[test]
fn a_patch_clears_and_edits_custom_keys() {
    let (_thread, metadata) = handle();
    block_on(metadata.ensure_loaded(token())).unwrap();
    let patch = MetadataPatch { clear: vec![MetadataField::Title], ..Default::default() };
    let updated = block_on(metadata.update(patch, token())).unwrap();
    assert_eq!(updated.title, None);
    let custom = block_on(metadata.update_custom(
        [("Dept".to_string(), None), ("Owner".to_string(), Some("Ana".into()))].into(),
        token(),
    ))
    .unwrap();
    assert_eq!(custom, [("Owner".to_string(), "Ana".to_string())].into());
    let events = Arc::new(Events::default());
    let _sub = metadata.subscribe(events.clone());
    block_on(metadata.update_custom([("Owner".to_string(), Some(String::new()))].into(), token()))
        .unwrap();
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.last(), Some(MetadataEvent::CustomUpdated { event }) if event.changed_keys == ["Owner"]),
        "{seen:?}"
    );
}

#[test]
fn a_key_both_set_and_cleared_is_refused() {
    let (_thread, metadata) = handle();
    let patch = MetadataPatch {
        title: Some("Renamed".into()),
        clear: vec![MetadataField::Title],
        ..Default::default()
    };
    let error = block_on(metadata.update(patch, token())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn cancelling_an_update_before_it_runs_leaves_the_document_alone() {
    let (thread, metadata) = handle();
    block_on(metadata.ensure_loaded(token())).unwrap();
    // Hold the kernel thread so the update's job queues behind this one.
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let patch = MetadataPatch { title: Some("Renamed".into()), ..Default::default() };
    let update = metadata.update(patch, cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(update).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    let snapshot = block_on(metadata.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.title.as_deref(), Some("Report"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, metadata) = handle();
    block_on(metadata.ensure_loaded(token())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(metadata.snapshot()).map(|_| ()).unwrap_err(),
        block_on(metadata.status()).map(|_| ()).unwrap_err(),
        block_on(metadata.custom_snapshot()).map(|_| ()).unwrap_err(),
        block_on(metadata.can_update()).map(|_| ()).unwrap_err(),
        block_on(metadata.ensure_loaded(token())).unwrap_err(),
        block_on(metadata.refresh(token())).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn metadata_the_record_cannot_hold_is_an_error_not_a_default() {
    let (_thread, metadata) = handle_over(|| {
        TestSpec::new(one_document())
            .respond("metadata.get", |_| Ok(Reply::value(json!({ "title": 5 }))))
            .respond("metadata.custom.get", |_| Ok(Reply::value(json!({}))))
    });
    block_on(metadata.ensure_loaded(token())).unwrap();
    let error = block_on(metadata.snapshot()).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, metadata) = handle();
    let events = Arc::new(Events::default());
    let subscription = metadata.subscribe(events.clone());
    block_on(metadata.ensure_loaded(token())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 0);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn subscribing_after_the_close_ends_at_once() {
    let (thread, metadata) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let events = Arc::new(Events::default());
    let _subscription = metadata.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
