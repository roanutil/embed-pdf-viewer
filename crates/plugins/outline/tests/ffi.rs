#![cfg(feature = "uniffi")]
use epdf_plugin_outline::ffi::{
    OutlineItemPatch, OutlineItemRef, OutlinePatchKey, OutlinePosition,
};
use serde_json::json;
#[test]
fn positions_and_patch_clear_preserve_absence_and_identity() {
    let item = OutlineItemRef { object_number: 42 };
    assert_eq!(item.to_json(), json!({"kind":"objectNumber","objectNumber":42}));
    assert_eq!(
        OutlinePosition::Before { item: item.clone() }.to_json(),
        json!({"before":item.to_json()})
    );
    let patch = OutlineItemPatch::default();
    assert_eq!(patch.to_json(), json!({}));
    let patch = OutlineItemPatch {
        title: Some("Résumé".into()),
        clear: vec![OutlinePatchKey::Target, OutlinePatchKey::Color],
        ..Default::default()
    };
    assert_eq!(patch.to_json(), json!({"title":"Résumé","target":null,"color":null}));
}

#[test]
fn read_records_preserve_action_data_and_reject_unknown_identity() {
    use epdf_plugin_outline::ffi::{OutlineItem, OutlineItemInput};
    let action = json!({"root":{"type":"future-action","payload":{"unknown": [1,2]}},"incomplete":true,"warnings":["future"]});
    let wire = json!({"ref":{"kind":"objectNumber","objectNumber":7},"parent":null,"title":"Résumé","target":null,"action":action,"open":false,"color":null,"bold":true,"italic":false});
    let item = OutlineItem::from_json(&wire).unwrap();
    let preserved: serde_json::Value = serde_json::from_str(&item.action.unwrap().json()).unwrap();
    assert_eq!(preserved, action);
    assert_eq!(item.item_ref.object_number, 7);
    assert!(OutlineItemRef::from_json(&json!({"kind":"index","objectNumber":7})).is_err());
    assert!(OutlineItemInput::default().to_json().get("target").is_none());
    assert_eq!(
        OutlineItemInput { clear_target: true, ..Default::default() }.to_json()["target"],
        serde_json::Value::Null
    );
}

#[test]
fn pending_load_cancels_and_close_revokes_late_subscriptions() {
    use epdf_ffi::{CancelToken, KernelThread, ViewerError};
    use epdf_plugin_outline::ffi::{OutlineEvent, OutlineEventListener, OutlineHandle};
    use epdf_plugin_outline::{OUTLINE, outline_plugin};
    use futures::{FutureExt, executor::block_on};
    use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, one_document};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Listener(AtomicUsize);
    impl OutlineEventListener for Listener {
        fn on_event(&self, _: OutlineEvent) {}
        fn on_end(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let thread = KernelThread::spawn(|_| {
        let mut engine = one_document();
        engine["documents"][DOCUMENT]["responses"] = json!([{"path":"outline.get","defer":"load"}]);
        TestKernel::open(vec![outline_plugin()], engine)
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |kernel| OutlineHandle::new(kernel.capability(&OUTLINE).unwrap(), post)),
    )
    .unwrap();
    let token = CancelToken::new();
    let mut load = Box::pin(handle.ensure_loaded(token.clone()));
    assert!(load.as_mut().now_or_never().is_none());
    block_on(thread.call(|_| ())).unwrap();
    token.cancel();
    assert!(
        matches!(block_on(load), Err(ViewerError::Kernel { code: Some(code), .. }) if code=="operation-cancelled")
    );
    let listener = Arc::new(Listener(AtomicUsize::new(0)));
    let _subscription = handle.subscribe(listener.clone());
    block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
    assert_eq!(listener.0.load(Ordering::SeqCst), 1);
    let late = Arc::new(Listener(AtomicUsize::new(0)));
    let _late = handle.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(late.0.load(Ordering::SeqCst), 1);
}

#[test]
fn a_held_outline_write_rejects_cancellation_and_document_close() {
    use epdf_ffi::{CancelToken, KernelThread, ViewerError};
    use epdf_plugin_outline::ffi::OutlineHandle;
    use epdf_plugin_outline::{OUTLINE, outline_plugin};
    use futures::{FutureExt, executor::block_on};
    use plugin_conformance::test_kernel::{DOCUMENT, TestKernel, one_document};
    for close in [false, true] {
        let thread=KernelThread::spawn(|_| {
            let mut engine=one_document();
            let item=json!({"ref":{"kind":"objectNumber","objectNumber":10},"parent":null,"title":"Original","target":null,"action":null,"open":false,"color":null,"bold":false,"italic":false});
            engine["documents"][DOCUMENT]["responses"]=json!([{"path":"outline.get","value":{"items":[item],"incomplete":false}},{"path":"apply","defer":"write"}]);
            TestKernel::open(vec![outline_plugin()],engine)
        }).unwrap();
        let post = thread.post();
        let handle =
            block_on(thread.call(move |kernel| {
                OutlineHandle::new(kernel.capability(&OUTLINE).unwrap(), post)
            }))
            .unwrap();
        block_on(handle.ensure_loaded(CancelToken::new())).unwrap();
        let token = CancelToken::new();
        let mut write = Box::pin(handle.update(
            OutlineItemRef { object_number: 10 },
            OutlineItemPatch { title: Some("Pending".into()), ..Default::default() },
            token.clone(),
        ));
        assert!(write.as_mut().now_or_never().is_none());
        block_on(thread.call(|_| ())).unwrap();
        if close {
            block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
        } else {
            token.cancel();
        }
        assert!(
            matches!(block_on(write),Err(ViewerError::Kernel{code:Some(code),..}) if code==if close {"instance-closed"} else {"operation-cancelled"})
        );
    }
}
