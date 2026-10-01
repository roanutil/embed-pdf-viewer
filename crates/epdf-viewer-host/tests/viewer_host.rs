use std::sync::{Arc, Mutex};

use epdf_ffi::ViewerError;
use epdf_viewer_host::{
    DocumentEvent, DocumentEventListener, OpenDocumentOptions, ViewerConfig, ViewerDelegate,
    ViewerHost,
};
use futures::executor::block_on;

#[derive(Default)]
struct Delegate;
impl ViewerDelegate for Delegate {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, _error: ViewerError) {}
}

#[derive(Default)]
struct Events(Mutex<Vec<String>>);
impl DocumentEventListener for Events {
    fn on_event(&self, event: DocumentEvent) {
        self.0.lock().unwrap().push(format!("{event:?}"));
    }
}

fn options() -> OpenDocumentOptions {
    OpenDocumentOptions { name: None, password: None, activate: true }
}

fn started() -> Arc<ViewerHost> {
    let host = ViewerHost::new(ViewerConfig::default(), Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_document_opens_and_lists_its_pages() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert_eq!(doc.page_count, 14);
    assert_eq!(block_on(host.pages(doc.id)).unwrap().len(), 14);
}

#[test]
fn a_locked_document_reports_that_it_needs_a_password() {
    let host = started();
    let error =
        block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
            .unwrap_err();
    assert!(format!("{error}").contains("DocPasswordRequired"), "{error}");
}

#[test]
fn the_password_reaches_the_engine() {
    let host = started();
    let options = OpenDocumentOptions { password: Some("hôtel".into()), ..options() };
    let doc = block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options));
    let doc = doc.unwrap();
    assert!(doc.page_count > 0, "{doc:?}");
}

#[test]
fn opening_and_closing_are_observed() {
    let host = started();
    let events = Arc::new(Events::default());
    let _subscription = host.subscribe_documents(events.clone());
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    block_on(host.close_document(doc.id)).unwrap();
    let seen = events.0.lock().unwrap().join("\n");
    assert!(seen.contains("Opened"), "{seen}");
    assert!(seen.contains("Closed"), "{seen}");
}

#[test]
fn a_call_after_shutdown_is_closed() {
    let host = started();
    host.shutdown();
    assert!(matches!(block_on(host.documents()), Err(ViewerError::Closed)));
}

#[test]
fn dropping_the_host_joins_both_threads() {
    let host = started();
    let weak = Arc::downgrade(&host);
    drop(host);
    assert!(weak.upgrade().is_none());
    // KernelThread::drop joins the kernel thread; Session::drop shuts the
    // EngineWorker down, which ends the client thread and closes OwnerThread.
    // A hang here is the failure.
}
