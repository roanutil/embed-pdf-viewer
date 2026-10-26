//! A search handle over `TestKernel` and the fake engine: two pages (object
//! numbers 11 and 22) and a `search.query` table, answered at once or
//! parked until the test settles it.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use epdf_ffi::{CancelToken, KernelThread, ViewerError};
use epdf_plugin_search::ffi::SearchHandle;
use epdf_plugin_search::{SEARCH, SearchConfig, search_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{DOCUMENT, TestKernel};
use serde_json::{Value, json};

fn page(index: u32, pon: u32) -> Value {
    json!({
        "index": index,
        "ref": { "kind": "objectNumber", "objectNumber": pon },
        "label": null,
        "size": { "width": 600, "height": 800 },
        "rotation": 0,
        "userUnit": 1,
        "boxes": {
            "media": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "crop": { "left": 0, "bottom": 0, "right": 600, "top": 800 }
        }
    })
}

/// A match in page space, as the engine answers it: PDF y-up 20..40 on an
/// 800pt page is y-down 760..780.
fn hit(pon: u32, start: u32) -> Value {
    json!({
        "page": { "kind": "objectNumber", "objectNumber": pon },
        "start": start,
        "count": 5,
        "segments": [{
            "quad": {
                "upperLeft": { "x": 10, "y": 760 }, "upperRight": { "x": 50, "y": 760 },
                "lowerLeft": { "x": 10, "y": 780 }, "lowerRight": { "x": 50, "y": 780 }
            },
            "rect": { "x": 10, "y": 760, "width": 40, "height": 20 },
            "advance": 1
        }],
        "snippet": { "before": "the ", "match": "alpha", "after": " test" }
    })
}

/// Every `search.query` answers one slice with three hits: two on page 11,
/// one on page 22.
pub fn answering() -> Value {
    json!({
        "path": "search.query",
        "value": {
            "matches": [hit(11, 0), hit(11, 9), hit(22, 3)],
            "nextCursor": null,
            "pagesSearched": 2,
            "pageCount": 2
        }
    })
}

/// Every `search.query` stays pending under the label `slice`.
pub fn deferred() -> Value {
    json!({ "path": "search.query", "defer": "slice" })
}

pub fn spec(search_query: Value) -> Value {
    json!({
        "documents": {
            DOCUMENT: {
                "id": DOCUMENT,
                "security": { "allows": "*" },
                "pages": [page(0, 11), page(1, 22)],
                "responses": [search_query]
            }
        }
    })
}

pub fn handle_over(search_query: Value) -> (KernelThread<TestKernel>, Arc<SearchHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(vec![search_plugin(SearchConfig::default())], spec(search_query))
    })
    .unwrap();
    let post = thread.post();
    let handle =
        block_on(thread.call(move |k| SearchHandle::new(k.capability(&SEARCH).unwrap(), post)))
            .unwrap();
    (thread, handle)
}

pub fn handle() -> (KernelThread<TestKernel>, Arc<SearchHandle>) {
    handle_over(answering())
}

pub fn handle_with_deferred_engine() -> (KernelThread<TestKernel>, Arc<SearchHandle>) {
    handle_over(deferred())
}

/// What the fake engine logged: calls, and `{"abort": index}` entries.
pub fn engine_log(thread: &KernelThread<TestKernel>) -> Vec<Value> {
    block_on(thread.call(|k| k.engine.log_since(0))).unwrap()
}

pub fn aborts(thread: &KernelThread<TestKernel>) -> usize {
    engine_log(thread).iter().filter(|entry| entry.get("abort").is_some()).count()
}

pub fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

pub fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

/// Poll an export's future once, so its job reaches the kernel thread: an
/// `async fn` posts nothing until it is first polled. It must still be
/// pending.
pub fn start<F: Future>(future: &mut Pin<Box<F>>) {
    let waker = futures::task::noop_waker();
    let poll = future.as_mut().poll(&mut Context::from_waker(&waker));
    assert!(matches!(poll, Poll::Pending), "the call answered before the engine did");
}
