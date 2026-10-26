#![cfg(feature = "uniffi")]
//! The redaction handle through uniffi.
//!
//! Not compiled until `epdf-ffi` builds again (Task 13.33). Migrated by hand
//! in Task 13.23 to the current engine wire: records and refs in page space
//! (`{kind, objectNumber}` page refs), annotation writes through `doc.apply`
//! (`{ops}` plus `{opId}`, answered with `{items}` and the confirmed
//! events), `redaction.apply` scopes `{pages}` or `{annotations}`, and
//! search slices with named page-space quads. The retired `failed` page
//! status is now an `invalid-state` decode refusal: the current engine's
//! apply is all or nothing (`RedactionApplyStatus = 'applied' | 'unchanged'`).

use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{AnnotationRef, CancelToken, KernelThread, PageRef, Rect, ViewerError};
use epdf_plugin_annotation::annotation_plugin;
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
    /// The next object number a create takes.
    static NEXT: RefCell<u32> = const { RefCell::new(20) };
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
    json!({ "kind": "objectNumber", "objectNumber": pon })
}

fn annot(n: u32) -> Value {
    json!({ "kind": "objectNumber", "page": page(1), "objectNumber": n })
}

fn mark_dto(n: u32) -> Value {
    json!({
        "subtype": "redact", "ref": annot(n), "page": page(1),
        "hasAppearance": true, "appearanceState": null, "nm": null,
        "rect": { "x": 100, "y": 100, "width": 100, "height": 100 },
        "contents": null, "subject": null, "blendMode": "normal",
        "invisible": false, "hidden": false, "print": true, "noZoom": false, "noRotate": false,
        "noView": false, "readOnly": false, "locked": false, "toggleNoView": false,
        "lockedContents": false, "reply": null, "popup": null, "groupId": null, "author": null,
        "createdAt": null, "modifiedAt": null, "userId": null, "createdBy": null,
        "modifiedBy": null, "importedBy": null, "actions": null,
        "color": "#ef4444", "opacity": 1, "quadPoints": [], "interiorColor": "#000000",
        "overlayText": "CONFIDENTIAL", "repeat": false, "fontFamily": "helvetica",
        "fontSize": 0, "fontColor": "#FF0000", "textAlign": "center",
    })
}

/// A search match on page 1, one segment.
fn hit(start: u32) -> Value {
    json!({ "page": page(1), "start": start, "count": 5,
        "segments": [{ "quad": {
                "upperLeft": { "x": 10, "y": 20 }, "upperRight": { "x": 50, "y": 20 },
                "lowerLeft": { "x": 10, "y": 40 }, "lowerRight": { "x": 50, "y": 40 } },
            "rect": { "x": 10, "y": 20, "width": 40, "height": 20 }, "advance": 1 }] })
}

fn origin(op: &Value) -> Value {
    json!({ "kind": "local", "sessionId": "s1", "sub": null, "ts": 0, "serverId": null,
        "tx": { "id": op, "index": 0, "count": 1 } })
}

/// `doc.apply({ops}, {opId})`: each op answered as the engine answers it,
/// its confirmed event published first.
fn apply_ops(args: &[Value]) -> Reply {
    SENT.with(|s| s.borrow_mut().push(("apply".into(), args[0].clone())));
    let op_id = args.get(1).and_then(|o| o.get("opId")).cloned().unwrap_or(Value::Null);
    let mut items = Vec::new();
    let mut events = Vec::new();
    for op in args[0]["ops"].as_array().cloned().unwrap_or_default() {
        let meta = json!({ "affectedPages": [page(1)], "cacheDelta": null, "opId": op_id,
            "undoable": true });
        match op["type"].as_str() {
            Some("annotations.create") => {
                let n = op["objectNumber"].as_u64().map(|n| n as u32).unwrap_or_else(|| {
                    NEXT.with(|next| {
                        let n = *next.borrow();
                        *next.borrow_mut() += 1;
                        n
                    })
                });
                let mut record = mark_dto(n);
                for (k, v) in op["data"].as_object().cloned().unwrap_or_default() {
                    record[k] = v;
                }
                items.push(json!({ "type": "annotations.create", "page": page(1),
                    "annotation": record, "meta": meta }));
                events.push(json!({ "type": "annotations.created", "page": page(1),
                    "annotation": record, "meta": meta, "origin": origin(&op_id) }));
            }
            Some("annotations.update") => {
                let mut record = mark_dto(5);
                for (k, v) in op["patch"].as_object().cloned().unwrap_or_default() {
                    record[k] = v;
                }
                items.push(json!({ "type": "annotations.update", "page": page(1),
                    "annotation": record, "meta": meta }));
                events.push(json!({ "type": "annotations.updated", "page": page(1),
                    "annotation": record, "meta": meta, "origin": origin(&op_id) }));
            }
            _ => {
                items.push(json!({ "type": "annotations.delete", "page": page(1), "meta": meta }));
                events.push(json!({ "type": "annotations.deleted", "page": page(1),
                    "deleted": [op["ref"]], "meta": meta, "origin": origin(&op_id) }));
            }
        }
    }
    let meta = json!({ "affectedPages": [page(1)], "cacheDelta": null, "opId": op_id,
        "undoable": true });
    Reply { value: json!({ "items": items, "meta": meta }), events }
}

fn spec(marks: Vec<Value>) -> TestSpec {
    let bx = json!({ "x": 0, "y": 0, "width": 600, "height": 800 });
    TestSpec::new(json!({ "documents": { "a": { "id": "a", "security": { "allows": "*" },
        "services": ["redaction"],
        "objectNumbers": [20, 21, 22, 23],
        "pages": [{ "index": 0, "ref": page(1), "label": null,
            "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
            "tabOrder": null, "pdfCropBox": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
            "boxes": { "media": bx, "crop": bx, "bleed": bx, "trim": bx, "art": bx } }],
        "responses": [{ "path": "annotations.list", "value": { "annotations": marks } }] } } }))
    .respond("apply", |args| Ok(apply_ops(args)))
    .respond("redaction.apply", |args| {
        SENT.with(|s| s.borrow_mut().push(("redaction.apply".into(), args[0].clone())));
        let result = json!({
            "scope": args[0],
            "results": [{ "page": page(1), "status": "applied", "removedAnnotationCount": 1 }],
            "removedAnnotationCount": 1,
            "meta": { "affectedPages": [page(1)], "cacheDelta": null, "opId": "apply-1",
                "undoable": false },
        });
        let mut event = result.clone();
        event["type"] = json!("redaction.applied");
        event["origin"] = origin(&json!("apply-1"));
        Ok(Reply { value: result, events: vec![event] })
    })
    .respond("search.query", |_| {
        Ok(Reply::value(json!({
            "matches": [hit(0), hit(9)], "nextCursor": null, "pagesSearched": 1, "pageCount": 1 })))
    })
}

fn handle(marks: Vec<Value>) -> (KernelThread<TestKernel>, Arc<RedactionHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                annotation_plugin(None),
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
        plugin_conformance::test_kernel::settle(&annotation.when_synced())
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

fn obj(n: u32) -> AnnotationRef {
    AnnotationRef::ObjectNumber { page: PageRef { page_object_number: 1 }, annot_object_number: n }
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
    assert!(!pending[0].repeat);
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
    assert_eq!(mark.annotation_ref, obj(20));
    let ops = sent(&thread, "apply");
    assert_eq!(ops.last().unwrap()["ops"][0]["data"]["subtype"], "redact");
    // The overlay settings ride every mark: the default black fill.
    assert_eq!(ops.last().unwrap()["ops"][0]["data"]["interiorColor"], "#000000");
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
    assert!(marks.iter().all(|m| m.kind == RedactionMarkKind::Text), "{marks:?}");
    let ops = sent(&thread, "apply");
    assert_eq!(ops.len(), 2);
    assert!(ops.iter().all(|c| c["ops"][0]["data"]["quadPoints"].is_array()), "{ops:?}");
    // An empty page filter keeps no hit, so it marks nothing.
    let none = block_on(redaction.mark_matches(query(), Some(vec![]), token()));
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
    assert!(result.failed.is_empty());
}

#[test]
fn a_label_edit_goes_through_the_plugins_update_label() {
    let (thread, redaction) = handle(vec![mark_dto(5)]);
    let mark = block_on(redaction.update_label(
        obj(5),
        RedactionLabelPatch {
            overlay_text: Some("REDACTED".into()),
            clear_overlay_text: false,
            repeat: None,
        },
        token(),
    ))
    .unwrap();
    assert_eq!(mark.overlay_text.as_deref(), Some("REDACTED"));
    let patch = sent(&thread, "apply").last().unwrap()["ops"][0]["patch"].clone();
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
    assert_eq!(result.op_id.as_deref(), Some("apply-1"));
    assert_eq!(sent(&thread, "redaction.apply").last().unwrap(), &json!({ "pages": [page(1)] }));
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
    let event = json!({ "type": "redaction.applied",
        "scope": { "pages": [page(1)] },
        "results": [{ "page": page(1), "status": "unchanged", "removedAnnotationCount": 0 }],
        "removedAnnotationCount": 0,
        "meta": { "affectedPages": [page(1)], "cacheDelta": null, "opId": "remote",
            "undoable": false },
        "origin": { "kind": "remote", "sessionId": "s2", "sub": null, "ts": 0, "serverId": "9" } });
    block_on(thread.call(move |k| k.emit(&event))).unwrap().unwrap();
    let expected = RedactionApplyScope::Pages { pages: vec![PageRef { page_object_number: 1 }] };
    let last = block_on(redaction.last_result()).unwrap().unwrap();
    assert_eq!(last.scope, expected);
    assert_eq!(last.results[0].status, RedactionPageStatus::Unchanged);
    let seen = events.0.lock().unwrap();
    assert!(
        seen.iter()
            .any(|e| matches!(e, RedactionEvent::Applied { result } if result.scope == expected)),
        "{seen:?}"
    );
}

/// Was `a_failed_pages_error_decodes_from_either_spelling`: the per-page
/// `failed` status and its error left the engine with the all-or-nothing
/// apply (engine-core `RedactionApplyStatus = 'applied' | 'unchanged'`), so
/// a result that still names one is a value the engine no longer writes,
/// and the handle refuses it with `invalid-state` rather than guess.
#[test]
fn a_retired_failed_status_is_refused_as_invalid_state() {
    let (thread, redaction) = handle(vec![]);
    let event = json!({ "type": "redaction.applied",
        "scope": { "pages": [page(1)] },
        "results": [{ "page": page(1), "status": "failed", "removedAnnotationCount": 0,
            "error": { "name": "EngineError", "code": "operation-failed", "message": "boom" } }],
        "removedAnnotationCount": 0,
        "meta": { "affectedPages": [page(1)], "cacheDelta": null, "opId": "old",
            "undoable": false },
        "origin": { "kind": "remote", "sessionId": "s2", "sub": null, "ts": 0, "serverId": "9" } });
    block_on(thread.call(move |k| k.emit(&event))).unwrap().unwrap();
    let error = block_on(redaction.last_result()).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}
