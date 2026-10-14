#![cfg(feature = "uniffi")]

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, PageRef, Point, ViewerError};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_selection::ffi::{
    SelectionChangeTrigger, SelectionEvent, SelectionEventListener, SelectionHandle,
    SelectionInput, SelectionTextPosition, SelectionTextRange,
};
use epdf_plugin_selection::{
    SELECTION, SelectionConfig, SelectionRangeInput, TextRange, selection_plugin,
};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{TestKernel, TestSpec};
use serde_json::Value;

const PAGE_1: PageRef = PageRef { page_object_number: 1 };

#[test]
fn a_range_and_a_span_round_trip() {
    let range = SelectionTextRange {
        start: SelectionTextPosition { page: PAGE_1, index: -3 },
        end: SelectionTextPosition { page: PAGE_1, index: 9_007_199_254_740_991 },
    };
    let core = TextRange::from(range);
    assert_eq!(core.start.index, -3);
    assert_eq!(SelectionTextRange::from(core), range);
    let span = SelectionInput::Span { page: PAGE_1, start: 2, count: 5 };
    assert!(matches!(
        SelectionRangeInput::from(span),
        SelectionRangeInput::Span { start: 2, count: 5, .. }
    ));
}

fn spec() -> Value {
    serde_json::from_str(include_str!("fixtures/selection-engine.json")).unwrap()
}

fn engine(allows: &str) -> TestSpec {
    // The fake engine reads `allows` as `"*"` or a list of features.
    let allows = if allows == "*" { serde_json::json!("*") } else { serde_json::json!([allows]) };
    let mut spec = spec();
    for doc in spec["documents"].as_object_mut().unwrap().values_mut() {
        doc["security"] = serde_json::json!({ "allows": allows.clone() });
    }
    TestSpec::new(spec)
}

/// The fixture document's pages, in order.
fn page_refs() -> Vec<PageRef> {
    let spec = spec();
    let doc = spec["documents"].as_object().unwrap().values().next().unwrap().clone();
    doc["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| PageRef {
            page_object_number: p["ref"]["pageObjectNumber"].as_u64().unwrap() as u32,
        })
        .collect()
}

#[derive(Default)]
struct Events(Mutex<Vec<SelectionEvent>>, AtomicUsize);

impl SelectionEventListener for Events {
    fn on_event(&self, event: SelectionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn handle_with(
    spec: impl FnOnce() -> TestSpec + Send + 'static,
) -> (KernelThread<TestKernel>, Arc<SelectionHandle>, Vec<PageRef>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                selection_plugin(SelectionConfig::default()),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| SelectionHandle::new(k.capability(&SELECTION).unwrap(), post)),
    )
    .unwrap();
    (thread, handle, page_refs())
}

fn handle() -> (KernelThread<TestKernel>, Arc<SelectionHandle>, Vec<PageRef>) {
    handle_with(|| engine("*"))
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn selecting_a_span_reads_its_text_and_geometry() {
    let (_thread, selection, pages) = handle();
    assert!(block_on(selection.can_select()).unwrap());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 4 }))
        .unwrap();
    assert!(block_on(selection.has_selection()).unwrap());
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    assert_eq!(text.encode_utf16().count(), 4, "{text:?}");
    assert!(!block_on(selection.rects(pages[0])).unwrap().is_empty());
    let snapshot = block_on(selection.snapshot()).unwrap();
    assert_eq!(snapshot.pages.len(), 1);
    assert!(snapshot.start.is_some() && snapshot.end.is_some());
    assert_eq!(block_on(selection.selected_pages()).unwrap(), [pages[0]]);
}

#[test]
fn a_span_past_the_end_clamps_to_the_page() {
    let (_thread, selection, pages) = handle();
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let page_text = block_on(selection.read_text(CancelToken::new())).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 1_000_000 }))
        .unwrap();
    assert_eq!(block_on(selection.read_text(CancelToken::new())).unwrap(), page_text);
}

#[test]
fn reading_text_loads_the_pages_it_needs() {
    let (_thread, selection, pages) = handle();
    assert!(!block_on(selection.is_loaded(pages[1])).unwrap());
    block_on(selection.select_all()).unwrap();
    let text = block_on(selection.read_text(CancelToken::new())).unwrap();
    assert!(text.contains('\n'), "pages join with a newline: {text:?}");
    assert!(!text.trim().is_empty());
}

#[test]
fn reading_a_range_leaves_the_selection_alone() {
    let (_thread, selection, pages) = handle();
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 2 }))
        .unwrap();
    let before = block_on(selection.range()).unwrap();
    let range = SelectionTextRange {
        start: SelectionTextPosition { page: pages[0], index: 0 },
        end: SelectionTextPosition { page: pages[0], index: 6 },
    };
    let text = block_on(selection.read_text_in_range(range, CancelToken::new())).unwrap();
    assert_eq!(text.encode_utf16().count(), 6);
    assert_eq!(block_on(selection.range()).unwrap(), before);
}

#[test]
fn without_copy_permission_reading_text_is_denied() {
    let (_thread, selection, pages) = handle_with(|| engine("doc.text.select"));
    assert!(block_on(selection.can_select()).unwrap());
    assert!(!block_on(selection.can_copy()).unwrap());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let error = block_on(selection.read_text(CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("permission-denied"), "{error:?}");
}

#[test]
fn a_cancelled_read_rejects_operation_cancelled() {
    let (_thread, selection, _) = handle();
    block_on(selection.select_all()).unwrap();
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = block_on(selection.read_text(cancel)).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
}

#[test]
fn select_and_clear_are_observed() {
    let (thread, selection, pages) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select(SelectionInput::Span { page: pages[0], start: 0, count: 3 }))
        .unwrap();
    block_on(selection.clear()).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    let seen = events.0.lock().unwrap();
    assert!(
        matches!(seen.first(), Some(SelectionEvent::Changed { event })
        if event.range.is_some() && event.trigger == SelectionChangeTrigger::Api),
        "{seen:?}"
    );
    assert!(seen.iter().any(|e| matches!(e, SelectionEvent::Cleared { .. })), "{seen:?}");
}

#[test]
fn a_gesture_commits_on_end() {
    let (thread, selection, pages) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let rect = block_on(selection.rects(pages[0])).unwrap()[0];
    block_on(selection.clear()).unwrap();
    let left = Point { x: rect.x + 0.5, y: rect.y + rect.height / 2.0 };
    let right = Point { x: rect.x + rect.width - 0.5, y: left.y };
    assert!(block_on(selection.begin_gesture_at(pages[0], left)).unwrap());
    assert!(block_on(selection.is_gesture_active()).unwrap());
    block_on(selection.extend_to(pages[0], right)).unwrap();
    block_on(selection.end_gesture()).unwrap();
    block_on(thread.call(|_| ())).unwrap();
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e, SelectionEvent::Committed { .. })));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, selection, _) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(selection.has_selection()).map(|_| ()).unwrap_err(),
        block_on(selection.select_all()).unwrap_err(),
        block_on(selection.read_text(CancelToken::new())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_selection_subscriptions() {
    let (thread, selection, _) = handle();
    let events = Arc::new(Events::default());
    let _sub = selection.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn a_read_cancelled_while_the_engine_reads_rejects_operation_cancelled() {
    // Page 1's text read parks until the test settles it, so the cancel
    // lands while the plugin's read is in flight, not before it starts.
    let (thread, selection, pages) = handle_with(|| {
        let mut spec = spec();
        let doc = spec["documents"].as_object_mut().unwrap().values_mut().next().unwrap();
        doc["security"] = serde_json::json!({ "allows": "*" });
        doc["responses"].as_array_mut().unwrap().insert(
            0,
            serde_json::json!({ "path": "page.text.read", "pon": 1, "defer": "text-1", "times": 1 }),
        );
        TestSpec::new(spec)
    });
    block_on(selection.ensure_loaded(pages[0])).unwrap();
    block_on(selection.select_page(pages[0])).unwrap();
    let cancel = CancelToken::new();
    let mut read = Box::pin(selection.read_text(cancel.clone()));
    let waker = futures::task::noop_waker();
    let mut cx = std::task::Context::from_waker(&waker);
    assert!(read.as_mut().poll(&mut cx).is_pending());
    // The read's job runs before this reply, and parks on the engine.
    block_on(thread.call(|_| ())).unwrap();
    cancel.cancel();
    let settled = block_on(thread.call(|k| {
        k.engine.settle(
            "text-1",
            Ok(serde_json::json!({ "text": "Hello world\r\nSecond line", "charCount": 24 })),
        )
    }))
    .unwrap();
    assert!(settled, "the read never reached the engine");
    let error = block_on(read).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    // A later read still works.
    assert!(!block_on(selection.read_text(CancelToken::new())).unwrap().is_empty());
}
