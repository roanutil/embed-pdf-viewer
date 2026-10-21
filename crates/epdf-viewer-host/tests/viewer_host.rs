use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use epdf_ffi::PageRef;
use epdf_ffi::{CancelToken, ResourceStatus, ViewerError};
use epdf_plugin_actions::ffi::sandbox::{
    ActionsJavaScriptConfig, ScriptEngine, ScriptEngineFactory, ScriptEvaluation,
};
use epdf_plugin_actions::ffi::{ActionTreeSource, ActionTrigger};
use epdf_plugin_actions::{ActionNodeStatus, DocumentTriggerEvent, OpenSequence};
use epdf_plugin_commands::ffi::{CommandDefinition, CommandExecuteResult, CommandPanel};
use epdf_plugin_i18n::ffi::{I18nConfig, LocalePack};
use epdf_plugin_metadata::ffi::MetadataPatch;
use epdf_plugin_page_edit::ffi::{PageEditBlankOptions, PageEditPlacement, PageEditTurn};
use epdf_plugin_render::ffi::{RenderConfig, RenderPageOptions, RenderPixelFormat};
use epdf_plugin_search::ffi::{SearchConfig, SearchQuery, SearchResultStatus, SearchStatus};
use epdf_plugin_shell::ffi::ShellOpenOptions;
use epdf_viewer_host::{
    DocumentEvent, DocumentEventListener, DocumentStatus, OpenDocumentOptions, SaveMode,
    ViewerConfig, ViewerDelegate, ViewerHost,
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
fn a_locked_document_opens_as_a_locked_tab() {
    let host = started();
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
            .unwrap();
    assert_eq!(doc.status, DocumentStatus::Locked, "{doc:?}");
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
fn a_failed_open_lists_its_tab_as_failed_with_the_reason() {
    let host = started();
    // A wrong password fails the open; no password at all opens locked.
    let options = OpenDocumentOptions { password: Some("nope".into()), ..options() };
    let _ = block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options))
        .unwrap_err();
    let documents = block_on(host.documents()).unwrap();
    let [tab] = documents.as_slice() else { panic!("{documents:?}") };
    assert_eq!(tab.status, DocumentStatus::Failed, "{tab:?}");
    let error = tab.error.as_ref().expect("a failed tab without its error");
    // The kernel's `toPluginError` keeps the engine code in the message.
    assert!(error.message.starts_with("DocPasswordIncorrect"), "{error:?}");
}

#[test]
fn an_open_document_lists_as_ready_without_an_error() {
    let host = started();
    block_on(host.open_document(test_support::fixture(test_support::REPORT), options())).unwrap();
    let documents = block_on(host.documents()).unwrap();
    let [tab] = documents.as_slice() else { panic!("{documents:?}") };
    assert_eq!(tab.status, DocumentStatus::Ready, "{tab:?}");
    assert_eq!(tab.error, None);
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

/// Every document event, as sent.
#[derive(Default)]
struct Recorded(Mutex<Vec<DocumentEvent>>);
impl DocumentEventListener for Recorded {
    fn on_event(&self, event: DocumentEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn a_failed_open_announces_the_tabs_error_with_its_code() {
    let host = started();
    let events = Arc::new(Recorded::default());
    let _subscription = host.subscribe_documents(events.clone());
    let options = OpenDocumentOptions { password: Some("nope".into()), ..options() };
    let _ = block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options))
        .unwrap_err();
    let tab = block_on(host.documents()).unwrap().remove(0);
    let failed = events.0.lock().unwrap().iter().find_map(|event| match event {
        DocumentEvent::OpenFailed { document_id, error } => {
            Some((document_id.clone(), error.clone()))
        }
        _ => None,
    });
    let (document_id, error) = failed.expect("no OpenFailed event");
    assert_eq!(document_id, tab.id);
    assert_eq!(Some(&error), tab.error.as_ref());
    assert_eq!(error.code, "permission-denied", "{error:?}");
    assert_eq!(error.capability, "documents");
    assert!(error.message.starts_with("DocPasswordIncorrect"), "{error:?}");
}

#[test]
fn a_locked_open_says_whether_a_password_was_given() {
    let host = started();
    let events = Arc::new(Recorded::default());
    let _subscription = host.subscribe_documents(events.clone());
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
            .unwrap();
    let locked = events.0.lock().unwrap().clone();
    assert!(
        locked.contains(&DocumentEvent::Locked { document_id: doc.id, password_provided: false }),
        "{locked:?}"
    );
}

fn kernel_code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn a_wrong_password_is_permission_denied_with_the_engine_code_in_the_detail() {
    let host = started();
    let wrong = OpenDocumentOptions { password: Some("nope".into()), ..options() };
    let error = block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), wrong))
        .unwrap_err();
    assert_eq!(kernel_code(&error), Some("permission-denied"), "{error:?}");
    assert!(format!("{error}").contains("DocPasswordIncorrect"), "{error}");
    let locked =
        block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
            .unwrap();
    let error = block_on(host.unlock_document(locked.id, "nope".into())).unwrap_err();
    assert_eq!(kernel_code(&error), Some("permission-denied"), "{error:?}");
}

#[test]
fn pages_of_an_unknown_document_are_not_found() {
    let host = started();
    let error = block_on(host.pages("nope".into())).unwrap_err();
    match &error {
        ViewerError::Kernel { name, code, .. } => {
            assert_eq!((name.as_str(), code.as_deref()), ("PluginError", Some("not-found")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_capability_refusal_says_why_in_the_plugin_vocabulary() {
    let host = with(ViewerConfig { metadata: true, ..Default::default() });
    let unknown = block_on(host.metadata("nope".into())).map(|_| ()).unwrap_err();
    assert_eq!(kernel_code(&unknown), Some("not-found"), "{unknown:?}");
    let locked =
        block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
            .unwrap();
    let error = block_on(host.metadata(locked.id.clone())).map(|_| ()).unwrap_err();
    assert_eq!(kernel_code(&error), Some("not-ready"), "{error:?}");
    let error = block_on(host.search(locked.id)).map(|_| ()).unwrap_err();
    assert_eq!(kernel_code(&error), Some("unsupported"), "{error:?}");
    block_on(host.destroy()).unwrap();
    let error = block_on(host.metadata("nope".into())).map(|_| ()).unwrap_err();
    assert_eq!(kernel_code(&error), Some("instance-closed"), "{error:?}");
}

#[test]
fn save_answers_the_whole_file_in_either_mode_and_it_reopens() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    for mode in [SaveMode::Incremental, SaveMode::Rewrite] {
        let bytes = block_on(host.save(doc.id.clone(), mode, CancelToken::new())).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "{mode:?}");
        let again = block_on(host.open_document(bytes, options())).unwrap();
        assert_eq!(again.page_count, 14, "{mode:?}");
    }
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 0, "a save left its bytes behind");
}

#[test]
fn an_incremental_save_keeps_the_original_bytes_as_its_prefix() {
    let host = started();
    let original = test_support::fixture(test_support::REPORT);
    let doc = block_on(host.open_document(original.clone(), options())).unwrap();
    let bytes = block_on(host.save(doc.id, SaveMode::Incremental, CancelToken::new())).unwrap();
    assert!(bytes.starts_with(&original), "an incremental save rewrote the original bytes");
}

#[test]
fn save_cancels_when_its_token_is_cancelled() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let token = CancelToken::new();
    token.cancel();
    let error = block_on(host.save(doc.id.clone(), SaveMode::Rewrite, token)).unwrap_err();
    assert_eq!(kernel_code(&error), Some("operation-cancelled"), "{error:?}");
    // The host still saves afterwards.
    assert!(block_on(host.save(doc.id, SaveMode::Rewrite, CancelToken::new())).is_ok());
}

#[test]
fn a_signed_document_refuses_a_rewrite_save_but_not_an_incremental_one() {
    let host = started();
    let signed =
        test_support::fixture("packages/engine/main/test/fixtures/signed_two_approvals.pdf");
    let doc = block_on(host.open_document(signed.clone(), options())).unwrap();
    let error =
        block_on(host.save(doc.id.clone(), SaveMode::Rewrite, CancelToken::new())).unwrap_err();
    assert_eq!(kernel_code(&error), Some("permission-denied"), "{error:?}");
    let bytes = block_on(host.save(doc.id, SaveMode::Incremental, CancelToken::new())).unwrap();
    assert!(bytes.starts_with(&signed));
}

#[test]
fn saving_an_unknown_document_fails() {
    let host = started();
    let error =
        block_on(host.save("nope".into(), SaveMode::Incremental, CancelToken::new())).unwrap_err();
    assert!(format!("{error:?}").contains("no document to save"), "{error:?}");
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
    let engine = block_on(host.engine_thread_witness()).unwrap();
    assert!(engine.upgrade().is_some());
    drop(host);
    assert!(weak.upgrade().is_none());
    // KernelThread::drop joins the kernel thread, whose Session::drop joins
    // the engine's client thread once it has closed PDFium. A hang here is
    // also a failure.
    assert!(engine.upgrade().is_none(), "the engine thread outlived its host");
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
    let host =
        ViewerHost::new(ViewerConfig { metadata: true, ..Default::default() }, Arc::new(Delegate))
            .unwrap();
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

fn with_search() -> Arc<ViewerHost> {
    let config = ViewerConfig { search: Some(SearchConfig::default()), ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_search_plugin_finds_hits_through_the_native_engine() {
    let host = with_search();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let search = block_on(host.search(doc.id)).unwrap();
    // A start page goes out as a `PageRef` and has to reach the host as its
    // object number.
    let query = SearchQuery { text: "Report".into(), ..Default::default() };
    let result = block_on(search.search(query, Some(pages[1].page), CancelToken::new())).unwrap();
    assert_eq!(result.status, SearchResultStatus::Complete);
    assert!(result.hit_count > 0, "{result:?}");
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Complete);
    let hit = block_on(search.active_hit()).unwrap().unwrap();
    let page = pages.iter().find(|p| p.page == hit.page).expect("the hit's page is listed");
    assert_eq!(hit.page_index, page.index);
    assert!(hit.bounds.is_some() && hit.snippet.is_some(), "{hit:?}");
    let the = SearchQuery { text: "the".into(), ..Default::default() };
    let all = block_on(search.find_all(the, None, CancelToken::new())).unwrap();
    assert!(all.len() > 1);
}

#[test]
fn without_its_config_the_search_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.search(doc.id)).is_err());
}

fn with_render() -> Arc<ViewerHost> {
    let config = ViewerConfig { render: Some(RenderConfig::default()), ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_render_plugin_renders_rgba_through_the_native_engine() {
    let host = with_render();
    let doc = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap().remove(0);
    let render = block_on(host.render(doc.id)).unwrap();
    assert!(block_on(render.can_render()).unwrap());
    let options = RenderPageOptions { scale: Some(1.0), ..Default::default() };
    let image = block_on(render.render_page(page.page, options, CancelToken::new())).unwrap();
    assert_eq!(f64::from(image.width), page.width.round());
    assert_eq!(f64::from(image.height), page.height.round());
    assert!(image.stride >= image.width * 4);
    assert_eq!(image.pixels.len(), (image.stride * image.height) as usize);
    assert_eq!(image.format, RenderPixelFormat::Rgba8);
    // Channel order: the fixture is an 80 by 20 point page of four 20-point
    // squares, red, green, blue and (0.2, 0.4, 0.6), so a red and blue swap
    // shows in the first square.
    assert_eq!((image.width, image.height), (80, 20));
    let pixel = |x: usize| {
        let at = 10 * image.stride as usize + x * 4;
        image.pixels[at..at + 4].to_vec()
    };
    assert_eq!(pixel(10), [255, 0, 0, 255]);
    assert_eq!(pixel(30), [0, 255, 0, 255]);
    assert_eq!(pixel(50), [0, 0, 255, 255]);
    assert_eq!(pixel(70), [51, 102, 153, 255]);
}

#[test]
fn a_closed_documents_pixels_go_with_its_render_handle() {
    let host = with_render();
    let doc = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = block_on(host.render(doc.id.clone())).unwrap();
    let options = RenderPageOptions { scale: Some(0.5), ..Default::default() };
    block_on(render.render_page(page, options, CancelToken::new())).unwrap();
    // The raster store keeps the image, and the host table its pixels.
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 1);
    block_on(host.close_document(doc.id)).unwrap();
    assert_eq!(block_on(host.host_table_entries()).unwrap(), 1);
    drop(render);
    let deadline = Instant::now() + Duration::from_secs(5);
    while block_on(host.host_table_entries()).unwrap() != 0 {
        assert!(Instant::now() < deadline, "the closed document's pixels were never released");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn shutting_down_with_a_rendered_image_held_does_not_abort() {
    // The kernel thread's slab still holds the render capability when the
    // thread exits, and dropping it releases the raster store's pixels while
    // thread locals are being torn down.
    let host = with_render();
    let doc = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let render = block_on(host.render(doc.id)).unwrap();
    let options = RenderPageOptions { scale: Some(0.25), ..Default::default() };
    let image = block_on(render.render_page(page, options, CancelToken::new())).unwrap();
    block_on(host.destroy()).unwrap();
    host.shutdown();
    drop(render);
    assert!(!image.pixels.is_empty());
}

#[test]
fn without_its_config_the_render_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
        .unwrap();
    assert!(block_on(host.render(doc.id)).is_err());
}

#[test]
fn the_features_fixture_opens() {
    let host = started();
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    assert_eq!(doc.page_count, 3);
}

use epdf_ffi::{HostError, Modifiers, Point, PointerPageHit, PointerPhase, PointerSample};
use epdf_plugin_interaction::ffi::{InteractionHandler, InteractionTool};

#[derive(Default)]
struct Reports(Mutex<Vec<String>>);
impl ViewerDelegate for Reports {
    fn state_changed(&self, _revision: u64) {}
    fn report(&self, error: ViewerError) {
        self.0.lock().unwrap().push(format!("{error}"));
    }
}

struct Throws;
impl InteractionHandler for Throws {
    fn enabled_for(&self, _tool: InteractionTool) -> bool {
        true
    }
    fn on_down(&self, _sample: PointerSample) -> Result<bool, HostError> {
        Err(HostError::Failed { detail: "swift threw".into() })
    }
    fn on_move(&self, _sample: PointerSample) {}
    fn on_up(&self, _sample: PointerSample) {}
    fn on_cancel(&self, _sample: PointerSample) {}
    fn on_hover(&self, _sample: PointerSample) {}
    fn claims_touch(&self, _sample: PointerSample) -> bool {
        false
    }
}

fn with_interaction(delegate: Arc<dyn ViewerDelegate>) -> Arc<ViewerHost> {
    let config = ViewerConfig { interaction: true, ..Default::default() };
    let host = ViewerHost::new(config, delegate).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_interaction_plugin_switches_tools_through_the_host() {
    let host = with_interaction(Arc::new(Delegate));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pointer");
    block_on(interaction.activate_tool("pan".into())).unwrap();
    assert_eq!(block_on(interaction.active_tool_id()).unwrap(), "pan");
}

#[test]
fn a_foreign_handlers_error_reaches_the_delegate() {
    let reports = Arc::new(Reports::default());
    let host = with_interaction(reports.clone());
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let _registration =
        block_on(interaction.register_handler("throws".into(), 1.0, None, Arc::new(Throws)))
            .unwrap();
    interaction.dispatch_pointer(PointerSample {
        phase: PointerPhase::Down,
        viewport: Point { x: 10.0, y: 10.0 },
        page: Some(PointerPageHit {
            page: pages[0].page,
            point: Point { x: 10.0, y: 10.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: None,
        gesture: None,
        source: None,
    });
    // Any reply after the fire means the dispatch has run.
    block_on(interaction.cursor()).unwrap();
    let reports = reports.0.lock().unwrap();
    assert!(
        reports.iter().any(|r| r.contains("HostError") && r.contains("swift threw")),
        "{reports:?}"
    );
}

#[test]
fn without_its_flag_the_interaction_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.interaction(doc.id)).is_err());
}

#[test]
fn without_its_config_the_feedback_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.feedback()).is_err());
}

use epdf_ffi::Size as FfiSize;
use epdf_plugin_stage::ffi::{StageConfig, StageGoToOptions, StageScrollBehavior};

fn with(config: ViewerConfig) -> Arc<ViewerHost> {
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_stage_plugin_lays_out_and_navigates_through_the_host() {
    let host = with(ViewerConfig { stage: Some(StageConfig::default()), ..Default::default() });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id)).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    assert!(!block_on(stage.visible_pages()).unwrap().is_empty());
    let instant =
        StageGoToOptions { behavior: Some(StageScrollBehavior::Instant), ..Default::default() };
    block_on(stage.go_to_page_index(5, Some(instant))).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 5);
}

#[test]
fn revealing_the_active_hit_moves_the_stage() {
    let host = with(ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        search: Some(SearchConfig::default()),
        ..Default::default()
    });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    let search = block_on(host.search(doc.id)).unwrap();
    let query = SearchQuery { text: "the".into(), ..Default::default() };
    block_on(search.search(query, None, CancelToken::new())).unwrap();
    let before = block_on(stage.camera()).unwrap();
    let mut moved = false;
    // Walk hits until one is off screen; revealing it must move the camera.
    for _ in 0..50 {
        let hit = block_on(search.next_hit(None)).unwrap().unwrap();
        let after = block_on(stage.camera()).unwrap();
        if after != before {
            assert!(block_on(stage.is_page_visible(hit.page)).unwrap());
            moved = true;
            break;
        }
    }
    assert!(moved, "no hit moved the stage");
}

#[test]
fn a_pan_tool_drag_scrolls_the_stage() {
    use epdf_ffi::{Modifiers, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample};
    let host = with(ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        interaction: true,
        ..Default::default()
    });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(interaction.activate_tool("pan".into())).unwrap();
    // The surface binding's job on the web (`<Stage>` registers it when it
    // forwards samples to the hub); a native reader asks for it.
    let _scroll = block_on(stage.register_scroll_handler(None)).unwrap();
    let lens = block_on(stage.lens_id()).unwrap();
    let at = |phase, y: f64| PointerSample {
        phase,
        viewport: Point { x: 200.0, y },
        page: Some(PointerPageHit {
            page: pages[0].page,
            point: Point { x: 100.0, y },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: None,
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: Some(lens.clone()),
    };
    let before = block_on(stage.camera()).unwrap();
    interaction.dispatch_pointer(at(PointerPhase::Down, 600.0));
    interaction.dispatch_pointer(at(PointerPhase::Move, 400.0));
    interaction.dispatch_pointer(at(PointerPhase::Up, 400.0));
    let after = block_on(stage.camera()).unwrap();
    assert!(after.y != before.y, "{before:?} -> {after:?}");
}

#[test]
fn a_fling_in_flight_at_close_stops_without_a_report() {
    let reports = Arc::new(Reports(Mutex::default()));
    let host = ViewerHost::new(
        ViewerConfig { stage: Some(StageConfig::default()), ..Default::default() },
        reports.clone(),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    block_on(stage.fling(0.0, -4000.0)).unwrap();
    assert!(block_on(stage.is_moving()).unwrap());
    block_on(host.close_document(doc.id)).unwrap();
    // A few frames' worth of the kernel's 16 ms timer.
    std::thread::sleep(Duration::from_millis(200));
    block_on(host.documents()).unwrap();
    assert!(reports.0.lock().unwrap().is_empty(), "{:?}", reports.0.lock().unwrap());
}

#[test]
fn a_scroll_handler_without_the_interaction_plugin_is_unsupported() {
    let host = with(ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        ..Default::default()
    });
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id)).unwrap();
    match block_on(stage.register_scroll_handler(None)).map(|_| ()).unwrap_err() {
        ViewerError::Kernel { code, .. } => assert_eq!(code.as_deref(), Some("unsupported")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn without_its_config_the_stage_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.stage(doc.id)).is_err());
}

use epdf_ffi::{
    Modifiers as SelModifiers, Point as SelPoint, PointerGesture, PointerKind as SelKind,
    PointerPageHit as SelHit, PointerPhase as SelPhase, PointerSample as SelSample,
    Rect as SelRect,
};
use epdf_plugin_interaction::ffi::{
    FeedbackConfig, FeedbackImpactWeight, FeedbackNotifyKind, InteractionEvent,
    InteractionEventListener, PlatformFeedback,
};
use epdf_plugin_selection::ffi::SelectionHandle;

fn with_selection(feedback: Option<Arc<FeedbackConfig>>) -> Arc<ViewerHost> {
    let config =
        ViewerConfig { interaction: true, selection: true, feedback, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

/// A text rect on page 0 of the report, found through selection itself.
fn text_rect(selection: &SelectionHandle, page: epdf_ffi::PageRef) -> SelRect {
    block_on(selection.ensure_loaded(page)).unwrap();
    block_on(selection.select_page(page)).unwrap();
    let rect = block_on(selection.rects(page)).unwrap()[0];
    block_on(selection.clear()).unwrap();
    rect
}

fn touch(phase: SelPhase, page: epdf_ffi::PageRef, point: SelPoint) -> SelSample {
    SelSample {
        phase,
        // Viewport points equal page points: a 1:1 view, enough to clear the drag threshold.
        viewport: point,
        page: Some(SelHit { page, point, scale: None, rotation: None, zoom: None }),
        modifiers: SelModifiers::default(),
        click_count: None,
        pointer_type: Some(SelKind::Touch),
        gesture: None,
        source: None,
    }
}

#[test]
fn a_plugin_without_its_requirement_fails_to_build() {
    let config = ViewerConfig { selection: true, ..Default::default() };
    let error = ViewerHost::new(config, Arc::new(Delegate)).err().unwrap();
    let ViewerError::Kernel { detail, .. } = error else { panic!("{error:?}") };
    assert!(detail.contains("requires capability \"interaction\""), "{detail}");
}

#[test]
fn the_selection_plugin_selects_and_reads_through_the_host() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let selection = block_on(host.selection(doc.id)).unwrap();
    block_on(selection.select_page(pages[0].page)).unwrap();
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    // report.pdf opens with the title "Trace-based Just-in-Time Type
    // Specialization for Dynamic Languages".
    assert!(text.contains("Dynamic"), "{text:?}");
}

#[test]
fn a_pointer_drag_over_text_selects_it() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let rect = text_rect(&selection, page);
    let y = rect.y + rect.height / 2.0;
    interaction.dispatch_pointer(touch(SelPhase::Down, page, SelPoint { x: rect.x + 1.0, y }));
    interaction.dispatch_pointer(touch(
        SelPhase::Move,
        page,
        SelPoint { x: rect.x + rect.width - 1.0, y },
    ));
    interaction.dispatch_pointer(touch(
        SelPhase::Up,
        page,
        SelPoint { x: rect.x + rect.width - 1.0, y },
    ));
    assert!(block_on(selection.has_selection()).unwrap());
    assert!(!block_on(selection.read_text(CancelToken::new())).unwrap().is_empty());
}

#[test]
fn a_drag_that_starts_off_text_selects_nothing() {
    let host = with_selection(None);
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(selection.ensure_loaded(page)).unwrap();
    // The page's top-left corner: inside the page, outside any text.
    let corner = SelPoint { x: 1.0, y: 1.0 };
    assert!(!block_on(selection.is_over_text(page, corner)).unwrap());
    let gestures = Arc::new(Gestures::default());
    let _sub = interaction.subscribe(gestures.clone());
    interaction.dispatch_pointer(touch(SelPhase::Down, page, corner));
    interaction.dispatch_pointer(touch(SelPhase::Move, page, SelPoint { x: 300.0, y: 300.0 }));
    interaction.dispatch_pointer(touch(SelPhase::Up, page, SelPoint { x: 300.0, y: 300.0 }));
    assert!(!block_on(selection.has_selection()).unwrap());
    // No handler captured the drag, so a lower one (stage's pan) would get
    // it. A press on text, as a control, is captured.
    assert!(gestures.0.lock().unwrap().is_empty(), "{:?}", gestures.0.lock().unwrap());
    let rect = text_rect(&selection, page);
    let on_text = SelPoint { x: rect.x + 1.0, y: rect.y + rect.height / 2.0 };
    interaction.dispatch_pointer(touch(SelPhase::Down, page, on_text));
    block_on(selection.has_selection()).unwrap();
    assert_eq!(gestures.0.lock().unwrap().len(), 1);
}

/// The handlers that captured a gesture, in order.
#[derive(Default)]
struct Gestures(Mutex<Vec<String>>);
impl InteractionEventListener for Gestures {
    fn on_event(&self, event: InteractionEvent) {
        if let InteractionEvent::GestureStarted { event } = event {
            self.0.lock().unwrap().push(event.handler_id);
        }
    }
    fn on_end(&self) {}
}

#[derive(Default)]
struct Ticks(Mutex<Vec<String>>);
impl PlatformFeedback for Ticks {
    fn selection(&self) {
        self.0.lock().unwrap().push("selection".into());
    }
    fn impact(&self, _weight: Option<FeedbackImpactWeight>) {}
    fn notify(&self, _kind: FeedbackNotifyKind) {}
}

#[test]
fn a_long_press_on_a_word_ticks_the_feedback() {
    let ticks = Arc::new(Ticks::default());
    let host = with_selection(Some(FeedbackConfig::new(ticks.clone())));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let selection = block_on(host.selection(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    let rect = text_rect(&selection, page);
    let mut press =
        touch(SelPhase::Down, page, SelPoint { x: rect.x + 2.0, y: rect.y + rect.height / 2.0 });
    press.click_count = Some(2);
    press.gesture = Some(PointerGesture::LongPress);
    interaction.dispatch_pointer(press.clone());
    press.phase = SelPhase::Up;
    interaction.dispatch_pointer(press);
    assert!(block_on(selection.has_selection()).unwrap());
    assert_eq!(*ticks.0.lock().unwrap(), ["selection"]);
}

#[test]
fn without_its_flag_the_selection_plugin_is_not_installed() {
    let host = with_interaction(Arc::new(Delegate));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.selection(doc.id)).is_err());
}

use epdf_ffi::PdfLinkTarget as HostTarget;
use epdf_plugin_link::ffi::{LinkActivation, LinkInput};

fn with_links(stage: bool) -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        link: true,
        stage: stage
            .then(|| epdf_plugin_stage::ffi::StageConfig { frames: false, ..Default::default() }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_link_plugin_lists_and_activates_links_through_the_host() {
    let host = with_links(false);
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let link = block_on(host.link(doc.id)).unwrap();
    block_on(link.ensure_loaded(pages[0].page)).unwrap();
    let links = block_on(link.links(pages[0].page)).unwrap();
    assert_eq!(links.len(), 2, "{links:?}");
    let external = links.iter().find(|l| matches!(l.target, HostTarget::Uri { .. })).unwrap();
    let activation =
        block_on(link.activate(LinkInput::Link { page: pages[0].page, id: external.id.clone() }))
            .unwrap();
    assert_eq!(activation, LinkActivation::Uri { uri: "https://example.invalid/target".into() });
}

#[test]
fn activating_a_goto_link_moves_the_stage() {
    let host = with_links(true);
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(epdf_ffi::Size { width: 390.0, height: 300.0 })).unwrap();
    let link = block_on(host.link(doc.id)).unwrap();
    block_on(link.ensure_loaded(pages[0].page)).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 0);
    // Inside the internal link: x 40..320, y 70..100 in page space.
    let activation =
        block_on(link.activate_at(pages[0].page, epdf_ffi::Point { x: 100.0, y: 85.0 })).unwrap();
    assert_eq!(activation, Some(LinkActivation::Revealed));
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 2);
}

#[test]
fn a_tap_on_a_link_activates_it() {
    use epdf_ffi::{Modifiers, Point, PointerKind, PointerPageHit, PointerPhase, PointerSample};
    use epdf_plugin_link::ffi::{LinkEvent, LinkEventListener};
    #[derive(Default)]
    struct Seen(Mutex<Vec<LinkEvent>>);
    impl LinkEventListener for Seen {
        fn on_event(&self, event: LinkEvent) {
            self.0.lock().unwrap().push(event);
        }
        fn on_end(&self) {}
    }
    let host = with_links(false);
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let link = block_on(host.link(doc.id.clone())).unwrap();
    let interaction = block_on(host.interaction(doc.id)).unwrap();
    block_on(link.ensure_loaded(page)).unwrap();
    let seen = Arc::new(Seen::default());
    let _sub = link.subscribe(seen.clone());
    // The web's link layer takes the click on its anchors; a native reader
    // asks for the handler that takes it from the hub.
    let _tap = block_on(link.register_tap_handler()).unwrap();
    // Inside the external link: x 40..360, y 130..160.
    let at = |phase| PointerSample {
        phase,
        viewport: Point { x: 100.0, y: 145.0 },
        page: Some(PointerPageHit {
            page,
            point: Point { x: 100.0, y: 145.0 },
            scale: None,
            rotation: None,
            zoom: None,
        }),
        modifiers: Modifiers::default(),
        click_count: Some(1),
        pointer_type: Some(PointerKind::Touch),
        gesture: None,
        source: None,
    };
    interaction.dispatch_pointer(at(PointerPhase::Down));
    interaction.dispatch_pointer(at(PointerPhase::Up));
    block_on(link.is_loaded(page)).unwrap();
    let seen = seen.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, LinkEvent::Activated { event }
        if matches!(&event.activation, LinkActivation::Uri { uri } if uri == "https://example.invalid/target"))),
        "{seen:?}");
}

/// `15-actions.md` (Cross-plugin tests) gives this one to plan 04. Both of
/// `features.pdf`'s links carry an `/A` tree, so with actions installed an
/// activation hands the tree to actions, and the internal link's `GoTo`
/// still moves the stage, through the executor the stage registers.
#[test]
fn with_actions_a_link_dispatches_its_action_tree() {
    let config = ViewerConfig {
        interaction: true,
        link: true,
        stage: Some(epdf_plugin_stage::ffi::StageConfig { frames: false, ..Default::default() }),
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: None,
            utc_offset_minutes: None,
        }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(epdf_ffi::Size { width: 390.0, height: 300.0 })).unwrap();
    let link = block_on(host.link(doc.id)).unwrap();
    block_on(link.ensure_loaded(pages[0].page)).unwrap();
    let links = block_on(link.links(pages[0].page)).unwrap();
    assert!(links.iter().all(|l| l.has_action), "{links:?}");
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 0);
    let activation =
        block_on(link.activate_at(pages[0].page, epdf_ffi::Point { x: 100.0, y: 85.0 })).unwrap();
    assert_eq!(activation, Some(LinkActivation::Dispatched));
    // The dispatch runs on the document's serial queue; a later call on the
    // kernel thread lets it settle.
    let deadline = Instant::now() + Duration::from_secs(5);
    while block_on(stage.current_page_index()).unwrap() != 2 {
        assert!(Instant::now() < deadline, "the GoTo never moved the stage");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn without_its_flag_the_link_plugin_is_not_installed() {
    let host = with_interaction(Arc::new(Delegate));
    let doc =
        block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
            .unwrap();
    assert!(block_on(host.link(doc.id)).is_err());
}

use epdf_plugin_annotation::ffi::{AnnotationConfig, AnnotationPatch, CreateAnnotationInput};

const ANNOTATED: &str = "tooling/engine-conformance/fixtures/annotation_kinds.pdf";

fn with_annotation() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(AnnotationConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_annotation_plugin_hydrates_through_the_native_engine() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(ANNOTATED), options())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let all = block_on(annotations.list(None)).unwrap();
    assert!(!all.is_empty(), "annotation_kinds.pdf has annotations");
}

#[test]
fn an_annotation_read_back_updates_through_the_native_engine() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(ANNOTATED), options())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let first = block_on(annotations.list(None)).unwrap().remove(0);
    block_on(annotations.update(
        first.annotation_ref.clone(),
        AnnotationPatch { contents: Some("Edited from Rust".into()), ..Default::default() },
        CancelToken::new(),
    ))
    .unwrap();
    let again = block_on(annotations.get(first.annotation_ref)).unwrap().unwrap();
    assert_eq!(again.contents.as_deref(), Some("Edited from Rust"));
}

#[test]
fn a_square_created_on_report_is_listed() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let created = block_on(annotations.create(
        CreateAnnotationInput {
            page: pages[0].page,
            subtype: "square".into(),
            bounds: Some(epdf_ffi::Rect { x: 72.0, y: 72.0, width: 100.0, height: 50.0 }),
            ..Default::default()
        },
        CancelToken::new(),
    ))
    .unwrap();
    assert!(block_on(annotations.get(created)).unwrap().is_some());
}

#[test]
fn annotation_without_interaction_fails_to_build() {
    let config = ViewerConfig {
        interaction: false,
        annotation: Some(AnnotationConfig::default()),
        ..Default::default()
    };
    let error = ViewerHost::new(config, Arc::new(Delegate)).err().expect("no interaction plugin");
    assert!(format!("{error}").contains("interaction"), "{error}");
}

#[test]
fn without_its_config_the_annotation_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.annotation(doc.id)).is_err());
}

#[test]
fn a_reply_lands_through_the_native_engine() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let pages = block_on(host.pages(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let root = block_on(annotations.create(
        CreateAnnotationInput {
            page: pages[0].page,
            subtype: "square".into(),
            bounds: Some(epdf_ffi::Rect { x: 72.0, y: 72.0, width: 100.0, height: 50.0 }),
            ..Default::default()
        },
        CancelToken::new(),
    ))
    .unwrap();
    let reply =
        block_on(annotations.threads().reply(root, "Agreed".into(), CancelToken::new())).unwrap();
    // The draft's `inReplyTo` reaches the host as the flat ref it parses
    // (before, the create failed WireFormat). `epdf-engine`'s create does
    // not write `/IRT` yet, so the reply is not read back as threaded.
    let read = block_on(annotations.get(reply)).unwrap().unwrap();
    assert_eq!(read.subtype, "text");
    assert_eq!(read.contents.as_deref(), Some("Agreed"));
}

use epdf_plugin_form::ffi::{
    FormConfig, FormDataFormat, FormFieldDetail, FormKind, FormValidation,
};

const FORM: &str = "tooling/engine-conformance/fixtures/form_fields.pdf";

fn with_form() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(epdf_plugin_annotation::ffi::AnnotationConfig::default()),
        form: Some(FormConfig { validation: Some(FormValidation::None) }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_fixtures_fields_list_with_widget_refs() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let fields = block_on(form.fields(None)).unwrap();
    assert!(!fields.is_empty());
    assert!(fields.iter().flat_map(|f| &f.widgets).all(|w| w.annotation_ref.is_some()));
}

#[test]
fn a_text_field_fills_through_the_native_engine() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let text = block_on(form.fields(None))
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Text { .. }) && !f.flags.read_only)
        .expect("form_fields.pdf has a writable text field");
    block_on(form.set_text(text.field_ref.clone(), "Filled".into(), CancelToken::new())).unwrap();
    let again = block_on(form.field(text.field_ref)).unwrap().unwrap();
    assert!(matches!(again.detail, FormFieldDetail::Text { value, .. } if value == "Filled"));
}

#[test]
fn a_checkbox_is_checked_by_its_on_state() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let field = block_on(form.fields(None))
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Checkbox { .. }) && !f.flags.read_only)
        .expect("form_fields.pdf has a writable checkbox");
    let on_state = field.widgets[0].toggle.as_ref().unwrap().on_state.clone();
    // The fixture's box starts checked; clear it so checking it is a change.
    let cleared =
        block_on(form.set_checked(field.field_ref.clone(), None, CancelToken::new())).unwrap();
    assert_eq!(cleared.status, epdf_plugin_form::ffi::FormCommitStatus::Applied);
    let checked =
        block_on(form.set_checked(field.field_ref.clone(), Some(on_state), CancelToken::new()))
            .unwrap();
    assert_eq!(checked.status, epdf_plugin_form::ffi::FormCommitStatus::Applied);
    let read = block_on(form.field(field.field_ref)).unwrap().unwrap();
    assert!(matches!(read.detail, FormFieldDetail::Checkbox { checked: true, .. }), "{read:?}");
    assert_eq!(read.widgets[0].toggle.as_ref().map(|t| t.checked), Some(true));
}

#[test]
fn a_reset_restores_the_default_through_the_native_engine() {
    let reports = Arc::new(Reports::default());
    let config = ViewerConfig {
        interaction: true,
        form: Some(FormConfig { validation: Some(FormValidation::None) }),
        ..Default::default()
    };
    let host = ViewerHost::new(config, reports.clone()).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(FORM), options())).unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let text = block_on(form.fields(None))
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Text { .. }) && !f.flags.read_only)
        .unwrap();
    block_on(form.set_text(text.field_ref.clone(), "Changed".into(), CancelToken::new())).unwrap();
    // `reset` takes `forms.applyEffects` when the host has it.
    block_on(form.reset(text.field_ref.clone(), CancelToken::new())).unwrap();
    let read = block_on(form.field(text.field_ref)).unwrap().unwrap();
    let FormFieldDetail::Text { value, default_value, .. } = read.detail else { unreachable!() };
    assert_eq!(value, default_value);
    assert_eq!(*reports.0.lock().unwrap(), Vec::<String>::new());
}

#[test]
fn an_fdf_export_imports_back() {
    let host = with_form();
    let bytes = test_support::fixture(FORM);
    let first = block_on(host.open_document(bytes.clone(), options())).unwrap();
    let form = block_on(host.form(first.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    let text = block_on(form.fields(None))
        .unwrap()
        .into_iter()
        .find(|f| matches!(f.detail, FormFieldDetail::Text { .. }) && !f.flags.read_only)
        .unwrap();
    block_on(form.set_text(text.field_ref.clone(), "Round trip".into(), CancelToken::new()))
        .unwrap();
    let export = block_on(form.export_data(Some(FormDataFormat::Fdf), CancelToken::new())).unwrap();
    assert!(export.bytes.starts_with(b"%FDF"));
    let second = block_on(host.open_document(bytes, options())).unwrap();
    let fresh = block_on(host.form(second.id)).unwrap();
    block_on(fresh.refresh(CancelToken::new())).unwrap();
    block_on(fresh.import_data(export.bytes, Some(FormDataFormat::Fdf), CancelToken::new()))
        .unwrap();
    let imported = block_on(fresh.field(text.field_ref)).unwrap().unwrap();
    assert!(
        matches!(imported.detail, FormFieldDetail::Text { value, .. } if value == "Round trip")
    );
}

#[test]
fn a_document_without_a_form_lists_no_fields() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let form = block_on(host.form(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    assert!(block_on(form.fields(None)).unwrap().is_empty());
    assert_eq!(block_on(form.snapshot()).unwrap().map(|s| s.form_kind), Some(FormKind::None));
}

#[test]
fn a_created_field_is_selectable_as_an_annotation() {
    let host = with_form();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let form = block_on(host.form(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    block_on(form.refresh(CancelToken::new())).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let created = block_on(form.create_field(
        epdf_plugin_form::ffi::FormCreateFieldInput {
            family: epdf_ffi::FormFieldFamily::Text,
            page,
            bounds: epdf_ffi::Rect { x: 72.0, y: 72.0, width: 200.0, height: 24.0 },
            name: Some("added".into()),
            options: None,
        },
        CancelToken::new(),
    ))
    .unwrap();
    let widget = created.widget.and_then(|w| w.annotation_ref).expect("the widget's ref");
    assert!(block_on(annotations.get(widget)).unwrap().is_some());
}

#[test]
fn without_its_config_the_form_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.form(doc.id)).is_err());
}

use epdf_plugin_redaction::ffi::{RedactionConfig, RedactionPageStatus};

fn with_redaction() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(AnnotationConfig::default()),
        search: Some(SearchConfig::default()),
        redaction: Some(RedactionConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_native_viewer_can_apply() {
    let host = with_redaction();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let redaction = block_on(host.redaction(doc.id)).unwrap();
    assert!(block_on(redaction.can_apply()).unwrap());
    assert!(block_on(redaction.can_mark()).unwrap());
}

#[test]
fn applying_a_page_removes_its_marked_text() {
    let host = with_redaction();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let search = block_on(host.search(doc.id.clone())).unwrap();
    let annotations = block_on(host.annotation(doc.id.clone())).unwrap();
    let redaction = block_on(host.redaction(doc.id)).unwrap();
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let query = || SearchQuery { text: "Report".into(), ..Default::default() };
    let before = block_on(search.find_all(query(), None, CancelToken::new())).unwrap();
    assert!(!before.is_empty(), "report.pdf contains \"Report\"");
    let page = before[0].page;
    let marks =
        block_on(redaction.mark_matches(query(), Some(vec![page]), CancelToken::new())).unwrap();
    assert!(!marks.is_empty());
    assert_eq!(block_on(redaction.pending(Some(page))).unwrap().len(), marks.len());
    let result = block_on(redaction.apply_pages(vec![page], CancelToken::new())).unwrap();
    assert_eq!(result.results[0].page, page);
    assert_eq!(result.results[0].status, RedactionPageStatus::Applied);
    let after = block_on(search.find_all(query(), None, CancelToken::new())).unwrap();
    assert!(after.iter().all(|hit| hit.page != page), "{after:?}");
    assert!(block_on(redaction.pending(Some(page))).unwrap().is_empty());
}

#[test]
fn without_its_config_the_redaction_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.redaction(doc.id)).is_err());
}

use epdf_plugin_measurement::ScaleChangeOptions;
use epdf_plugin_measurement::ffi::{
    MeasurementConfig, MeasurementPageTarget, MeasurementReading, MeasurementScaleSource,
};

fn with_measurement() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(AnnotationConfig::default()),
        measurement: Some(MeasurementConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn a_measurement_scale_persists_through_the_native_engine() {
    let host = with_measurement();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let m = block_on(host.measurement(doc.id.clone())).unwrap();
    block_on(m.ensure_loaded(page, CancelToken::new())).unwrap();
    let before = block_on(m.page_scale(page)).unwrap();
    assert!(before.persistent, "the native engine answers page.measure");
    assert_eq!(before.source, MeasurementScaleSource::Default);
    let preset = block_on(m.presets()).unwrap()[0].id.clone();
    let reports = block_on(m.set_preset(
        MeasurementPageTarget::Page { page },
        preset,
        ScaleChangeOptions::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(block_on(m.page_scale(page)).unwrap().source, MeasurementScaleSource::Owned);
    let reading = block_on(m.measure_distance(
        page,
        epdf_ffi::Point { x: 0.0, y: 0.0 },
        epdf_ffi::Point { x: 72.0, y: 0.0 },
    ))
    .unwrap();
    assert!(matches!(reading, MeasurementReading::Value { .. }), "{reading:?}");
}

#[test]
fn a_foreign_scale_is_read_and_kept_through_the_native_engine() {
    let host = with_measurement();
    let doc =
        block_on(host.open_document(
            test_support::fixture(test_support::MEASURE_FOREIGN_VIEWPORT),
            options(),
        ))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let m = block_on(host.measurement(doc.id.clone())).unwrap();
    block_on(m.ensure_loaded(page, CancelToken::new())).unwrap();
    let foreign = block_on(m.page_scale(page)).unwrap();
    assert_eq!(foreign.source, MeasurementScaleSource::Foreign);
    assert_eq!(foreign.measure.unwrap().ratio.as_deref(), Some("1 in = 1 m"));
    let preset = block_on(m.presets()).unwrap()[0].id.clone();
    block_on(m.set_preset(
        MeasurementPageTarget::Page { page },
        preset,
        ScaleChangeOptions::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(block_on(m.page_scale(page)).unwrap().source, MeasurementScaleSource::Owned);
    block_on(m.clear_scale(
        MeasurementPageTarget::Page { page },
        ScaleChangeOptions::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(block_on(m.page_scale(page)).unwrap().source, MeasurementScaleSource::Foreign);
}

#[test]
fn without_its_config_the_measurement_plugin_is_not_installed() {
    let host = with_annotation();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.measurement(doc.id)).is_err());
}

use epdf_plugin_stamp::ffi::{StampAddAssetInput, StampAssetFilter, StampConfig};

fn with_stamp() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        interaction: true,
        annotation: Some(AnnotationConfig::default()),
        stamp: Some(StampConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

fn stamps_pdf() -> Vec<u8> {
    std::fs::read(test_support::workspace_root().join("packages/default-stamps/en/stamps.pdf"))
        .unwrap()
}

/// A store in memory, for persistence over a real engine.
#[derive(Default)]
struct MemoryStampStore(Mutex<Vec<epdf_plugin_stamp::ffi::StoredStampLibrary>>);

impl epdf_plugin_stamp::ffi::StampLibraryStore for MemoryStampStore {
    fn list(&self) -> Result<Vec<epdf_plugin_stamp::ffi::StoredStampLibrary>, epdf_ffi::HostError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn put(&self, id: String, bytes: Vec<u8>) -> Result<(), epdf_ffi::HostError> {
        let mut rows = self.0.lock().unwrap();
        rows.retain(|r| r.id != id);
        rows.push(epdf_plugin_stamp::ffi::StoredStampLibrary { id, bytes });
        Ok(())
    }
    fn delete(&self, id: String) -> Result<(), epdf_ffi::HostError> {
        self.0.lock().unwrap().retain(|r| r.id != id);
        Ok(())
    }
}

/// Wait up to two seconds for `store` to hold `rows` libraries.
fn stored(store: &MemoryStampStore, rows: usize) -> usize {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let n = store.0.lock().unwrap().len();
        if n == rows || Instant::now() > deadline {
            return n;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_dropped_persist_registration_keeps_writing_until_unregistered() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let store = Arc::new(MemoryStampStore::default());
    let registration = block_on(stamp.persist(store.clone(), Vec::new(), Some(0.0))).unwrap();
    let unregister = {
        let registration = registration.clone();
        move || registration.unregister()
    };
    // The bug this pins: the old `StampPersistence` stopped when dropped.
    drop(registration);
    block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new())).unwrap();
    assert_eq!(stored(&store, 1), 1, "a dropped registration stopped persisting");
    unregister();
    block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new())).unwrap();
    assert_eq!(block_on(stamp.libraries(None)).unwrap().len(), 2);
    assert_eq!(stored(&store, 2), 1, "an unregistered persist kept writing");
}

#[test]
fn a_stamp_library_imports_and_places_over_pdfium() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let library =
        block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new()))
            .unwrap();
    let filter = StampAssetFilter { library_id: Some(library.clone()), ..Default::default() };
    let assets = block_on(stamp.assets(Some(filter))).unwrap();
    assert!(!assets.is_empty());
    assert_eq!(block_on(stamp.library(library)).unwrap().unwrap().asset_ids.len(), assets.len());
    let preview = block_on(stamp.asset_preview(assets[0].id.clone())).unwrap().unwrap();
    assert_eq!(preview.mime_type.as_deref(), Some("image/png"));
    assert!(preview.bytes.unwrap().starts_with(b"\x89PNG"));
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let at = epdf_ffi::Point { x: 100.0, y: 100.0 };
    let placed = block_on(stamp.place_asset(
        doc.id.clone(),
        assets[0].id.clone(),
        page,
        at,
        None,
        None,
        CancelToken::new(),
    ))
    .unwrap();
    let annotations = block_on(host.annotation(doc.id)).unwrap();
    assert!(block_on(annotations.get(placed)).unwrap().is_some());
}

#[test]
fn a_plain_pdf_imports_one_asset_per_page() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let report = test_support::fixture(test_support::REPORT);
    let library =
        block_on(stamp.import_library(report, Default::default(), CancelToken::new())).unwrap();
    let filter = StampAssetFilter { library_id: Some(library), ..Default::default() };
    let assets = block_on(stamp.assets(Some(filter))).unwrap();
    // report.pdf has 14 pages (`a_document_opens_and_lists_its_pages`).
    assert_eq!(assets.len(), 14);
    assert_eq!((assets[0].name.as_str(), assets[0].label.as_str()), ("Stamp1", "Stamp1"));
}

#[test]
fn importing_bytes_that_are_not_a_pdf_is_invalid_input() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    let error =
        block_on(stamp.import_library(png, Default::default(), CancelToken::new())).unwrap_err();
    match error {
        // The engine's `InvalidArg`, as `toPluginError` codes it.
        ViewerError::Kernel { code, .. } => assert_eq!(code.as_deref(), Some("invalid-input")),
        other => panic!("{other:?}"),
    }
    assert!(block_on(stamp.libraries(None)).unwrap().is_empty());
}

#[test]
fn a_library_created_natively_takes_a_pdf_asset() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let library =
        block_on(stamp.create_library("Mine".into(), Default::default(), CancelToken::new()))
            .unwrap();
    let input = StampAddAssetInput {
        library_id: Some(library.clone()),
        label: Some("Colors".into()),
        source: Some(test_support::fixture(test_support::COLORS)),
        ..Default::default()
    };
    let asset = block_on(stamp.create_asset(input, CancelToken::new())).unwrap();
    assert!(block_on(stamp.read_asset_bytes(asset)).unwrap().unwrap().starts_with(b"%PDF"));
    let exported = block_on(stamp.export_library(library, CancelToken::new())).unwrap();
    assert!(exported.starts_with(b"%PDF"));
}

#[test]
fn an_authored_mark_needs_an_appearance_export_the_native_engine_lacks() {
    // Ink and text marks are drawn as annotations and exported through
    // `page.annotations.exportAppearance`, which `epdf-engine` doesn't have.
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let library =
        block_on(stamp.create_library("Mine".into(), Default::default(), CancelToken::new()))
            .unwrap();
    let stroke = vec![epdf_ffi::Point { x: 0.0, y: 0.0 }, epdf_ffi::Point { x: 40.0, y: 20.0 }];
    let input = StampAddAssetInput {
        library_id: Some(library),
        mark: Some(epdf_plugin_stamp::ffi::StampMark::Ink {
            strokes: vec![stroke],
            color: None,
            stroke_width: None,
        }),
        ..Default::default()
    };
    match block_on(stamp.create_asset(input, CancelToken::new())).unwrap_err() {
        // The engine's `NotImplemented`.
        ViewerError::Kernel { code, .. } => assert_eq!(code.as_deref(), Some("unsupported")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn create_asset_from_annotations_is_unsupported_natively() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let error = block_on(stamp.create_asset_from_annotations(
        doc.id,
        page,
        Vec::new(),
        StampAddAssetInput::default(),
        CancelToken::new(),
    ))
    .unwrap_err();
    match error {
        ViewerError::Kernel { code, .. } => assert_eq!(code.as_deref(), Some("unsupported")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn dropping_the_host_mid_import_joins_the_asset_engine_thread() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    // The asset engine starts on the first open.
    block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new())).unwrap();
    let assets = block_on(host.asset_engine_thread_witness()).unwrap().unwrap();
    assert!(assets.upgrade().is_some());
    // Started (the job is posted on the first poll) and never finished.
    let mut importing =
        Box::pin(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new()));
    assert!((&mut importing).now_or_never().is_none());
    drop(host);
    assert!(assets.upgrade().is_none(), "the asset engine thread outlived its host");
    drop(importing);
}

#[test]
fn placing_into_a_closed_document_fails_and_clears_its_arming() {
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    let library =
        block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new()))
            .unwrap();
    let filter = StampAssetFilter { library_id: Some(library), ..Default::default() };
    let asset = block_on(stamp.assets(Some(filter))).unwrap()[0].id.clone();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    block_on(stamp.arm_asset(doc.id.clone(), asset.clone(), None, CancelToken::new())).unwrap();
    assert!(block_on(stamp.armed_asset(doc.id.clone())).unwrap().is_some());
    block_on(host.close_document(doc.id.clone())).unwrap();
    let at = epdf_ffi::Point { x: 10.0, y: 10.0 };
    let error = block_on(stamp.place_asset(
        doc.id.clone(),
        asset,
        page,
        at,
        None,
        None,
        CancelToken::new(),
    ))
    .unwrap_err();
    match error {
        // The kernel's plain error for a token on a document it no longer
        // has, as the TypeScript kernel throws it, which `toPluginError`
        // makes `operation-failed`.
        ViewerError::Kernel { name, code, detail } => {
            assert_eq!((name.as_str(), code.as_deref()), ("Error", Some("operation-failed")));
            assert!(detail.contains("no document"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
    assert!(block_on(stamp.armed_asset(doc.id)).unwrap().is_none());
}

#[test]
fn a_library_edit_leaves_an_open_documents_pages_alone() {
    // The asset engine shares the kernel thread with the viewer's engine; its
    // documents must not be mistaken for the viewer's.
    let host = with_stamp();
    let stamp = block_on(host.stamp()).unwrap();
    // Two open documents, so the import and the delete (each opening a
    // library document of its own) both have a viewer document to collide with.
    let _first =
        block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
            .unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let before = block_on(host.pages(doc.id.clone())).unwrap();
    let library =
        block_on(stamp.import_library(stamps_pdf(), Default::default(), CancelToken::new()))
            .unwrap();
    let filter = StampAssetFilter { library_id: Some(library), ..Default::default() };
    let asset = block_on(stamp.assets(Some(filter))).unwrap()[0].id.clone();
    block_on(stamp.delete_asset(asset, CancelToken::new())).unwrap();
    assert_eq!(block_on(host.pages(doc.id)).unwrap(), before);
}

#[test]
fn without_its_config_the_stamp_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.stamp()).is_err());
}

fn with_page_edit() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        page_edit: true,
        render: Some(RenderConfig::default()),
        ..Default::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn rotating_twice_reads_the_rotation_the_first_edit_left() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let first = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    block_on(edit.rotate_by(vec![first], PageEditTurn::Clockwise, CancelToken::new())).unwrap();
    let layout =
        block_on(edit.rotate_by(vec![first], PageEditTurn::Clockwise, CancelToken::new())).unwrap();
    assert_eq!(layout.pages[0].rotation, 180);
    assert_eq!(block_on(host.pages(doc.id)).unwrap()[0].rotation, 180);
}

#[test]
fn an_inserted_blank_page_renders() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let first = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let options = PageEditBlankOptions {
        placement: Some(PageEditPlacement::After { page: first }),
        ..Default::default()
    };
    let insertion = block_on(edit.insert_blank(options, CancelToken::new())).unwrap();
    assert_eq!(insertion.pages.len(), 15);
    assert_eq!(insertion.pages[1].page, insertion.inserted[0]);
    let render = block_on(host.render(doc.id)).unwrap();
    let image = block_on(render.render_page(
        insertion.inserted[0],
        RenderPageOptions::default(),
        CancelToken::new(),
    ))
    .unwrap();
    assert!(image.width > 0 && image.height > 0);
}

#[test]
fn extracting_and_duplicating_leave_nothing_in_the_host_table() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let before = block_on(host.host_table_entries()).unwrap();
    let pages: Vec<PageRef> =
        block_on(host.pages(doc.id.clone())).unwrap().iter().take(2).map(|p| p.page).collect();
    let pdf = block_on(edit.extract(pages.clone(), CancelToken::new())).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let insertion = block_on(edit.duplicate(pages, None, CancelToken::new())).unwrap();
    assert_eq!(insertion.inserted.len(), 2);
    assert_eq!(block_on(host.host_table_entries()).unwrap(), before);
}

#[test]
fn a_subset_of_another_pdf_inserts_through_a_second_open() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let options = epdf_plugin_page_edit::ffi::PageEditBytesOptions {
        page_indexes: Some(vec![0]),
        placement: Some(PageEditPlacement::Index { index: 0 }),
    };
    let insertion = block_on(edit.insert_from_bytes(
        test_support::fixture(test_support::REPORT),
        options,
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(insertion.inserted.len(), 1);
    assert_eq!(insertion.pages[0].page, insertion.inserted[0]);
}

#[test]
fn bytes_that_are_not_a_pdf_are_refused() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let error = block_on(edit.insert_from_bytes(
        b"not a pdf".to_vec(),
        Default::default(),
        CancelToken::new(),
    ))
    .unwrap_err();
    // `MalformedPdf`, as `PagesInserter.insert` refuses a source it can't
    // open (`runPageInsertConformance`).
    assert!(format!("{error:?}").contains("MalformedPdf"), "{error:?}");
    assert_eq!(block_on(host.pages(doc.id)).unwrap().len(), 14);
}

#[test]
fn without_its_flag_the_page_edit_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.page_edit(doc.id)).is_err());
}

#[test]
fn a_duplicate_with_an_unknown_anchor_leaves_nothing_in_the_host_table() {
    let host = with_page_edit();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let edit = block_on(host.page_edit(doc.id.clone())).unwrap();
    let before = block_on(host.host_table_entries()).unwrap();
    let first = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    let unknown = PageEditPlacement::After { page: PageRef { page_object_number: 9999 } };
    assert!(block_on(edit.duplicate(vec![first], Some(unknown), CancelToken::new())).is_err());
    assert_eq!(block_on(host.host_table_entries()).unwrap(), before);
}

fn with_view_manager() -> Arc<ViewerHost> {
    let config = ViewerConfig { view_manager: true, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_first_open_creates_the_default_pane() {
    let host = with_view_manager();
    let vm = block_on(host.view_manager()).unwrap();
    assert!(block_on(vm.panes()).unwrap().is_empty(), "no pane before a document opens");
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let panes = block_on(vm.panes()).unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].id, "pane-1");
    assert_eq!(panes[0].document_ids, [doc.id.as_str()]);
    assert_eq!(block_on(vm.focused_pane_id()).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn a_second_document_splits_into_its_own_pane() {
    let host = with_view_manager();
    let vm = block_on(host.view_manager()).unwrap();
    let report =
        block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
            .unwrap();
    let colors =
        block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
            .unwrap();
    let pane = block_on(vm.split_pane(colors.id.clone())).unwrap();
    assert_eq!(block_on(vm.pane_of_document(colors.id)).unwrap(), Some(pane));
    assert_eq!(block_on(vm.pane_of_document(report.id)).unwrap().as_deref(), Some("pane-1"));
}

#[test]
fn without_its_flag_the_view_manager_is_not_installed() {
    let host = started();
    assert!(block_on(host.view_manager()).is_err());
}

fn with_shell() -> Arc<ViewerHost> {
    let config = ViewerConfig { shell: true, ..Default::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn each_document_keeps_its_own_shell() {
    let host = with_shell();
    let report =
        block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
            .unwrap();
    let colors =
        block_on(host.open_document(test_support::fixture(test_support::COLORS), options()))
            .unwrap();
    let first = block_on(host.shell(report.id.clone())).unwrap();
    let second = block_on(host.shell(colors.id)).unwrap();
    let options =
        ShellOpenOptions { exclusive: None, props_json: Some(r#"{"tab":"outline"}"#.into()) };
    block_on(first.open_surface("sidebar".into(), Some(options))).unwrap();
    assert!(block_on(first.is_open("sidebar".into())).unwrap());
    assert!(!block_on(second.is_open("sidebar".into())).unwrap());
    block_on(host.close_document(report.id)).unwrap();
    assert!(block_on(second.open_surfaces()).unwrap().is_empty());
}

#[test]
fn without_its_flag_the_shell_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.shell(doc.id)).is_err());
}

fn en_pack() -> LocalePack {
    LocalePack {
        code: "en".into(),
        name: "English".into(),
        dir: None,
        translations: [("hello".to_string(), "Hello".to_string())].into(),
    }
}

#[test]
fn the_i18n_plugin_translates_before_any_document_opens() {
    let config = ViewerConfig {
        i18n: Some(I18nConfig {
            locale: None,
            fallback_locale: None,
            locales: vec![en_pack()],
            lazy_locales: vec![],
            loader: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let i18n = block_on(host.i18n()).unwrap();
    assert_eq!(block_on(i18n.t("hello".into(), Default::default(), None)).unwrap(), "Hello");
}

#[test]
fn without_its_config_the_i18n_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.i18n()).is_err());
}

#[test]
fn lazy_locales_without_a_loader_fail_to_build() {
    let config = ViewerConfig {
        i18n: Some(I18nConfig {
            locale: None,
            fallback_locale: None,
            locales: vec![en_pack()],
            lazy_locales: vec!["es".into()],
            loader: None,
        }),
        ..ViewerConfig::default()
    };
    let error = ViewerHost::new(config, Arc::new(Delegate)).err().unwrap();
    assert!(
        matches!(&error, ViewerError::Kernel { code: Some(code), .. } if code == "invalid-input"),
        "{error:?}"
    );
}

fn routing_command(id: &str, label_key: &str) -> CommandDefinition {
    CommandDefinition {
        id: id.into(),
        label_key: label_key.into(),
        icon: None,
        shortcuts: vec![],
        categories: vec![],
        menu: None,
        panel: None,
        modal: None,
        runner: None,
        hooks: vec![],
    }
}

#[test]
fn the_commands_plugin_registers_and_resolves() {
    let config = ViewerConfig { commands: true, ..ViewerConfig::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let commands = block_on(host.commands()).unwrap();
    block_on(commands.register_command(routing_command("zoom:in", "commands.zoomIn"), false))
        .unwrap();
    let resolved = block_on(commands.resolve_command("zoom:in".into(), None)).unwrap().unwrap();
    // No i18n: the label is the key.
    assert_eq!(resolved.label, "commands.zoomIn");
    let result = block_on(commands.execute("zoom:in".into(), None, None)).unwrap();
    assert_eq!(result, CommandExecuteResult::Executed);
}

#[test]
fn without_its_flag_the_commands_plugin_is_not_installed() {
    let host = started();
    assert!(block_on(host.commands()).is_err());
}

#[test]
fn a_command_label_resolves_through_i18n() {
    let pack = LocalePack {
        code: "en".into(),
        name: "English".into(),
        dir: None,
        translations: [("commands.zoomIn".to_string(), "Zoom in".to_string())].into(),
    };
    let config = ViewerConfig {
        commands: true,
        i18n: Some(I18nConfig {
            locale: None,
            fallback_locale: None,
            locales: vec![pack],
            lazy_locales: vec![],
            loader: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let commands = block_on(host.commands()).unwrap();
    block_on(commands.register_command(routing_command("zoom:in", "commands.zoomIn"), false))
        .unwrap();
    let resolved = block_on(commands.resolve_command("zoom:in".into(), None)).unwrap().unwrap();
    assert_eq!(resolved.label, "Zoom in");
}

#[test]
fn a_panel_command_toggles_the_shell_surface() {
    let config = ViewerConfig { commands: true, shell: true, ..ViewerConfig::default() };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let commands = block_on(host.commands()).unwrap();
    let mut toggle = routing_command("panel:search", "commands.search");
    toggle.panel = Some(CommandPanel { id: "search".into(), exclusive: None });
    block_on(commands.register_command(toggle, false)).unwrap();
    block_on(commands.execute("panel:search".into(), Some(doc.id.clone()), None)).unwrap();
    let shell = block_on(host.shell(doc.id)).unwrap();
    assert!(block_on(shell.is_open("search".into())).unwrap());
}

const DOCUMENT_ACTIONS: &str = "tooling/engine-conformance/fixtures/document_actions.pdf";

fn with_actions() -> Arc<ViewerHost> {
    let config = ViewerConfig {
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: None,
            utc_offset_minutes: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    host
}

#[test]
fn the_actions_plugin_reads_the_catalog_through_the_native_engine() {
    let host = with_actions();
    let doc =
        block_on(host.open_document(test_support::fixture(DOCUMENT_ACTIONS), options())).unwrap();
    let actions = block_on(host.actions(doc.id)).unwrap();
    let open = block_on(actions.action_tree(ActionTreeSource::Document { event: None })).unwrap();
    assert_eq!(open.and_then(|t| t.root_type()).as_deref(), Some("javascript"));
    assert!(!block_on(actions.is_scripting_enabled()).unwrap());
    // Scripting is off, so the will-close script is recognized and inert.
    let result = block_on(
        actions.dispatch(ActionTrigger::Document { event: DocumentTriggerEvent::WillClose }),
    )
    .unwrap();
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].result.nodes[0].status, ActionNodeStatus::Inert);
}

#[test]
fn without_its_config_the_actions_plugin_is_not_installed() {
    let host = started();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    assert!(block_on(host.actions(doc.id)).is_err());
}

struct ProbeOnly;

impl ScriptEngine for ProbeOnly {
    fn evaluate(&self, _program: String, _limit: u32) -> ScriptEvaluation {
        ScriptEvaluation::Value { json: "\"function\"".into() }
    }
    fn dispose(&self) {}
}

impl ScriptEngineFactory for ProbeOnly {
    fn create(&self) -> Result<Arc<dyn ScriptEngine>, epdf_ffi::HostError> {
        Ok(Arc::new(ProbeOnly))
    }
}

#[test]
fn an_engine_turns_scripting_on() {
    let config = ViewerConfig {
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: Some(ActionsJavaScriptConfig {
                enabled: true,
                engine: Some(Arc::new(ProbeOnly)),
                budget: None,
                max_script_nodes_per_dispatch: None,
            }),
            utc_offset_minutes: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc =
        block_on(host.open_document(test_support::fixture(DOCUMENT_ACTIONS), options())).unwrap();
    let actions = block_on(host.actions(doc.id)).unwrap();
    assert!(block_on(actions.is_scripting_enabled()).unwrap());
}

/// Plan 15's cross-plugin test with the stage, which merged after it: the
/// stage registers the `NextPage` verb with actions.
#[test]
fn executing_a_named_next_page_action_moves_the_stage() {
    let config = ViewerConfig {
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: None,
            utc_offset_minutes: None,
        }),
        ..ViewerConfig::default()
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(test_support::fixture(test_support::REPORT), options()))
        .unwrap();
    let stage = block_on(host.stage(doc.id.clone())).unwrap();
    block_on(stage.set_viewport_size(FfiSize { width: 390.0, height: 844.0 })).unwrap();
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 0);
    let actions = block_on(host.actions(doc.id)).unwrap();
    let result = block_on(actions.execute_named("NextPage".into(), None)).unwrap();
    assert_eq!(result.nodes[0].status, ActionNodeStatus::Executed, "{result:?}");
    assert_eq!(block_on(stage.current_page_index()).unwrap(), 1);
}

// ── 16 signature ───────────────────────────────────────────────────────────

mod common;

use epdf_plugin_page_edit::ffi::PageEditTurn as SignatureTestTurn;
use epdf_plugin_signature::ffi::{
    SignatureConfig, SignatureIntegrity, SignatureMark, SignatureMode, SignatureModificationsBasis,
    SignatureModificationsVerdict, SignaturePlaceResult, SignaturePlaceTarget, SignatureSignInput,
    SignatureSummary, SignatureTrustStatus, SignatureValidateRequest,
};

const SIGNING: &str = "packages/engine/main/test/fixtures";

fn signature_host_with(scope: Option<Vec<String>>) -> Arc<ViewerHost> {
    let host = ViewerHost::new(
        ViewerConfig {
            scope,
            interaction: true,
            annotation: Some(Default::default()),
            form: Some(Default::default()),
            page_edit: true,
            signature: Some(SignatureConfig::default()),
            ..Default::default()
        },
        Arc::new(Delegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    host
}

fn signature_host() -> Arc<ViewerHost> {
    signature_host_with(None)
}

fn open_signing(host: &ViewerHost, name: &str) -> String {
    let bytes = test_support::fixture(&format!("{SIGNING}/{name}"));
    block_on(host.open_document(bytes, options())).unwrap().id
}

fn sign_input(field: epdf_ffi::FormFieldRef) -> SignatureSignInput {
    SignatureSignInput { field, mark: None, attribution: None, certify: None, lock: None }
}

#[test]
fn a_document_signs_and_validates_over_pdfium() {
    let host = signature_host();
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(false))));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let done = block_on(s.sign(sign_input(field.clone()), None, CancelToken::new())).unwrap();
    assert!(done.signature.signed);
    assert_eq!(block_on(s.pending()).unwrap(), None);
    let verdict =
        block_on(s.validate_field(field, SignatureValidateRequest::default(), CancelToken::new()))
            .unwrap();
    assert_eq!(verdict.integrity, SignatureIntegrity::Valid, "{verdict:?}");
    assert_eq!(verdict.summary, SignatureSummary::ValidUntrusted, "{verdict:?}");
    assert!(verdict.signature.signer.name.is_some());
}

#[test]
fn the_test_certificate_as_an_anchor_is_trusted() {
    let host = signature_host();
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(false))));
    host.set_signature_trust(Some(Arc::new(common::TestAnchors)));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    block_on(s.sign(sign_input(field.clone()), None, CancelToken::new())).unwrap();
    let verdict =
        block_on(s.validate_field(field, SignatureValidateRequest::default(), CancelToken::new()))
            .unwrap();
    assert_eq!(verdict.trust, SignatureTrustStatus::Trusted, "{verdict:?}");
    assert_eq!(verdict.summary, SignatureSummary::Valid, "{verdict:?}");
}

#[test]
fn without_anchors_a_good_signature_is_valid_untrusted() {
    let host = signature_host();
    let doc = open_signing(&host, "signed_two_approvals.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let verdicts =
        block_on(s.validate(SignatureValidateRequest::default(), CancelToken::new())).unwrap();
    assert_eq!(verdicts.len(), 2);
    assert!(verdicts.iter().all(|v| v.summary != SignatureSummary::Valid), "{verdicts:?}");
}

#[test]
fn a_changed_document_validates_invalid() {
    let host = signature_host();
    let doc = open_signing(&host, "signed_two_approvals.pdf");
    let page = block_on(host.pages(doc.clone())).unwrap()[0].page;
    let edit = block_on(host.page_edit(doc.clone())).unwrap();
    block_on(edit.rotate_by(vec![page], SignatureTestTurn::Clockwise, CancelToken::new())).unwrap();
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let request = SignatureValidateRequest {
        at: None,
        until: Some(SignatureModificationsBasis::WorkingCopy),
    };
    let verdicts = block_on(s.validate(request, CancelToken::new())).unwrap();
    assert!(
        verdicts
            .iter()
            .any(|v| v.modifications.verdict == SignatureModificationsVerdict::Forbidden),
        "{verdicts:?}"
    );
    assert!(verdicts.iter().all(|v| v.summary != SignatureSummary::Valid));
    assert!(verdicts.iter().any(|v| v.summary == SignatureSummary::Invalid));
}

#[test]
fn a_throwing_signer_leaves_nothing_pending_over_pdfium() {
    let host = signature_host();
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(true))));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let error = block_on(s.sign(sign_input(field), None, CancelToken::new())).unwrap_err();
    assert!(format!("{error:?}").contains("operation-cancelled"), "{error:?}");
    assert_eq!(block_on(s.pending()).unwrap(), None);
    // The document is not left read-only: another signing can prepare.
    assert_eq!(block_on(s.unsigned_fields()).unwrap().len(), 1);
}

#[test]
fn a_pending_signature_does_not_block_the_kernel_thread() {
    let host = signature_host();
    let (held, signing) = mpsc::channel();
    host.set_signature_signer(Some(Arc::new(common::HeldSigner {
        signer: common::TestSigner::load(false),
        held,
    })));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc.clone())).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let signed = {
        let s = s.clone();
        std::thread::spawn(move || block_on(s.sign(sign_input(field), None, CancelToken::new())))
    };
    let (message, completion) = signing.recv_timeout(Duration::from_secs(5)).unwrap();
    // The signer has not answered, and the kernel still serves other calls.
    assert_eq!(block_on(host.pages(doc)).unwrap().len(), 1);
    assert!(!signed.is_finished(), "the signing finished without its signature");
    let signature = common::TestSigner::load(false).raw_signature(&message);
    std::thread::spawn(move || completion.finish(signature)).join().unwrap();
    let done = signed.join().unwrap().unwrap();
    assert!(done.signature.signed);
    assert_eq!(block_on(s.pending()).unwrap(), None);
}

#[test]
fn a_signer_completion_dropped_unanswered_fails_the_signing() {
    let host = signature_host();
    let (held, signing) = mpsc::channel();
    host.set_signature_signer(Some(Arc::new(common::HeldSigner {
        signer: common::TestSigner::load(false),
        held,
    })));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let signed = {
        let s = s.clone();
        std::thread::spawn(move || block_on(s.sign(sign_input(field), None, CancelToken::new())))
    };
    drop(signing.recv_timeout(Duration::from_secs(5)).unwrap());
    let error = signed.join().unwrap().unwrap_err();
    assert!(format!("{error:?}").contains("operation-failed"), "{error:?}");
    assert_eq!(block_on(s.pending()).unwrap(), None);
}

#[test]
fn can_sign_follows_the_host_signer() {
    let host = signature_host();
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    assert!(!block_on(s.can_sign()).unwrap());
    assert_eq!(block_on(s.mode()).unwrap(), SignatureMode::Visual);
    assert!(block_on(s.can_fill()).unwrap());
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(false))));
    assert!(block_on(s.can_sign()).unwrap());
    assert_eq!(block_on(s.mode()).unwrap(), SignatureMode::Sign);
    host.set_signature_signer(None);
    assert!(!block_on(s.can_sign()).unwrap());
}

#[test]
fn a_narrow_scope_cannot_sign_or_fill() {
    let host = signature_host_with(Some(vec!["doc.open".into(), "doc.render".into()]));
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(false))));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    assert!(!block_on(s.can_sign()).unwrap());
    assert!(!block_on(s.can_fill()).unwrap());
}

#[test]
fn a_visual_fill_needs_no_signer() {
    let host = signature_host();
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let artwork = test_support::fixture(&format!("{SIGNING}/signature_artwork.pdf"));
    block_on(s.fill_field(field, SignatureMark::Source { bytes: artwork }, CancelToken::new()))
        .unwrap();
}

#[test]
fn a_free_mark_is_a_stamp_annotation() {
    let host = signature_host();
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let page = block_on(host.pages(doc.clone())).unwrap()[0].page;
    let s = block_on(host.signature(doc)).unwrap();
    let artwork = test_support::fixture(&format!("{SIGNING}/signature_artwork.pdf"));
    let target = SignaturePlaceTarget::Free {
        page,
        at: epdf_ffi::Point { x: 100.0, y: 100.0 },
        target_width: None,
        rotation: None,
    };
    let result = block_on(s.place_mark(
        SignatureMark::Source { bytes: artwork },
        target,
        CancelToken::new(),
    ))
    .unwrap();
    assert!(matches!(result, SignaturePlaceResult::Placed { .. }), "{result:?}");
}

#[test]
fn signature_without_form_fails_to_build() {
    let built = ViewerHost::new(
        ViewerConfig { signature: Some(SignatureConfig::default()), ..Default::default() },
        Arc::new(Delegate),
    );
    assert!(built.is_err());
}

#[test]
fn a_certified_document_refuses_edits_natively() {
    let host = signature_host();
    let doc = open_signing(&host, "signed_certified.pdf");
    // P=2: form fill and signing only, whatever the scope grants.
    let edit = block_on(host.page_edit(doc.clone())).unwrap();
    assert!(!block_on(edit.can_edit()).unwrap());
    let annotations = block_on(host.annotation(doc.clone())).unwrap();
    assert!(!block_on(annotations.can_create()).unwrap());
    let s = block_on(host.signature(doc)).unwrap();
    assert!(block_on(s.can_fill()).unwrap());
}

#[test]
fn a_certification_signed_here_protects_the_document() {
    let host = ViewerHost::new(
        ViewerConfig {
            interaction: true,
            form: Some(Default::default()),
            page_edit: true,
            signature: Some(SignatureConfig { mode: None, allow_certify: Some(true) }),
            ..Default::default()
        },
        Arc::new(Delegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    host.set_signature_signer(Some(Arc::new(common::TestSigner::load(false))));
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let edit = block_on(host.page_edit(doc.clone())).unwrap();
    assert!(block_on(edit.can_edit()).unwrap());
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let input = SignatureSignInput { certify: Some(1), ..sign_input(field) };
    block_on(s.sign(input, None, CancelToken::new())).unwrap();
    assert!(!block_on(edit.can_edit()).unwrap());
}

#[test]
fn placing_a_mark_without_a_signer_fills_the_field() {
    let host = signature_host();
    let doc = open_signing(&host, "unsigned_sigfield.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    assert_eq!(block_on(s.mode()).unwrap(), SignatureMode::Visual);
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let artwork = test_support::fixture(&format!("{SIGNING}/signature_artwork.pdf"));
    let result = block_on(s.place_mark(
        SignatureMark::Source { bytes: artwork },
        SignaturePlaceTarget::Field { field: field.clone() },
        CancelToken::new(),
    ))
    .unwrap();
    assert_eq!(result, SignaturePlaceResult::Filled { field });
}

#[test]
fn a_narrow_scope_cannot_read_a_revision() {
    let host = signature_host_with(Some(vec![
        "doc.open".into(),
        "doc.render".into(),
        "doc.forms.read".into(),
    ]));
    let doc = open_signing(&host, "signed_two_approvals.pdf");
    let s = block_on(host.signature(doc)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.signatures()).unwrap()[0].field.clone();
    assert!(block_on(s.read_revision(field, CancelToken::new())).is_err());
}

// 17 close-out.

/// Every `ViewerConfig` field set at once: the plugins install together,
/// a document opens, and each plugin's handle resolves for it.
#[test]
fn every_plugin_builds_together() {
    let ticks = Arc::new(Ticks::default());
    let config = ViewerConfig {
        scope: None,
        metadata: true,
        search: Some(SearchConfig::default()),
        render: Some(RenderConfig::default()),
        interaction: true,
        feedback: Some(FeedbackConfig::new(ticks)),
        stage: Some(StageConfig { frames: false, ..Default::default() }),
        selection: true,
        link: true,
        annotation: Some(AnnotationConfig::default()),
        form: Some(FormConfig { validation: Some(FormValidation::None) }),
        redaction: Some(RedactionConfig::default()),
        measurement: Some(MeasurementConfig::default()),
        stamp: Some(StampConfig::default()),
        page_edit: true,
        view_manager: true,
        shell: true,
        i18n: Some(I18nConfig {
            locale: None,
            fallback_locale: None,
            locales: vec![en_pack()],
            lazy_locales: vec![],
            loader: None,
        }),
        commands: true,
        actions: Some(epdf_plugin_actions::ffi::ActionsConfig {
            policy: None,
            triggers: None,
            open_sequence: Some(OpenSequence::Off),
            javascript: None,
            utc_offset_minutes: None,
        }),
        signature: Some(SignatureConfig::default()),
    };
    let host = ViewerHost::new(config, Arc::new(Delegate)).unwrap();
    block_on(host.start()).unwrap();
    let bytes = test_support::fixture(&format!("{SIGNING}/unsigned_sigfield.pdf"));
    let doc = block_on(host.open_document(bytes, options())).unwrap().id;
    assert!(!block_on(host.pages(doc.clone())).unwrap().is_empty());
    block_on(host.metadata(doc.clone())).unwrap();
    block_on(host.search(doc.clone())).unwrap();
    block_on(host.render(doc.clone())).unwrap();
    block_on(host.interaction(doc.clone())).unwrap();
    block_on(host.feedback()).unwrap();
    block_on(host.stage(doc.clone())).unwrap();
    block_on(host.selection(doc.clone())).unwrap();
    block_on(host.link(doc.clone())).unwrap();
    block_on(host.annotation(doc.clone())).unwrap();
    block_on(host.form(doc.clone())).unwrap();
    block_on(host.redaction(doc.clone())).unwrap();
    block_on(host.measurement(doc.clone())).unwrap();
    block_on(host.stamp()).unwrap();
    block_on(host.page_edit(doc.clone())).unwrap();
    block_on(host.view_manager()).unwrap();
    block_on(host.shell(doc.clone())).unwrap();
    block_on(host.i18n()).unwrap();
    block_on(host.commands()).unwrap();
    block_on(host.actions(doc.clone())).unwrap();
    let s = block_on(host.signature(doc.clone())).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    assert!(!block_on(s.unsigned_fields()).unwrap().is_empty());
    block_on(host.close_document(doc)).unwrap();
}
