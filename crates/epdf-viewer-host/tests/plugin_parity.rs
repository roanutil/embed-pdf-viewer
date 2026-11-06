mod common;
use common::{NullDelegate, options};
use epdf_ffi::ViewerError;
use epdf_plugin_history::ffi::{HistoryEvent, HistoryEventListener};
use epdf_viewer_host::{ViewerConfig, ViewerHost};
use futures::executor::block_on;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

struct HistoryEvents(Mutex<mpsc::Sender<HistoryEvent>>);
impl HistoryEventListener for HistoryEvents {
    fn on_event(&self, event: HistoryEvent) {
        self.0.lock().unwrap().send(event).ok();
    }
    fn on_end(&self) {}
}
fn opened(config: ViewerConfig) -> (Arc<ViewerHost>, String) {
    let host = ViewerHost::new(config, Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    let document =
        block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
            .unwrap();
    (host, document.id)
}
#[test]
fn missing_history_is_unsupported_and_closed_history_is_revoked() {
    let (host, document) = opened(ViewerConfig::default());
    assert!(
        matches!(block_on(host.history(document)), Err(ViewerError::Kernel { code: Some(code), .. }) if code == "unsupported")
    );
    let (host, document) = opened(ViewerConfig { history: true, ..Default::default() });
    let history = block_on(host.history(document.clone())).unwrap();
    block_on(host.close_document(document)).unwrap();
    assert!(
        matches!(block_on(history.can_undo()), Err(ViewerError::Kernel { code: Some(code), .. }) if code == "instance-closed")
    );
}

#[test]
fn history_confirms_page_undo_redo_without_crossing_documents() {
    let (host, document) =
        opened(ViewerConfig { history: true, page_edit: true, ..Default::default() });
    let other =
        block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
            .unwrap();
    let history = block_on(host.history(document.clone())).unwrap();
    let other_history = block_on(host.history(other.id)).unwrap();
    let edit = block_on(host.page_edit(document.clone())).unwrap();
    let page = block_on(host.pages(document.clone())).unwrap()[0].page;
    let (sender, receiver) = mpsc::channel();
    let _subscription = history.subscribe(Arc::new(HistoryEvents(Mutex::new(sender))));
    block_on(edit.rotate_by(
        vec![page],
        epdf_plugin_page_edit::ffi::PageEditTurn::Clockwise,
        epdf_ffi::CancelToken::new(),
    ))
    .unwrap();
    assert!(block_on(history.can_undo()).unwrap());
    assert!(!block_on(other_history.can_undo()).unwrap());
    let label = block_on(history.undo_label()).unwrap().unwrap();
    assert!(!label.key.is_empty());
    block_on(history.undo()).unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        HistoryEvent::Undone { redo: false, skipped: 0, .. }
    ));
    assert_eq!(block_on(host.pages(document.clone())).unwrap()[0].rotation, 0);
    block_on(history.redo()).unwrap();
    assert!(matches!(
        receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        HistoryEvent::Undone { redo: true, .. }
    ));
    assert_eq!(block_on(host.pages(document)).unwrap()[0].rotation, 90);
}
