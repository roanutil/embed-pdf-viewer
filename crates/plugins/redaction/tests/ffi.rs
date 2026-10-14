#![cfg(feature = "uniffi")]

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{AnnotationRef, CancelToken, KernelThread, PageRef, Rect, ViewerError};
use epdf_plugin_annotation::{AnnotationConfig, annotation_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_redaction::ffi::{
    RedactionApplyScope, RedactionEvent, RedactionEventListener, RedactionHandle,
    RedactionLabelPatch, RedactionMarkKind, RedactionPageStatus,
};
use epdf_plugin_redaction::{REDACTION, RedactionConfig, redaction_plugin};
use epdf_plugin_search::{SearchConfig, SearchQuery, search_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

thread_local! {
    /// What the engine was sent, by path, on the kernel thread.
    static SENT: RefCell<Vec<(String, Value)>> = const { RefCell::new(Vec::new()) };
}

fn sent(thread: &KernelThread<TestKernel>, path: &str) -> Vec<Value> {
    let path = path.to_string();
    block_on(thread.call(move |_| {
        SENT.with(|s| {
            s.borrow().iter().filter(|(p, _)| *p == path).map(|(_, v)| v.clone()).collect()
        })
    }))
    .unwrap()
}

fn page(pon: u32) -> Value {
    json!({ "kind": "objectNumber", "pageObjectNumber": pon })
}

fn mark_dto(annot: u32) -> Value {
    json!({ "ref": { "kind": "objectNumber", "page": page(1), "annotObjectNumber": annot },
        "page": page(1), "index": annot, "identityQuality": "durable", "nm": null,
        "flags": { "invisible": false, "hidden": false, "print": true, "noZoom": false,
            "noRotate": false, "noView": false, "readOnly": false, "locked": false,
            "toggleNoView": false, "lockedContents": false },
        "rect": { "left": 100, "bottom": 600, "right": 200, "top": 700 },
        "contents": null, "subject": null, "author": null, "created": null, "modified": null,
        "blendMode": "normal", "inReplyTo": null, "replyType": null, "subtype": "redact",
        "overlayText": "CONFIDENTIAL", "quadPoints": [], "fontColor": "#FF0000" })
}

/// A search hit on page 1, one segment.
fn hit(char_start: u32) -> Value {
    json!({ "page": page(1), "charStart": char_start, "charCount": 5,
        "segments": [{ "quad": { "p1": { "x": 10, "y": 40 }, "p2": { "x": 50, "y": 40 },
            "p3": { "x": 10, "y": 20 }, "p4": { "x": 50, "y": 20 } },
            "rect": { "left": 10, "bottom": 20, "right": 50, "top": 40 }, "advance": 1 }],
        "snippet": { "text": "the secret test", "matchStart": 4, "matchLength": 6 } })
}

fn spec(marks: Vec<Value>) -> TestSpec {
    let bx = json!({ "left": 0, "bottom": 0, "right": 600, "top": 800 });
    let snapshot = json!({ "pages": [{ "pageState": { "page": page(1) }, "annotations": marks }] });
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "services": ["redaction"],
        "pages": [{ "index": 0, "ref": page(1), "label": null, "size": { "width": 600, "height": 800 },
            "rotation": 0, "userUnit": 1, "boxes": { "media": bx, "crop": bx } }] } } }))
        .respond("annotations.listRawAll", move |_| Ok(Reply::value(snapshot.clone())))
        .respond_page("page.annotations.create", |_, args| {
            SENT.with(|s| s.borrow_mut().push(("page.annotations.create".into(), args[0].clone())));
            Ok(Reply::value(json!({ "created": mark_dto(20) })))
        })
        .respond_page("page.annotations.update", |_, args| {
            SENT.with(|s| s.borrow_mut().push(("page.annotations.update".into(), json!(args))));
            Ok(Reply::value(json!({ "updated": mark_dto(5) })))
        })
        .respond_page("page.annotations.delete", |_, _| Ok(Reply::value(json!({}))))
        .respond("redaction.apply", |args| {
            SENT.with(|s| s.borrow_mut().push(("redaction.apply".into(), args[0].clone())));
            Ok(Reply::value(json!({
                "scope": args[0],
                "results": [{ "page": page(1), "status": "applied", "removedAnnotationCount": 1 }],
                "removedAnnotationCount": 1
            })))
        })
        .respond("search.query", |_| Ok(Reply::value(json!({
            "matches": [hit(0), hit(9)], "nextCursor": null, "scannedPages": 1, "totalPages": 1 }))))
}

fn handle(marks: Vec<Value>) -> (KernelThread<TestKernel>, Arc<RedactionHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(AnnotationConfig::default()),
                search_plugin(SearchConfig::default()),
                redaction_plugin(RedactionConfig::default()),
            ],
            spec(marks),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(
        thread.call(move |k| RedactionHandle::new(k.capability(&REDACTION).unwrap(), post)),
    )
    .unwrap();
    // Hydrate the annotation plane the marks project from.
    block_on(thread.call(|k| {
        let annotation = k.capability(&epdf_plugin_annotation::ANNOTATION).unwrap();
        plugin_conformance::test_kernel::settle(&annotation.refresh())
    }))
    .unwrap()
    .unwrap();
    (thread, handle)
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

fn token() -> Arc<CancelToken> {
    CancelToken::new()
}

fn obj(annot: u32) -> AnnotationRef {
    AnnotationRef::ObjectNumber {
        page: PageRef { page_object_number: 1 },
        annot_object_number: annot,
    }
}

#[derive(Default)]
struct Events(Mutex<Vec<RedactionEvent>>, AtomicUsize);

impl RedactionEventListener for Events {
    fn on_event(&self, event: RedactionEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn pending_marks_project_from_the_annotation_plane() {
    let (_thread, redaction) = handle(vec![mark_dto(5)]);
    let pending = block_on(redaction.pending(None)).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].annotation_ref, obj(5));
    assert_eq!(pending[0].page_index, Some(0));
    assert_eq!(pending[0].kind, RedactionMarkKind::Area);
    assert_eq!(pending[0].overlay_text.as_deref(), Some("CONFIDENTIAL"));
    assert_eq!(block_on(redaction.pending_count(None)).unwrap(), 1);
}

#[test]
fn marking_an_area_creates_a_redact_annotation() {
    let (thread, redaction) = handle(vec![]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    let mark = block_on(redaction.mark_area(
        PageRef { page_object_number: 1 },
        Rect { x: 10.0, y: 10.0, width: 100.0, height: 20.0 },
        token(),
    ))
    .unwrap();
    assert_eq!(mark, obj(20));
    let created = sent(&thread, "page.annotations.create");
    assert_eq!(created.last().unwrap()["subtype"], "redact");
    let seen = events.0.lock().unwrap();
    assert!(seen.iter().any(|e| matches!(e, RedactionEvent::PendingChanged { .. })), "{seen:?}");
}

#[test]
fn marking_matches_creates_a_mark_per_hit() {
    let (thread, redaction) = handle(vec![]);
    // The fake's search answers two hits on page 1.
    let query = || SearchQuery { text: "secret".into(), ..Default::default() };
    let marks = block_on(redaction.mark_matches(query(), None, token())).unwrap();
    assert_eq!(marks.len(), 2);
    let created = sent(&thread, "page.annotations.create");
    assert_eq!(created.len(), 2);
    assert!(created.iter().all(|c| c["subtype"] == "redact"), "{created:?}");
    // A page filter that names no hit's page marks nothing.
    let none = block_on(redaction.mark_matches(
        query(),
        Some(vec![PageRef { page_object_number: 2 }]),
        token(),
    ));
    assert!(none.unwrap().is_empty());
}

#[test]
fn unmarking_a_non_mark_skips_it() {
    let (_thread, redaction) = handle(vec![mark_dto(5)]);
    let result = block_on(redaction.unmark(vec![obj(5), obj(77)], token())).unwrap();
    assert_eq!(result.applied, vec![obj(5)]);
    assert_eq!(result.skipped.len(), 1);
    assert_eq!(result.skipped[0].annotation_ref, obj(77));
    assert_eq!(result.skipped[0].reason, "not a pending redaction mark");
}

#[test]
fn a_label_edit_goes_through_the_plugins_update_label() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    block_on(redaction.update_label(
        obj(5),
        RedactionLabelPatch {
            overlay_text: Some("REDACTED".into()),
            clear_overlay_text: false,
            repeat: None,
        },
        token(),
    ))
    .unwrap();
    let patch = sent(&thread, "page.annotations.update").last().unwrap()[1].clone();
    assert_eq!(patch["overlayText"], "REDACTED");
    // The plugin carries the mark's current styling with every label edit.
    assert_eq!(patch["fontColor"], "#FF0000", "{patch}");
}

#[test]
fn applying_a_page_answers_a_typed_result_and_is_observed() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    assert!(block_on(redaction.can_apply()).unwrap());
    let result =
        block_on(redaction.apply_pages(vec![PageRef { page_object_number: 1 }], token())).unwrap();
    assert_eq!(
        result.scope,
        RedactionApplyScope::Pages { pages: vec![PageRef { page_object_number: 1 }] }
    );
    assert_eq!(result.results[0].status, RedactionPageStatus::Applied);
    assert_eq!(result.removed_annotation_count, 1);
    assert_eq!(sent(&thread, "redaction.apply").last().unwrap()["kind"], "pages");
    assert_eq!(block_on(redaction.last_result()).unwrap(), Some(result));
}

#[test]
fn applying_refs_that_are_not_pending_is_not_found() {
    let (_thread, redaction) = handle(vec![]);
    let error = block_on(redaction.apply(vec![obj(5)], token())).unwrap_err();
    assert_eq!(code(&error), Some("not-found"), "{error:?}");
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, redaction) = handle(vec![]);
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(redaction.pending(None)).map(|_| ()).unwrap_err(),
        block_on(redaction.can_apply()).map(|_| ()).unwrap_err(),
        block_on(redaction.apply_all(token())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, redaction) = handle(vec![]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_an_apply_before_it_runs_applies_nothing() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let (parked_tx, parked) = std::sync::mpsc::channel::<()>();
    let blocker = thread.call(move |_| {
        parked_tx.send(()).unwrap();
        gate.recv().unwrap();
    });
    parked.recv().unwrap();
    let cancel = token();
    let apply = redaction.apply_all(cancel.clone());
    cancel.cancel();
    release.send(()).unwrap();
    block_on(blocker).unwrap();
    let error = block_on(apply).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert!(sent(&thread, "redaction.apply").is_empty());
}

#[test]
fn a_remote_apply_arrives_as_a_typed_result() {
    let (thread, redaction) = handle(vec![]);
    let events = Arc::new(Events::default());
    let _sub = redaction.subscribe(events.clone());
    block_on(thread.call(|_| ())).unwrap();
    // A remote apply is the whole `redaction.applied` event, the result's
    // members spread into it (the vectors' shape).
    let event = json!({ "type": "redaction.applied", "origin": { "kind": "remote" },
        "scope": { "kind": "pages", "pages": [page(1)] },
        "results": [{ "page": page(1), "status": "applied", "removedAnnotationCount": 0 }],
        "removedAnnotationCount": 0 });
    block_on(thread.call(move |k| k.emit(&event))).unwrap().unwrap();
    let expected = RedactionApplyScope::Pages { pages: vec![PageRef { page_object_number: 1 }] };
    let last = block_on(redaction.last_result()).unwrap().unwrap();
    assert_eq!(last.scope, expected);
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter()
            .any(|e| matches!(e, RedactionEvent::Applied { result } if result.scope == expected)),
        "{seen:?}"
    );
}
