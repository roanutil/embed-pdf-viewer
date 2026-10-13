#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{KernelThread, ViewerError};
use epdf_kernel::{OpenOptions, OpenSource};
use epdf_plugin_view_manager::ffi::{
    ViewManagerEvent, ViewManagerEventListener, ViewManagerHandle,
};
use epdf_plugin_view_manager::{DocumentMovedEvent, VIEW_MANAGER, view_manager_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, settle};
use serde_json::json;

#[derive(Default)]
struct Events(Mutex<Vec<ViewManagerEvent>>, AtomicUsize);

impl ViewManagerEventListener for Events {
    fn on_event(&self, event: ViewManagerEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

/// Two documents in the engine, `a` and `b`. `TestKernel` opens `a`; the
/// test opens `b` itself with `open_b`.
fn spec() -> serde_json::Value {
    json!({ "documents": {
        "a": { "id": "a", "security": { "allows": "*" }, "pages": [] },
        "b": { "id": "b", "security": { "allows": "*" }, "pages": [] }
    } })
}

fn handle() -> (KernelThread<TestKernel>, Arc<ViewManagerHandle>) {
    let thread =
        KernelThread::spawn(|_| TestKernel::open(vec![view_manager_plugin()], spec())).unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| {
        ViewManagerHandle::new(k.kernel.capability(&VIEW_MANAGER, None).unwrap(), post)
    }))
    .unwrap();
    (thread, handle)
}

fn document_a(thread: &KernelThread<TestKernel>) -> String {
    block_on(thread.call(|k| k.document_id.clone())).unwrap()
}

fn open_b(thread: &KernelThread<TestKernel>) -> String {
    block_on(thread.call(|k| {
        let opened = k.kernel.documents().open(
            OpenSource::Input(json!({ "kind": "bytes", "id": "b", "bytes": { "$bytes": "" } })),
            OpenOptions::default(),
        );
        settle(&opened).unwrap().as_str().unwrap().to_string()
    }))
    .unwrap()
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn the_open_document_lands_in_the_default_pane() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].id, "pane-1");
    assert_eq!(panes[0].document_ids, [a.clone()]);
    assert_eq!(panes[0].active_document_id.as_deref(), Some(a.as_str()));
    assert_eq!(block_on(vm.focused_pane_id()).unwrap().as_deref(), Some("pane-1"));
    assert_eq!(block_on(vm.pane_of_document(a)).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn splitting_moves_the_document_and_says_so_in_order() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    let pane = block_on(vm.split_pane(b.clone())).unwrap();
    assert_eq!(pane, "pane-2");
    assert_eq!(block_on(vm.pane_order()).unwrap(), ["pane-1", "pane-2"]);
    assert_eq!(block_on(vm.pane(pane.clone())).unwrap().unwrap().document_ids, [b.clone()]);
    assert_eq!(block_on(vm.pane("pane-1".into())).unwrap().unwrap().document_ids, [a]);
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(&seen[0], ViewManagerEvent::PaneCreated { event } if event.pane_id == "pane-2")
    );
    assert!(
        matches!(&seen[1], ViewManagerEvent::FocusChanged { event } if event.pane_id.as_deref() == Some("pane-2"))
    );
    assert!(
        matches!(&seen[2], ViewManagerEvent::DocumentMoved { event } if *event == DocumentMovedEvent {
            document_id: b.clone(),
            from_pane_id: Some("pane-1".into()),
            to_pane_id: Some("pane-2".into()),
        }),
        "{seen:?}"
    );
}

#[test]
fn removing_a_pane_hands_its_documents_on() {
    let (thread, vm) = handle();
    let b = open_b(&thread);
    let pane = block_on(vm.split_pane(b.clone())).unwrap();
    block_on(vm.remove_pane(pane, Some("pane-1".into()))).unwrap();
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert!(panes[0].document_ids.contains(&b));
}

#[test]
fn an_index_past_the_end_appends() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    let second = block_on(vm.create_pane(vec![])).unwrap();
    block_on(vm.move_pane("pane-1".into(), u32::MAX)).unwrap();
    assert_eq!(block_on(vm.pane_order()).unwrap(), [second, "pane-1".to_string()]);
    block_on(vm.move_document_within("pane-1".into(), a.clone(), u32::MAX)).unwrap();
    assert_eq!(block_on(vm.pane("pane-1".into())).unwrap().unwrap().document_ids, [b, a]);
}

#[test]
fn unknown_ids_are_ignored_not_errors() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let before = block_on(vm.panes()).unwrap();
    block_on(vm.remove_pane("pane-9".into(), None)).unwrap();
    block_on(vm.move_pane("pane-9".into(), 0)).unwrap();
    block_on(vm.add_document("pane-9".into(), a.clone(), None)).unwrap();
    block_on(vm.set_active_document("pane-1".into(), Some("nope".into()))).unwrap();
    block_on(vm.remove_document("pane-1".into(), "nope".into())).unwrap();
    assert_eq!(block_on(vm.pane("pane-9".into())).unwrap(), None);
    assert_eq!(block_on(vm.pane_of_document("nope".into())).unwrap(), None);
    assert_eq!(block_on(vm.panes()).unwrap(), before);
}

#[test]
fn a_tab_dragged_between_panes_becomes_active_there() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let second = block_on(vm.create_pane(vec![])).unwrap();
    block_on(vm.move_document_between("pane-1".into(), second.clone(), a.clone(), Some(0)))
        .unwrap();
    let pane = block_on(vm.pane(second.clone())).unwrap().unwrap();
    assert_eq!(pane.document_ids, [a.clone()]);
    assert_eq!(pane.active_document_id, Some(a));
    assert_eq!(block_on(vm.focused_pane_id()).unwrap(), Some(second));
}

#[test]
fn closing_a_document_takes_it_out_of_its_pane() {
    let (thread, vm) = handle();
    let a = document_a(&thread);
    let b = open_b(&thread);
    block_on(vm.set_active_document("pane-1".into(), Some(a.clone()))).unwrap();
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let pane = block_on(vm.pane("pane-1".into())).unwrap().unwrap();
    assert_eq!(pane.document_ids, [b.clone()]);
    assert_eq!(pane.active_document_id, Some(b));
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter().any(|e| matches!(e, ViewManagerEvent::DocumentMoved { event }
            if event.document_id == a && event.to_pane_id.is_none())),
        "{seen:?}"
    );
    assert_eq!(
        events.1.load(Ordering::SeqCst),
        0,
        "a document close doesn't end a workspace stream"
    );
}

#[test]
fn a_destroyed_kernels_capability_refuses_with_instance_closed() {
    let (thread, vm) = handle();
    let events = Arc::new(Events::default());
    let _sub = vm.subscribe(events.clone());
    block_on(thread.call(|k| settle(&k.kernel.destroy()))).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    for error in [
        block_on(vm.panes()).map(|_| ()).unwrap_err(),
        block_on(vm.create_pane(vec![])).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
    // A stream made after the destroy ends at once.
    let late = Arc::new(Events::default());
    let _late = vm.subscribe(late.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(late.1.load(Ordering::SeqCst), 1);
}

#[test]
fn a_cancelled_subscription_hears_nothing_more() {
    let (thread, vm) = handle();
    let events = Arc::new(Events::default());
    let subscription = vm.subscribe(events.clone());
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    block_on(vm.create_pane(vec![])).unwrap();
    assert!(events.0.lock().unwrap().is_empty());
}
