#![cfg(feature = "uniffi")]
//! The search handle through uniffi. Not compiled until `epdf-ffi` builds
//! again (Task 13.33); its assertions follow the current TypeScript.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::PageRef;
use epdf_plugin_search::ffi::{
    SearchEvent, SearchEventListener, SearchQuery, SearchResultStatus, SearchStatus,
};
use futures::executor::block_on;
use serde_json::json;

mod support;

use support::{aborts, code, engine_log, token};

fn query(text: &str) -> SearchQuery {
    SearchQuery { text: text.into(), ..Default::default() }
}

#[derive(Default)]
struct Events(Mutex<Vec<SearchEvent>>, AtomicUsize);

impl SearchEventListener for Events {
    fn on_event(&self, event: SearchEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn a_query_finds_hits_and_navigates_them() {
    let (_thread, search) = support::handle();
    let result = block_on(search.search(query("alpha"), None, token())).unwrap();
    assert_eq!(result.status, SearchResultStatus::Complete);
    assert_eq!(result.hit_count, 3);
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Complete);
    // The first hit becomes active as the hits stream in; navigation moves on.
    assert_eq!(block_on(search.active_hit_index()).unwrap(), Some(0));
    let next = block_on(search.next_hit(None)).unwrap().unwrap();
    assert_eq!(next.start, 9.0);
    assert_eq!(block_on(search.active_hit_index()).unwrap(), Some(1));
    assert_eq!(block_on(search.active_hit()).unwrap(), Some(next));
    let wrapped = block_on(search.go_to_hit(3, None)).unwrap().unwrap();
    assert_eq!(wrapped.start, 0.0);
    let previous = block_on(search.previous_hit(None)).unwrap().unwrap();
    assert_eq!(previous.page, PageRef { page_object_number: 22 });
    assert_eq!(block_on(search.query()).unwrap(), Some(query("alpha")));
}

#[test]
fn hits_carry_their_page_geometry_and_snippet() {
    let (_thread, search) = support::handle();
    block_on(search.search(query("alpha"), None, token())).unwrap();
    let page = PageRef { page_object_number: 11 };
    let on_page = block_on(search.hits(Some(page))).unwrap();
    assert_eq!(on_page.len(), 2);
    assert_eq!(block_on(search.hit_count(Some(page))).unwrap(), 2);
    assert_eq!(block_on(search.hit_count(None)).unwrap(), 3);
    assert_eq!(
        block_on(search.pages_with_hits()).unwrap(),
        vec![page, PageRef { page_object_number: 22 }]
    );
    let hit = &on_page[0];
    assert_eq!(hit.page_index, 0);
    assert_eq!(hit.count, 5.0);
    assert_eq!(hit.segments.len(), 1);
    assert_eq!(hit.segments[0].advance, 1.0);
    // The engine's page-space segment, and its rect as the bounds.
    let bounds = hit.bounds.unwrap();
    assert_eq!((bounds.x, bounds.y, bounds.width, bounds.height), (10.0, 760.0, 40.0, 20.0));
    assert_eq!(hit.segments[0].quad.upper_start.y, 760.0);
    let snippet = hit.snippet.as_ref().unwrap();
    assert_eq!(
        (snippet.before.as_str(), snippet.matched.as_str(), snippet.after.as_str()),
        ("the ", "alpha", " test")
    );
    let progress = block_on(search.progress()).unwrap();
    assert_eq!((progress.pages_searched, progress.page_count), (2.0, 2.0));
    assert_eq!(block_on(search.error()).unwrap(), None);
    assert!(block_on(search.can_search(None)).unwrap());
}

#[test]
fn no_active_hit_is_none_not_minus_one() {
    let (_thread, search) = support::handle();
    assert_eq!(block_on(search.active_hit_index()).unwrap(), None);
    assert_eq!(block_on(search.active_hit()).unwrap(), None);
    assert_eq!(block_on(search.next_hit(None)).unwrap(), None);
}

#[test]
fn a_from_page_reaches_the_engine_as_a_page_ref() {
    let (thread, search) = support::handle();
    let start = PageRef { page_object_number: 22 };
    block_on(search.search(query("alpha"), Some(start), token())).unwrap();
    let call = engine_log(&thread)
        .into_iter()
        .find(|entry| entry["path"] == "search.query")
        .expect("a search.query call");
    assert_eq!(call["args"][0]["from"], json!({"kind": "objectNumber", "objectNumber": 22}));
}

#[test]
fn find_all_answers_every_hit_and_leaves_the_session_alone() {
    let (_thread, search) = support::handle();
    let hits = block_on(search.find_all(query("alpha"), None, token())).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Idle);
    assert_eq!(block_on(search.hit_count(None)).unwrap(), 0);
}

#[test]
fn dropping_find_all_aborts_it() {
    let (thread, search) = support::handle_with_deferred_engine();
    let mut future = Box::pin(search.find_all(query("alpha"), None, token()));
    support::start(&mut future);
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(aborts(&thread), 0);
    drop(future);
    block_on(thread.call(|_| ())).unwrap();
    // findAll never touches the session; the abort reaches the engine call.
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Idle);
    assert_eq!(aborts(&thread), 1);
}

#[test]
fn cancelling_find_all_rejects_operation_cancelled() {
    let (thread, search) = support::handle_with_deferred_engine();
    let cancel = token();
    let mut future = Box::pin(search.find_all(query("alpha"), None, cancel.clone()));
    support::start(&mut future);
    block_on(thread.call(|_| ())).unwrap();
    cancel.cancel();
    let error = block_on(future).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert_eq!(aborts(&thread), 1);
}

#[test]
fn cancelling_a_search_resolves_cancelled_and_keeps_the_session() {
    let (thread, search) = support::handle_with_deferred_engine();
    let cancel = token();
    let mut future = Box::pin(search.search(query("alpha"), None, cancel.clone()));
    support::start(&mut future);
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Searching);
    cancel.cancel();
    let result = block_on(future).unwrap();
    assert_eq!(result.status, SearchResultStatus::Cancelled);
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Cancelled);
    assert_eq!(block_on(search.query()).unwrap(), Some(query("alpha")));
}

#[test]
fn cancel_and_clear_drive_the_session() {
    let (thread, search) = support::handle_with_deferred_engine();
    let mut future = Box::pin(search.search(query("alpha"), None, token()));
    support::start(&mut future);
    block_on(thread.call(|_| ())).unwrap();
    block_on(search.cancel()).unwrap();
    assert_eq!(block_on(future).unwrap().status, SearchResultStatus::Cancelled);
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Cancelled);
    block_on(search.clear()).unwrap();
    assert_eq!(block_on(search.status()).unwrap(), SearchStatus::Idle);
    assert_eq!(block_on(search.query()).unwrap(), None);
}

#[test]
fn a_session_is_observed() {
    let (_thread, search) = support::handle();
    let events = Arc::new(Events::default());
    let _subscription = search.subscribe(events.clone());
    block_on(search.search(query("alpha"), None, token())).unwrap();
    block_on(search.next_hit(None)).unwrap();
    block_on(search.clear()).unwrap();
    let seen = events.0.lock().unwrap();
    let kinds: Vec<&str> = seen
        .iter()
        .map(|event| match event {
            SearchEvent::Started { .. } => "started",
            SearchEvent::ProgressChanged { .. } => "progress",
            SearchEvent::Completed { .. } => "completed",
            SearchEvent::Cancelled { .. } => "cancelled",
            SearchEvent::Failed { .. } => "failed",
            SearchEvent::ActiveHitChanged { .. } => "active",
            SearchEvent::Cleared => "cleared",
        })
        .collect();
    // The progress, active-hit and cleared events derive from state
    // changes (`controller.ts:80-92`): the first slice makes hit 0 active,
    // and clear() puts progress and the active hit back to none.
    assert_eq!(
        kinds,
        ["started", "progress", "active", "completed", "active", "progress", "active", "cleared"]
    );
    assert!(matches!(&seen[1], SearchEvent::ProgressChanged { event } if event.hit_count == 3));
    assert!(matches!(&seen[2], SearchEvent::ActiveHitChanged { event } if event.index == Some(0)));
    assert!(matches!(&seen[3], SearchEvent::Completed { event } if event.hit_count == 3));
    assert!(matches!(
        &seen[4],
        SearchEvent::ActiveHitChanged { event } if event.index == Some(1)
            && event.hit.as_ref().map(|h| h.start) == Some(9.0)
    ));
    assert!(matches!(&seen[6], SearchEvent::ActiveHitChanged { event } if event.index.is_none()));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, search) = support::handle();
    block_on(search.search(query("alpha"), None, token())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let errors = [
        block_on(search.can_search(None)).map(|_| ()).unwrap_err(),
        block_on(search.search(query("alpha"), None, token())).map(|_| ()).unwrap_err(),
        block_on(search.refresh(token())).map(|_| ()).unwrap_err(),
        block_on(search.cancel()).unwrap_err(),
        block_on(search.clear()).unwrap_err(),
        block_on(search.next_hit(None)).map(|_| ()).unwrap_err(),
        block_on(search.previous_hit(None)).map(|_| ()).unwrap_err(),
        block_on(search.go_to_hit(0, None)).map(|_| ()).unwrap_err(),
        block_on(search.reveal_active_hit(None)).unwrap_err(),
        block_on(search.query()).map(|_| ()).unwrap_err(),
        block_on(search.status()).map(|_| ()).unwrap_err(),
        block_on(search.hits(None)).map(|_| ()).unwrap_err(),
        block_on(search.hit_count(None)).map(|_| ()).unwrap_err(),
        block_on(search.pages_with_hits()).map(|_| ()).unwrap_err(),
        block_on(search.active_hit_index()).map(|_| ()).unwrap_err(),
        block_on(search.active_hit()).map(|_| ()).unwrap_err(),
        block_on(search.progress()).map(|_| ()).unwrap_err(),
        block_on(search.error()).map(|_| ()).unwrap_err(),
        block_on(search.find_all(query("alpha"), None, token())).map(|_| ()).unwrap_err(),
    ];
    for error in errors {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, search) = support::handle();
    let events = Arc::new(Events::default());
    let subscription = search.subscribe(events.clone());
    block_on(search.search(query("alpha"), None, token())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 0);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
    subscription.cancel();
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn subscribing_after_the_close_ends_at_once() {
    let (thread, search) = support::handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    let events = Arc::new(Events::default());
    let _subscription = search.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
