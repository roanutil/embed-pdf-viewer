mod common;
trait ErrorCode {
    fn code(&self) -> Option<&str>;
}
impl ErrorCode for epdf_ffi::ViewerError {
    fn code(&self) -> Option<&str> {
        match self {
            Self::Kernel { code, .. } => code.as_deref(),
            Self::Closed => Some("instance-closed"),
        }
    }
}

use epdf_ffi::{CancelToken, HostFailure};
use epdf_viewer_host::{
    DocumentFetchCancellation, DocumentFetchCompletion, DocumentFetcher, DocumentStatus,
    ViewerConfig, ViewerHost,
};
use futures::executor::block_on;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

type Request = (String, Arc<DocumentFetchCompletion>, Arc<Cancellation>);
#[derive(Default)]
struct Cancellation(AtomicUsize);
impl DocumentFetchCancellation for Cancellation {
    fn cancel(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
struct Fetcher {
    requests: Mutex<mpsc::Sender<Request>>,
    count: AtomicUsize,
}
impl DocumentFetcher for Fetcher {
    fn fetch(
        &self,
        url: String,
        completion: Arc<DocumentFetchCompletion>,
    ) -> Arc<dyn DocumentFetchCancellation> {
        let cancel = Arc::new(Cancellation::default());
        self.count.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().send((url, completion, cancel.clone())).unwrap();
        cancel
    }
}
fn started() -> (Arc<ViewerHost>, Arc<Fetcher>, mpsc::Receiver<Request>) {
    let (send, receive) = mpsc::channel();
    let fetch = Arc::new(Fetcher { requests: Mutex::new(send), count: AtomicUsize::new(0) });
    let host = ViewerHost::new_with_fetcher(
        ViewerConfig::default(),
        Arc::new(common::NullDelegate),
        fetch.clone(),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    (host, fetch, receive)
}
fn request(receive: &mpsc::Receiver<Request>) -> Request {
    receive.recv_timeout(Duration::from_secs(5)).unwrap()
}
fn open(
    host: &Arc<ViewerHost>,
    cancel: Arc<CancelToken>,
) -> std::thread::JoinHandle<Result<epdf_viewer_host::DocumentInfo, epdf_ffi::ViewerError>> {
    let host = host.clone();
    std::thread::spawn(move || {
        block_on(host.open_url("https://example.test/report.pdf".into(), common::options(), cancel))
    })
}

#[test]
fn failed_http_and_non_pdf_responses_remain_retryable_tabs_and_refetch() {
    for (status, body, code) in [
        (404, b"missing".to_vec(), "not-found"),
        (403, vec![], "permission-denied"),
        (200, b"<html>login</html>".to_vec(), "invalid-input"),
    ] {
        let (host, fetch, receive) = started();
        let opening = open(&host, CancelToken::new());
        let (url, completion, _) = request(&receive);
        assert_eq!(url, "https://example.test/report.pdf");
        let loading = block_on(host.documents()).unwrap().remove(0);
        assert_eq!(loading.status, DocumentStatus::Loading);
        completion.finish(status, body);
        assert_eq!(opening.join().unwrap().unwrap_err().code(), Some(code));
        let failed = block_on(host.document(Some(loading.id.clone()))).unwrap().unwrap();
        assert_eq!(failed.status, DocumentStatus::Failed);
        assert_eq!(failed.error.unwrap().code, code);
        assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
        let other = block_on(
            host.open_document(test_support::fixture(test_support::REPORT), common::options()),
        )
        .unwrap();
        let retry_host = host.clone();
        let id = loading.id.clone();
        let retry = std::thread::spawn(move || block_on(retry_host.retry_document(id)));
        let (_, completion, _) = request(&receive);
        assert_eq!(
            block_on(host.document(Some(loading.id.clone()))).unwrap().unwrap().status,
            DocumentStatus::Loading
        );
        completion.finish(200, test_support::fixture(test_support::REPORT));
        let retried = retry.join().unwrap().unwrap();
        assert_eq!(retried.status, DocumentStatus::Ready);
        assert_eq!(retried.page_count, 14);
        assert_eq!(block_on(host.active_document_id()).unwrap(), Some(other.id.clone()));
        assert_eq!(block_on(host.document_order()).unwrap(), vec![retried.id, other.id]);
        assert_eq!(fetch.count.load(Ordering::SeqCst), 2);
        assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
    }
}

#[test]
fn cancelling_open_or_closing_loading_aborts_transport_and_removes_tab() {
    for close in [false, true] {
        let (host, fetch, receive) = started();
        let token = CancelToken::new();
        let opening = open(&host, token.clone());
        let (_, completion, cancellation) = request(&receive);
        let loading = block_on(host.documents()).unwrap().remove(0);
        if close {
            block_on(host.close_document(loading.id)).unwrap();
        } else {
            token.cancel();
        }
        assert_eq!(opening.join().unwrap().unwrap_err().code(), Some("operation-cancelled"));
        assert!(block_on(host.documents()).unwrap().is_empty());
        assert_eq!(cancellation.0.load(Ordering::SeqCst), 1);
        // A late transport answer cannot recreate the closed document.
        completion.finish(200, test_support::fixture(test_support::REPORT));
        assert!(block_on(host.documents()).unwrap().is_empty());
        assert_eq!(fetch.count.load(Ordering::SeqCst), 1);
        assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
    }
}

#[test]
fn cancelled_before_start_never_fetches_and_foreign_failures_keep_their_code() {
    let (host, fetch, receive) = started();
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(open(&host, token).join().unwrap().unwrap_err().code(), Some("operation-cancelled"));
    assert_eq!(fetch.count.load(Ordering::SeqCst), 0);
    let opening = open(&host, CancelToken::new());
    let (_, completion, _) = request(&receive);
    completion.fail_with(HostFailure {
        name: "Offline".into(),
        code: None,
        detail: "connection lost".into(),
    });
    assert_eq!(opening.join().unwrap().unwrap_err().code(), Some("operation-failed"));
    let tab = block_on(host.documents()).unwrap().remove(0);
    assert_eq!(tab.status, DocumentStatus::Failed);
    assert_eq!(tab.error.unwrap().message, "connection lost");
    assert_eq!(
        block_on(host.retry_document("missing".into())).unwrap_err().code(),
        Some("invalid-input")
    );
}

#[test]
fn closing_a_delayed_retry_aborts_the_second_fetch_and_removes_its_tab() {
    let (host, fetch, receive) = started();
    let opening = open(&host, CancelToken::new());
    let (_, completion, _) = request(&receive);
    completion.finish(404, vec![]);
    assert_eq!(opening.join().unwrap().unwrap_err().code(), Some("not-found"));
    let tab = block_on(host.documents()).unwrap().remove(0);
    let retry_host = host.clone();
    let id = tab.id.clone();
    let retry = std::thread::spawn(move || block_on(retry_host.retry_document(id)));
    let (_, completion, cancellation) = request(&receive);
    assert_eq!(
        block_on(host.document(Some(tab.id.clone()))).unwrap().unwrap().status,
        DocumentStatus::Loading
    );
    block_on(host.close_document(tab.id)).unwrap();
    assert_eq!(retry.join().unwrap().unwrap_err().code(), Some("operation-cancelled"));
    assert_eq!(cancellation.0.load(Ordering::SeqCst), 1);
    completion.finish(200, test_support::fixture(test_support::REPORT));
    assert!(block_on(host.documents()).unwrap().is_empty());
    assert_eq!(fetch.count.load(Ordering::SeqCst), 2);
}

struct DroppingFetcher;
impl DocumentFetcher for DroppingFetcher {
    fn fetch(
        &self,
        _url: String,
        _completion: Arc<DocumentFetchCompletion>,
    ) -> Arc<dyn DocumentFetchCancellation> {
        Arc::new(Cancellation::default())
    }
}

#[test]
fn dropping_an_unanswered_completion_fails_the_tab_instead_of_hanging() {
    let host = ViewerHost::new_with_fetcher(
        ViewerConfig::default(),
        Arc::new(common::NullDelegate),
        Arc::new(DroppingFetcher),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let error = block_on(host.open_url(
        "https://example.test/dropped.pdf".into(),
        common::options(),
        CancelToken::new(),
    ))
    .unwrap_err();
    assert_eq!(error.code(), Some("operation-failed"));
    assert_eq!(block_on(host.documents()).unwrap().remove(0).status, DocumentStatus::Failed);
    block_on(host.close_all_documents()).unwrap();
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 0);
}
