use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use epdf_ffi::{CancelToken, ResourceStatus, ViewerError};
use epdf_plugin_metadata::ffi::MetadataPatch;
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
