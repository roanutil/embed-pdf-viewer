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

#[test]
fn outline_lifecycle_edits_events_and_saved_identity() {
    use epdf_ffi::document::DocumentEventOriginKind;
    use epdf_plugin_outline::ffi::{
        OutlineCreateOptions, OutlineEvent, OutlineEventListener, OutlineItemInput,
        OutlineItemPatch, OutlinePatchKey, OutlinePosition,
    };
    struct Events(Mutex<mpsc::Sender<OutlineEvent>>);
    impl OutlineEventListener for Events {
        fn on_event(&self, event: OutlineEvent) {
            self.0.lock().unwrap().send(event).ok();
        }
        fn on_end(&self) {}
    }
    let (host, document) = opened(ViewerConfig { outline: true, ..Default::default() });
    let outline = block_on(host.outline(document.clone())).unwrap();
    let cancel = epdf_ffi::CancelToken::new;
    assert_eq!(block_on(outline.status()).unwrap(), epdf_ffi::ResourceStatus::Idle);
    block_on(outline.ensure_loaded(cancel())).unwrap();
    assert_eq!(block_on(outline.status()).unwrap(), epdf_ffi::ResourceStatus::Ready);
    assert!(block_on(outline.current_view_target()).unwrap().is_none());
    assert!(block_on(outline.current_item()).unwrap().is_none());
    assert!(!block_on(outline.is_incomplete()).unwrap());
    assert!(block_on(outline.can_edit()).unwrap());
    let (sender, receiver) = mpsc::channel();
    let _subscription = outline.subscribe(Arc::new(Events(Mutex::new(sender))));
    let parent = block_on(outline.create(
        OutlineItemInput { title: Some("Parent".into()), clear_target: true, ..Default::default() },
        OutlineCreateOptions { select: true },
        cancel(),
    ))
    .unwrap();
    assert_eq!(block_on(outline.selected_items()).unwrap(), vec![parent.item_ref.clone()]);
    assert!(
        matches!(receiver.recv_timeout(Duration::from_secs(2)).unwrap(), OutlineEvent::Changed { origin, kind, .. } if origin.kind == DocumentEventOriginKind::Local && kind == "created")
    );
    let child = block_on(outline.create(
        OutlineItemInput {
            title: Some("Résumé".into()),
            parent: Some(parent.item_ref.clone()),
            color: Some("#ff0000".into()),
            clear_target: true,
            ..Default::default()
        },
        OutlineCreateOptions::default(),
        cancel(),
    ))
    .unwrap();
    block_on(outline.expand(parent.item_ref.clone())).unwrap();
    assert!(block_on(outline.is_expanded(parent.item_ref.clone())).unwrap());
    assert_eq!(block_on(outline.children(Some(parent.item_ref.clone()))).unwrap().len(), 1);
    assert!(
        block_on(outline.rows())
            .unwrap()
            .iter()
            .any(|row| row.item.item_ref == child.item_ref && row.depth == 1)
    );
    block_on(outline.collapse(parent.item_ref.clone())).unwrap();
    block_on(outline.toggle(parent.item_ref.clone())).unwrap();
    block_on(outline.expand_all()).unwrap();
    block_on(outline.collapse_all()).unwrap();
    block_on(outline.reveal(child.item_ref.clone())).unwrap();
    block_on(outline.select(vec![child.item_ref.clone()])).unwrap();
    assert_eq!(
        block_on(outline.activate(child.item_ref.clone())).unwrap(),
        epdf_plugin_link::ffi::LinkActivation::None
    );
    let updated = block_on(outline.update(
        child.item_ref.clone(),
        OutlineItemPatch {
            title: Some("Renamed".into()),
            clear: vec![OutlinePatchKey::Color, OutlinePatchKey::Target],
            ..Default::default()
        },
        cancel(),
    ))
    .unwrap();
    assert!(updated.color.is_none() && updated.target.is_none());
    block_on(outline.move_items(
        vec![child.item_ref.clone()],
        None,
        OutlinePosition::Start,
        cancel(),
    ))
    .unwrap();
    assert!(block_on(outline.item(child.item_ref.clone())).unwrap().unwrap().parent.is_none());
    let bytes =
        block_on(host.save(document.clone(), epdf_viewer_host::SaveMode::Incremental, cancel()))
            .unwrap();
    let reopened = block_on(host.open_document(bytes, options())).unwrap();
    let saved = block_on(host.outline(reopened.id)).unwrap();
    block_on(saved.ensure_loaded(cancel())).unwrap();
    assert_eq!(block_on(saved.item(child.item_ref.clone())).unwrap().unwrap().title, "Renamed");
    block_on(outline.delete_items(vec![child.item_ref, parent.item_ref.clone()], cancel()))
        .unwrap();
    assert!(block_on(outline.item(parent.item_ref)).unwrap().is_none());
    block_on(outline.refresh(cancel())).unwrap();
    block_on(host.close_document(document)).unwrap();
    assert!(
        matches!(block_on(outline.list()), Err(ViewerError::Kernel { code: Some(code), .. }) if code == "instance-closed")
    );
}
