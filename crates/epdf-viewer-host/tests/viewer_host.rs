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
    DocumentEvent, DocumentEventListener, DocumentStatus, OpenDocumentOptions, ViewerConfig,
    ViewerDelegate, ViewerHost,
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
fn a_failed_open_lists_its_tab_as_failed_with_the_reason() {
    let host = started();
    let _ = block_on(host.open_document(test_support::fixture(test_support::ENCRYPTED), options()))
        .unwrap_err();
    let documents = block_on(host.documents()).unwrap();
    let [tab] = documents.as_slice() else { panic!("{documents:?}") };
    assert_eq!(tab.status, DocumentStatus::Failed, "{tab:?}");
    let error = tab.error.as_ref().expect("a failed tab without its error");
    // The kernel's `toPluginError` maps the engine's `DocPasswordRequired`
    // to `permission-denied` and keeps the engine code in the message.
    assert_eq!(error.code, "permission-denied", "{error:?}");
    assert!(error.message.starts_with("DocPasswordRequired"), "{error:?}");
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
    let doc = block_on(host.open_document(test_support::fixture(test_support::FEATURES), options()))
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
    assert!(format!("{error:?}").contains("DocOpenFailed"), "{error:?}");
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
