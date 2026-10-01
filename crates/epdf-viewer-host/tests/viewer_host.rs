use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use epdf_ffi::{CancelToken, ResourceStatus, ViewerError};
use epdf_plugin_metadata::ffi::MetadataPatch;
use epdf_viewer_host::{
    DocumentEvent, DocumentEventListener, OpenDocumentOptions, ViewerConfig, ViewerDelegate,
    ViewerHost,
};
use futures::FutureExt;
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

/// Holds the host and lets it go from the kernel thread, once the test has
/// let go of its own reference.
struct Releasing {
    host: Mutex<Option<Arc<ViewerHost>>>,
    go: Mutex<mpsc::Receiver<()>>,
    released: Mutex<Option<mpsc::Sender<()>>>,
}
impl ViewerDelegate for Releasing {
    fn state_changed(&self, _revision: u64) {
        let Some(host) = self.host.lock().unwrap().take() else { return };
        let _ = self.go.lock().unwrap().recv_timeout(Duration::from_secs(5));
        drop(host);
        if let Some(tx) = self.released.lock().unwrap().take() {
            let _ = tx.send(());
        }
    }
    fn report(&self, _error: ViewerError) {}
}

#[test]
fn a_delegate_that_drops_the_last_host_on_the_kernel_thread_does_not_deadlock() {
    let (go_tx, go) = mpsc::channel();
    let (released_tx, released) = mpsc::channel();
    let delegate = Arc::new(Releasing {
        host: Mutex::new(None),
        go: Mutex::new(go),
        released: Mutex::new(Some(released_tx)),
    });
    let host = ViewerHost::new(ViewerConfig::default(), delegate.clone()).unwrap();
    let weak = Arc::downgrade(&host);
    *delegate.host.lock().unwrap() = Some(host.clone());
    // One poll posts the open. Its first store change blocks the kernel
    // thread in `state_changed` until this test has dropped its reference.
    let opening = host.open_document(test_support::fixture(test_support::REPORT), options());
    assert!(opening.now_or_never().is_none());
    drop(host);
    go_tx.send(()).unwrap();
    released.recv_timeout(Duration::from_secs(5)).expect("the delegate panicked or hung");
    assert!(weak.upgrade().is_none());
    // The loop exits after that job and drops the session, which holds the
    // delegate.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Arc::strong_count(&delegate) > 1 {
        assert!(Instant::now() < deadline, "the kernel thread never dropped its session");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn with_metadata() -> Arc<ViewerHost> {
    let host = ViewerHost::new(ViewerConfig { metadata: true }, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_metadata_plugin_reads_and_updates_through_the_native_engine() {
    let host = with_metadata();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let metadata = block_on(host.metadata(doc.id)).unwrap();
    block_on(metadata.refresh(CancelToken::new())).unwrap();
    assert_eq!(block_on(metadata.status()).unwrap(), ResourceStatus::Ready);
    assert!(block_on(metadata.snapshot()).unwrap().is_some());
    let patch = MetadataPatch { title: Some("Renamed".into()), ..Default::default() };
    let updated = block_on(metadata.update(patch, CancelToken::new())).unwrap();
    assert_eq!(updated.title.as_deref(), Some("Renamed"));
    let snapshot = block_on(metadata.snapshot()).unwrap().unwrap();
    assert_eq!(snapshot.title.as_deref(), Some("Renamed"));
}

#[test]
fn without_its_flag_the_metadata_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.metadata(doc.id)).is_err());
}
