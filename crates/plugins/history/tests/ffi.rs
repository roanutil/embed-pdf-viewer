#![cfg(feature = "uniffi")]

use epdf_ffi::{KernelThread, ViewerError};
use epdf_plugin_history::ffi::{
    HistoryEvent, HistoryEventListener, HistoryHandle, HistorySettingsPatch,
};
use epdf_plugin_history::{HISTORY, history_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, TestSpec, one_document};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Events(Mutex<Vec<HistoryEvent>>, Mutex<usize>);
impl HistoryEventListener for Events {
    fn on_event(&self, event: HistoryEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        *self.1.lock().unwrap() += 1;
    }
}

#[test]
fn settings_empty_submissions_and_close_follow_the_document_lifetime() {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![history_plugin(Default::default())], TestSpec::new(one_document()))
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |kernel| HistoryHandle::new(kernel.capability(&HISTORY).unwrap(), post)),
    )
    .unwrap();
    block_on(async {
        assert!(!handle.can_undo().await.unwrap());
        assert!(!handle.can_redo().await.unwrap());
        assert!(handle.undo_label().await.unwrap().is_none());
        assert!(handle.redo_label().await.unwrap().is_none());
        let events = Arc::new(Events::default());
        let _subscription = handle.subscribe(events.clone());
        handle.undo().await.unwrap();
        handle.redo().await.unwrap();
        assert!(events.0.lock().unwrap().is_empty());
        assert_eq!(handle.settings().await.unwrap().limit, 100.0);
        handle.update_settings(HistorySettingsPatch { limit: Some(5.0) }).await.unwrap();
        assert_eq!(handle.settings().await.unwrap().limit, 5.0);
        assert_eq!(events.0.lock().unwrap().len(), 1);
        handle.reset_settings().await.unwrap();
        assert_eq!(handle.settings().await.unwrap().limit, 100.0);
        handle.clear().await.unwrap();
    });
    drop(thread);
    assert!(matches!(block_on(handle.can_undo()), Err(ViewerError::Closed)));
}

#[test]
fn close_ends_existing_and_late_listeners_and_labels_keep_value_kinds() {
    use epdf_plugin_history::ffi::{HistoryChangeLabel, HistoryLabelValue};
    use serde_json::json;
    let label =
        HistoryChangeLabel::from_json(&json!({"key":"pages.rotate", "count":2, "title":"Résumé"}))
            .unwrap();
    assert_eq!(label.values["count"], HistoryLabelValue::Number { value: 2.0 });
    assert_eq!(label.values["title"], HistoryLabelValue::Text { value: "Résumé".into() });
    assert!(HistoryChangeLabel::from_json(&json!({"key":"bad", "count":null})).is_err());
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(vec![history_plugin(Default::default())], TestSpec::new(one_document()))
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |kernel| HistoryHandle::new(kernel.capability(&HISTORY).unwrap(), post)),
    )
    .unwrap();
    let events = Arc::new(Events::default());
    let _subscription = handle.subscribe(events.clone());
    block_on(thread.call(|kernel| kernel.close())).unwrap().unwrap();
    assert_eq!(*events.1.lock().unwrap(), 1);
    let late = Arc::new(Events::default());
    let _late_subscription = handle.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(*late.1.lock().unwrap(), 1);
}

#[test]
fn skipped_and_refused_confirmations_preserve_label_and_error() {
    use epdf_kernel::changes::{ChangeQueue, StagedChange};
    use epdf_kernel::{Error, Token, define};
    use plugin_conformance::test_kernel::Reply;
    use serde_json::json;
    use std::sync::LazyLock;
    struct Confirmations(Mutex<std::sync::mpsc::Sender<HistoryEvent>>);
    impl HistoryEventListener for Confirmations {
        fn on_event(&self, event: HistoryEvent) {
            self.0.lock().unwrap().send(event).ok();
        }
        fn on_end(&self) {}
    }
    static CHANGES: LazyLock<Token<ChangeQueue>> = LazyLock::new(|| Token::new("test-changes"));
    for refuse in [false, true] {
        let thread = KernelThread::spawn(move |_| {
            let feature = define::<serde_json::Value, serde_json::Value>("test-changes").document_scoped().create(&CHANGES, |cx| Ok((cx.changes()?, None))).build();
            let spec = TestSpec::new(one_document()).respond("apply", move |args| {
                if args[0].get("undoOf").is_some() && refuse {
                    return Err(Error::named("EngineError", "test refusal").with("code", "Forbidden"));
                }
                Ok(Reply::value(json!({"items": if args[0].get("undoOf").is_some() { vec![json!({"type":"skipped"})] } else { vec![] }, "meta":{"undoable":true}})))
            });
            TestKernel::open(vec![history_plugin(Default::default()), feature], spec)
        }).unwrap();
        let post = thread.post();
        let handle =
            block_on(thread.call(move |kernel| {
                HistoryHandle::new(kernel.capability(&HISTORY).unwrap(), post)
            }))
            .unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let _subscription = handle.subscribe(Arc::new(Confirmations(Mutex::new(sender))));
        block_on(thread.call(|kernel| {
            let queue = kernel.capability(&CHANGES).unwrap();
            let mut change = StagedChange::ops(
                json!({"key":"test.edit","count":2}),
                vec![json!({"type":"test"})],
            );
            change.undo = change.ops.clone();
            queue.stage(change).unwrap();
        }))
        .unwrap();
        block_on(handle.undo()).unwrap();
        let event = receiver.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
        if refuse {
            assert!(
                matches!(&event, HistoryEvent::UndoFailed { error, .. } if error.code == "permission-denied"),
                "{event:?}"
            );
        } else {
            assert!(matches!(event, HistoryEvent::Undone { skipped: 1, redo: false, .. }));
        }
    }
}
